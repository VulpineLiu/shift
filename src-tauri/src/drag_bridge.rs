use std::thread;

#[cfg(target_os = "macos")]
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

use serde::Serialize;

#[cfg(target_os = "macos")]
use serde::Deserialize;
use tauri::{AppHandle, Emitter, LogicalPosition, Manager};

use crate::{models::FileInfo, AppState};

#[cfg(target_os = "macos")]
#[derive(Debug, Deserialize)]
struct BridgeEvent {
    #[serde(rename = "type")]
    event_type: String,
    x: Option<f64>,
    y: Option<f64>,
    files: Option<Vec<String>>,
    tools: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadialContext {
    pub files: Vec<String>,
    pub file_info: Vec<FileInfo>,
    pub mode: String,
    pub dragging: bool,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RadialEvent {
    x: f64,
    y: f64,
    mode: String,
}

#[cfg(target_os = "macos")]
pub fn resolve_bridge_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let target = std::env::consts::ARCH;
    let triple = match target {
        "aarch64" => "aarch64-apple-darwin",
        "x86_64" => "x86_64-apple-darwin",
        _ => return None,
    };
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            let sidecar = parent.join("drag-bridge");
            if sidecar.exists() {
                return Some(sidecar);
            }
        }
    }
    let dev = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("drag-bridge-{triple}"));
    if dev.exists() {
        return Some(dev);
    }
    let resource = app.path().resource_dir().ok()?;
    for candidate in [
        resource.join("drag-bridge"),
        resource.join("binaries").join("drag-bridge"),
        resource.join(format!("drag-bridge-{triple}")),
    ] {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

pub fn spawn(app: &AppHandle) {
    #[cfg(target_os = "windows")]
    {
        spawn_windows_monitor(app);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
    }
    #[cfg(target_os = "macos")]
    {
        let Some(path) = resolve_bridge_path(app) else {
            eprintln!("Shift drag bridge was not found. Shift-drag will be unavailable.");
            return;
        };
        let mut child = match Command::new(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                eprintln!("Could not start Shift drag bridge: {error}");
                return;
            }
        };
        let Some(stdout) = child.stdout.take() else {
            return;
        };
        let reader = BufReader::new(stdout);
        let handle = app.clone();
        thread::spawn(move || {
            for line in reader.lines().map_while(Result::ok) {
                if let Ok(event) = serde_json::from_str::<BridgeEvent>(&line) {
                    handle_bridge_event(&handle, event);
                }
            }
        });
        let state = app.state::<AppState>();
        if let Ok(mut bridge) = state.bridge.lock() {
            *bridge = Some(child);
        };
    }
}

#[cfg(target_os = "windows")]
fn spawn_windows_monitor(app: &AppHandle) {
    use windows_sys::Win32::{
        Foundation::POINT,
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, VK_CONTROL, VK_LBUTTON, VK_RBUTTON, VK_SHIFT,
            },
            WindowsAndMessaging::{
                GetCursorPos, GetSystemMetrics, SM_CXDRAG, SM_CYDRAG, SM_SWAPBUTTON,
            },
        },
    };

    let app = app.clone();
    thread::spawn(move || {
        let mut mouse_was_down = false;
        let mut armed = false;
        let mut tools_mode = false;
        let mut down = POINT { x: 0, y: 0 };
        let mut ignore_press = false;

        loop {
            let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
            let primary = if swapped { VK_RBUTTON } else { VK_LBUTTON };
            let mouse_down = unsafe { GetAsyncKeyState(primary as i32) } as u16 & 0x8000 != 0;
            let mut point = POINT { x: 0, y: 0 };
            unsafe {
                GetCursorPos(&mut point);
            }

            if !mouse_down {
                if armed {
                    armed = false;
                    let window = app.get_webview_window("radial");
                    if let Some(window) = window {
                        thread::spawn(move || {
                            thread::sleep(std::time::Duration::from_millis(360));
                            let _ = window.hide();
                        });
                    }
                }
                mouse_was_down = false;
                thread::sleep(std::time::Duration::from_millis(33));
                continue;
            }

            if !mouse_was_down {
                mouse_was_down = true;
                down = point;
                ignore_press = foreground_is_self();
            }
            if ignore_press {
                thread::sleep(std::time::Duration::from_millis(33));
                continue;
            }

            let shift = unsafe { GetAsyncKeyState(VK_SHIFT as i32) } as u16 & 0x8000 != 0;
            let control = unsafe { GetAsyncKeyState(VK_CONTROL as i32) } as u16 & 0x8000 != 0;
            let threshold_x = unsafe { GetSystemMetrics(SM_CXDRAG) }.max(4);
            let threshold_y = unsafe { GetSystemMetrics(SM_CYDRAG) }.max(4);

            if !armed {
                if !drag_enabled(&app) {
                    thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                if !shift {
                    thread::sleep(std::time::Duration::from_millis(33));
                    continue;
                }
                if (point.x - down.x).abs() <= threshold_x
                    && (point.y - down.y).abs() <= threshold_y
                {
                    thread::sleep(std::time::Duration::from_millis(33));
                    continue;
                }
                armed = true;
                tools_mode = control;
                let (x, y) = physical_to_logical(&app, point);
                let mode = if tools_mode { "tools" } else { "formats" };
                let _ = show_radial(&app, Vec::new(), mode, Some((x, y)), true);
            } else if shift && control != tools_mode {
                tools_mode = control;
                let mode = if tools_mode { "tools" } else { "formats" };
                let state = app.state::<AppState>();
                if let Ok(mut current) = state.radial_mode.lock() {
                    *current = mode.to_string();
                }
                if let Some(window) = app.get_webview_window("radial") {
                    let _ = window.emit("radial://mode", mode);
                }
            }

            thread::sleep(std::time::Duration::from_millis(33));
        }
    });
}

#[cfg(target_os = "windows")]
fn foreground_is_self() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut process_id);
    }
    process_id == std::process::id()
}

#[cfg(target_os = "windows")]
fn physical_to_logical(
    app: &AppHandle,
    point: windows_sys::Win32::Foundation::POINT,
) -> (f64, f64) {
    let monitors = app.available_monitors().unwrap_or_default();
    let monitor = monitors.iter().find(|monitor| {
        let x = monitor.position().x as i32;
        let y = monitor.position().y as i32;
        point.x >= x
            && point.y >= y
            && point.x <= x + monitor.size().width as i32
            && point.y <= y + monitor.size().height as i32
    });
    let scale = monitor.map(|monitor| monitor.scale_factor()).unwrap_or(1.0);
    (point.x as f64 / scale, point.y as f64 / scale)
}

/// Whether the Shift-drag gesture may open a ring. Read by both the macOS
/// bridge reader and the Windows poller, so it is not platform-gated.
#[allow(dead_code)] // unused on Linux, which has no drag monitor yet
fn drag_enabled(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| state.drag_enabled.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or(true)
}

#[cfg(target_os = "macos")]
fn handle_bridge_event(app: &AppHandle, event: BridgeEvent) {
    // The master switch only gates drag gestures; tray and window actions still work.
    if !drag_enabled(app) {
        return;
    }
    match event.event_type.as_str() {
        "show" => {
            let Some(files) = event.files else { return };
            let mode = if event.tools.unwrap_or(false) {
                "tools"
            } else {
                "formats"
            };
            let x = event.x.unwrap_or(0.0);
            let y = event.y.unwrap_or(0.0);
            let _ = show_radial(app, files, mode, Some((x, y)), true);
        }
        "move" | "end" => {
            let Some(window) = app.get_webview_window("radial") else {
                return;
            };
            let state = app.state::<AppState>();
            let origin = state.radial_origin.lock().ok().and_then(|value| *value);
            let Some((origin_x, origin_y)) = origin else {
                return;
            };
            let x = event.x.unwrap_or(origin_x + 380.0) - origin_x;
            let y = event.y.unwrap_or(origin_y + 380.0) - origin_y;
            let mode = state
                .radial_mode
                .lock()
                .ok()
                .map(|value| value.clone())
                .unwrap_or_else(|| "formats".into());
            let payload = RadialEvent { x, y, mode };
            let event_name = if event.event_type == "end" {
                "radial://drop"
            } else {
                "radial://move"
            };
            let _ = window.emit(event_name, payload);

            if event.event_type == "end" {
                let window = window.clone();
                thread::spawn(move || {
                    thread::sleep(std::time::Duration::from_millis(500));
                    let _ = window.hide();
                });
            }
        }
        "mode" => {
            let mode = if event.tools.unwrap_or(false) {
                "tools"
            } else {
                "formats"
            };
            let state = app.state::<AppState>();
            if let Ok(mut current) = state.radial_mode.lock() {
                *current = mode.to_string();
            }
            if let Some(window) = app.get_webview_window("radial") {
                let _ = window.emit("radial://mode", mode);
            }
        }
        _ => {}
    }
}

fn clamp_origin(app: &AppHandle, x: f64, y: f64) -> (f64, f64) {
    let mut origin_x = x - 380.0;
    let mut origin_y = y - 380.0;
    let monitors = app.available_monitors().unwrap_or_default();
    let monitor = monitors.iter().find(|monitor| {
        let scale = monitor.scale_factor();
        let start_x = monitor.position().x as f64 / scale;
        let start_y = monitor.position().y as f64 / scale;
        let width = monitor.size().width as f64 / scale;
        let height = monitor.size().height as f64 / scale;
        x >= start_x && x <= start_x + width && y >= start_y && y <= start_y + height
    });
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let start_x = monitor.position().x as f64 / scale;
        let start_y = monitor.position().y as f64 / scale;
        let width = monitor.size().width as f64 / scale;
        let height = monitor.size().height as f64 / scale;
        origin_x = origin_x.clamp(start_x, (start_x + width - 760.0).max(start_x));
        origin_y = origin_y.clamp(start_y, (start_y + height - 760.0).max(start_y));
    }
    (origin_x, origin_y)
}

pub fn show_radial(
    app: &AppHandle,
    files: Vec<String>,
    mode: &str,
    position: Option<(f64, f64)>,
    dragging: bool,
) -> Result<RadialContext, String> {
    let file_info = crate::files::inspect_paths(&files)?;
    let state = app.state::<AppState>();
    let (origin_x, origin_y) = if let Some((x, y)) = position {
        clamp_origin(app, x, y)
    } else {
        let monitor = app
            .primary_monitor()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "No monitor found.".to_string())?;
        let scale = monitor.scale_factor();
        let width = monitor.size().width as f64 / scale;
        let height = monitor.size().height as f64 / scale;
        ((width - 760.0) / 2.0, (height - 760.0) / 2.0)
    };
    *state
        .radial_files
        .lock()
        .map_err(|error| error.to_string())? = files.clone();
    *state
        .radial_mode
        .lock()
        .map_err(|error| error.to_string())? = mode.to_string();
    *state
        .radial_origin
        .lock()
        .map_err(|error| error.to_string())? = Some((origin_x, origin_y));
    *state
        .radial_dragging
        .lock()
        .map_err(|error| error.to_string())? = dragging;
    let context = RadialContext {
        files,
        file_info,
        mode: mode.to_string(),
        dragging,
    };

    let window = app
        .get_webview_window("radial")
        .ok_or_else(|| "Radial window is unavailable.".to_string())?;
    window
        .set_position(LogicalPosition::new(origin_x, origin_y))
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window
        .emit("radial://show", context.clone())
        .map_err(|error| error.to_string())?;
    Ok(context)
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("radial") {
        let _ = window.hide();
    }
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut dragging) = state.radial_dragging.lock() {
            *dragging = false;
        }
    }
}

#[tauri::command]
pub fn get_radial_context(
    state: tauri::State<'_, AppState>,
) -> Result<Option<RadialContext>, String> {
    let files = state
        .radial_files
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let mode = state
        .radial_mode
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    if files.is_empty() {
        return Ok(None);
    }
    Ok(Some(RadialContext {
        file_info: crate::files::inspect_paths(&files)?,
        files,
        mode,
        dragging: *state
            .radial_dragging
            .lock()
            .map_err(|error| error.to_string())?,
    }))
}

#[tauri::command]
pub fn hide_radial(app: AppHandle) -> Result<(), String> {
    hide(&app);
    Ok(())
}

#[tauri::command]
pub fn show_files_in_radial(
    app: AppHandle,
    files: Vec<String>,
    mode: Option<String>,
) -> Result<RadialContext, String> {
    if files.is_empty() {
        return Err("No files selected.".into());
    }
    show_radial(
        &app,
        files,
        mode.as_deref().unwrap_or("actions"),
        None,
        false,
    )
}

