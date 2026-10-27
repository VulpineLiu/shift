use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(not(target_os = "macos"))]
use std::io::Write;

use chrono::Local;
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, State};

use crate::{
    capture, editor,
    models::{
        ActionOutcome, NormalizedRect, RecordingCapabilities, RecordingInfo, RecordingOptions,
    },
    settings, AppState,
};

pub struct RecordingSession {
    pub child: Child,
    pub stdin: Option<ChildStdin>,
    pub output: PathBuf,
    pub started_at: i64,
    /// ffmpeg quits and finalizes the file when it is asked to on stdin; a Wayland
    /// recorder has no such console, so it only finalizes on a signal.
    pub stops_with_signal: bool,
}

struct Region {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Region as an X11 geometry string, which is what `wf-recorder -g` expects.
/// Only the Linux recorder calls it; the rest of the tree keeps it alive for the test.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn x11_geometry(region: &Region) -> String {
    format!(
        "{}x{}+{}+{}",
        region.width.round() as u32,
        region.height.round() as u32,
        region.x.round() as i64,
        region.y.round() as i64,
    )
}

#[cfg(target_os = "macos")]
fn logical_region(
    app: &AppHandle,
    state: &AppState,
    rect: &NormalizedRect,
) -> Result<Region, String> {
    let session = state
        .capture
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "No capture session is active.".to_string())?;
    let source = session.source;
    let scale = source.scale_factor.max(1.0);
    let monitor_width = source.width as f64 / scale;
    let monitor_height = source.height as f64 / scale;
    let _ = app;
    Ok(Region {
        x: source.monitor_x / scale + rect.x.clamp(0.0, 1.0) * monitor_width,
        y: source.monitor_y / scale + rect.y.clamp(0.0, 1.0) * monitor_height,
        width: (rect.width.clamp(0.0, 1.0) * monitor_width).max(2.0),
        height: (rect.height.clamp(0.0, 1.0) * monitor_height).max(2.0),
    })
}

#[cfg(not(target_os = "macos"))]
fn physical_region(
    app: &AppHandle,
    state: &AppState,
    rect: &NormalizedRect,
) -> Result<Region, String> {
    let session = state
        .capture
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "No capture session is active.".to_string())?;
    let source = session.source;
    let _ = app;
    Ok(Region {
        x: source.monitor_x + rect.x.clamp(0.0, 1.0) * source.width as f64,
        y: source.monitor_y + rect.y.clamp(0.0, 1.0) * source.height as f64,
        width: (rect.width.clamp(0.0, 1.0) * source.width as f64).max(2.0),
        height: (rect.height.clamp(0.0, 1.0) * source.height as f64).max(2.0),
    })
}

/// Which recording switches this build actually honours.
///
/// `screencapture -v` has no frame-rate option, so macOS reports audio only. The
/// ffmpeg backends take `-framerate` but have no audio input wired up, so Windows and
/// Linux report frame rate only. The overlay reads this and hides whatever is a lie.
pub fn capabilities() -> RecordingCapabilities {
    #[cfg(target_os = "macos")]
    return RecordingCapabilities {
        audio: true,
        fps: false,
    };
    #[cfg(not(target_os = "macos"))]
    return RecordingCapabilities {
        audio: false,
        fps: true,
    };
}

fn recording_directory(app: &AppHandle) -> Result<PathBuf, String> {
    settings::recording_dir(app)
}

fn output_path(app: &AppHandle, extension: &str) -> Result<PathBuf, String> {
    Ok(recording_directory(app)?.join(format!(
        "Shift Recording {}.{}",
        Local::now().format("%Y-%m-%d %H.%M.%S"),
        extension
    )))
}

fn start_process(
    app: &AppHandle,
    state: &AppState,
    rect: &NormalizedRect,
    options: &RecordingOptions,
) -> Result<(Child, Option<ChildStdin>, PathBuf, bool), String> {
    #[cfg(target_os = "macos")]
    {
        // `screencapture -v` picks its own frame rate; there is nothing to pass.
        let _ = options.fps;
        let region = logical_region(app, state, rect)?;
        let output = output_path(app, "mov")?;
        let mut command = crate::process::command("/usr/sbin/screencapture");
        command.args([
            "-x",
            "-v",
            "-R",
            &format!(
                "{},{},{},{}",
                region.x.round() as i64,
                region.y.round() as i64,
                region.width.round() as u32,
                region.height.round() as u32
            ),
        ]);
        if options.audio {
            command.arg("-g");
        }
        command.arg(&output);
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start screen recording: {error}"))?;
        let mut child = child;
        let stdin = child.stdin.take();
        Ok((child, stdin, output, true))
    }

    #[cfg(not(target_os = "macos"))]
    {
        // No audio input is wired into the ffmpeg grab yet; see `capabilities`.
        let _ = options.audio;
        #[cfg(target_os = "linux")]
        {
            // Under Wayland there is no root window to grab, so ffmpeg's x11grab would
            // record a black frame. wf-recorder talks to the same wlroots/PipeWire
            // compositor grim takes stills from, and finalizes on SIGINT.
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                if let Some(recorder) = crate::files::resolve_tool("wf-recorder") {
                    let geometry = x11_geometry(&region);
                    let mut wayland = crate::process::command(recorder);
                    wayland.args(["-f", &output, "-g", &geometry]);
                    if let Some(fps) = options.fps {
                        wayland.args(["-r", &fps.to_string()]);
                    }
                    let child = wayland
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .map_err(|error| format!("Could not start wf-recorder: {error}"))?;
                    return Ok((child, None, output, true));
                }
            }
        }
        let ffmpeg = crate::files::resolve_tool("ffmpeg")
            .ok_or_else(|| "Screen recording needs ffmpeg on Windows and Linux.".to_string())?;
        let region = physical_region(app, state, rect)?;
        let output = output_path(app, "mp4")?;
        let mut command = crate::process::command(ffmpeg);
        let size = format!(
            "{}x{}",
            region.width.round() as u32,
            region.height.round() as u32
        );
        // `-framerate` is an input option for both grabbers, so it is part of the
        // input argument list rather than a separate flag after `-i`.
        let framerate = options
            .fps
            .map(|fps| vec!["-framerate".to_string(), fps.to_string()])
            .unwrap_or_default();
        #[cfg(target_os = "windows")]
        {
            let source = region.x.round() as i64;
            let top = region.y.round() as i64;
            command.args(["-y", "-f", "gdigrab"]);
            command.args(&framerate);
            command.args([
                "-offset_x",
                &source.to_string(),
                "-offset_y",
                &top.to_string(),
                "-video_size",
                &size,
                "-i",
                "desktop",
            ]);
        }
        #[cfg(target_os = "linux")]
        {
            let origin = format!(
                ":0.0+{},{}",
                region.x.round() as i64,
                region.y.round() as i64
            );
            command.args(["-y", "-f", "x11grab"]);
            command.args(&framerate);
            command.args(["-video_size", &size, "-i", &origin]);
        }
        command.args([
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-pix_fmt",
            "yuv420p",
            output.to_string_lossy().as_ref(),
        ]);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start ffmpeg: {error}"))?;
        let stdin = child.stdin.take();
        return Ok((child, stdin, output, false));
    }
}

fn ensure_capture_started(child: &mut Child, output: &PathBuf) -> Result<(), String> {
    thread::sleep(Duration::from_millis(400));
    match child.try_wait() {
        Ok(Some(status)) => {
            let _ = fs::remove_file(output);
            Err(format!(
                "Screen recording could not start ({status}). Allow Screen Recording for Shift in System Settings > Privacy & Security > Screen Recording."
            ))
        }
        Ok(None) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub fn start(
    app: &AppHandle,
    state: &AppState,
    rect: &NormalizedRect,
    options: RecordingOptions,
) -> Result<RecordingInfo, String> {
    if state
        .recording
        .lock()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("A recording is already in progress.".into());
    }
    let (mut child, stdin, output, stops_with_signal) = start_process(app, state, rect, &options)?;
    ensure_capture_started(&mut child, &output)?;
    let started_at = chrono::Utc::now().timestamp_millis();
    let info = RecordingInfo {
        output: output.to_string_lossy().to_string(),
        started_at,
    };
    *state.recording.lock().map_err(|error| error.to_string())? = Some(RecordingSession {
        child,
        stdin,
        output: output.clone(),
        started_at,
        stops_with_signal,
    });
    // The overlay goes away, but the panel stays hidden until the recording ends:
    // a restored window would be filmed inside the selected region.
    capture::dismiss_capture_for_recording(app);

    if let Some(window) = app.get_webview_window("recorder") {
        if let Ok(Some(monitor)) = app.primary_monitor() {
            let scale = monitor.scale_factor();
            let width = monitor.size().width as f64 / scale;
            let height = monitor.size().height as f64 / scale;
            let _ =
                window.set_position(LogicalPosition::new((width - 300.0) / 2.0, height - 124.0));
        }
        let _ = window.show();
        let _ = window.emit("recording://started", info.clone());
    }
    Ok(info)
}

fn stop_process(session: &mut RecordingSession) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // screencapture only finalizes the movie when interrupted.
        let _ = unsafe { libc::kill(session.child.id() as libc::pid_t, libc::SIGINT) };
    }
    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(target_os = "linux")]
        if session.stops_with_signal {
            let _ = unsafe { libc::kill(session.child.id() as libc::pid_t, libc::SIGINT) };
        } else if let Some(stdin) = session.stdin.as_mut() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
        #[cfg(not(target_os = "linux"))]
        if let Some(stdin) = session.stdin.as_mut() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match session.child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(60)),
            Ok(None) => {
                let _ = session.child.kill();
                let _ = session.child.wait();
                break;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn wait_for_output(path: &PathBuf) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut last_size = 0;
    loop {
        if let Ok(metadata) = fs::metadata(path) {
            let size = metadata.len();
            if size > 0 && size == last_size {
                return true;
            }
            last_size = size;
        }
        if Instant::now() >= deadline {
            return fs::metadata(path)
                .map(|metadata| metadata.len() > 0)
                .unwrap_or(false);
        }
        thread::sleep(Duration::from_millis(80));
    }
}

pub fn stop(app: &AppHandle, state: &AppState) -> Result<Option<RecordingInfo>, String> {
    let mut guard = state.recording.lock().map_err(|error| error.to_string())?;
    let Some(mut session) = guard.take() else {
        return Ok(None);
    };
    let stopped = stop_process(&mut session);
    let saved = stopped.is_ok() && wait_for_output(&session.output);
    if let Some(window) = app.get_webview_window("recorder") {
        // The HUD goes down first: it is what tells `restore_windows` the recording
        // has ended, and the panel comes back with it.
        let _ = window.hide();
    }
    // The recorder held the panel down so it would not be filmed; it returns whether
    // or not the file came out, or a failed recording would lose the window.
    capture::restore_windows(app);
    stopped?;
    if !saved {
        return Err(
            "No video was saved. Allow Screen Recording for Shift in System Settings > Privacy & Security > Screen Recording, then try again."
                .into(),
        );
    }
    let info = RecordingInfo {
        output: session.output.to_string_lossy().to_string(),
        started_at: session.started_at,
    };
    match crate::settings::load(app).capture_after {
        crate::settings::CaptureAfter::Editor => {
            editor::show_recording_editor(app, &info)?;
        }
        crate::settings::CaptureAfter::Card => {
            if crate::result_card::show(app, Path::new(&info.output)).is_err() {
                editor::show_recording_editor(app, &info)?;
            }
        }
    }
    crate::history::publish_result(
        app,
        ActionOutcome {
            outputs: vec![info.output.clone()],
            message: "Recording saved.".into(),
        },
    );
    Ok(Some(info))
}

pub fn stop_on_exit(app: &AppHandle, state: &AppState) {
    if let Ok(mut guard) = state.recording.lock() {
        if let Some(mut session) = guard.take() {
            let _ = stop_process(&mut session);
        }
    }
    let _ = app;
}

/// Starting a recording spawns a capture process and stopping one waits for the
/// encoder to finalize the file (up to three seconds), so neither belongs on the
/// main thread.
/// The recording switches the overlay is allowed to offer on this platform.
#[tauri::command]
pub fn recording_capabilities() -> RecordingCapabilities {
    capabilities()
}

#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    rect: NormalizedRect,
    options: Option<RecordingOptions>,
) -> Result<RecordingInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        start(&app, &state, &rect, options.unwrap_or_default())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> Result<Option<RecordingInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        stop(&app, &state)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn recording_status(state: State<'_, AppState>) -> Result<Option<RecordingInfo>, String> {
    Ok(state
        .recording
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
        .map(|session| RecordingInfo {
            output: session.output.to_string_lossy().to_string(),
            started_at: session.started_at,
        }))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn wayland_geometry_uses_the_x11_shape() {
        let region = Region {
            x: 12.6,
            y: 30.4,
            width: 800.5,
            height: 600.2,
        };
        assert_eq!(x11_geometry(&region), "801x600+13+30");
    }

    #[test]
    fn stops_recording_process_cleanly() {
        let child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut session = RecordingSession {
            child,
            stdin: None,
            output: std::env::temp_dir().join("shift-recording-test.mov"),
            started_at: 0,
            stops_with_signal: false,
        };
        let started = Instant::now();
        stop_process(&mut session).unwrap();
        assert!(started.elapsed() < Duration::from_secs(8));
        assert!(session.child.try_wait().unwrap().is_some());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_recording_helper_finalizes_a_movie() {
        if std::env::var("NEXSHIFT_GUI_TESTS").is_err() {
            return;
        }
        let directory =
            std::env::temp_dir().join(format!("shift-recording-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let output = directory.join("capture.mov");
        let mut child = Command::new("/usr/sbin/screencapture")
            .args(["-x", "-v", "-R", "10,10,160,120"])
            .arg(&output)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        thread::sleep(Duration::from_millis(900));
        let stdin = child.stdin.take();
        let mut session = RecordingSession {
            child,
            stdin,
            output: output.clone(),
            started_at: 0,
            stops_with_signal: true,
        };
        stop_process(&mut session).unwrap();
        let metadata = fs::metadata(&output).unwrap();
        assert!(metadata.len() > 0);
        fs::remove_dir_all(directory).unwrap();
    }
}
