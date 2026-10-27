use std::thread;

#[cfg(target_os = "macos")]
use std::{
    io::{BufRead, BufReader},
    process::Stdio,
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
    [
        resource.join("drag-bridge"),
        resource.join("binaries").join("drag-bridge"),
        resource.join(format!("drag-bridge-{triple}")),
    ]
    .into_iter()
    .find(|candidate| candidate.exists())
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
        let mut child = match crate::process::command(path)
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
                GetAsyncKeyState, VK_LBUTTON, VK_MENU, VK_RBUTTON, VK_SHIFT,
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
        // Gesture tracing is edge-triggered so the log stays readable on a 33ms poll.
        let mut reported_disabled = false;
        let mut reported_foreground = false;

        loop {
            let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
            let primary = if swapped { VK_RBUTTON } else { VK_LBUTTON };
            let mouse_down = unsafe { GetAsyncKeyState(primary as i32) } as u16 & 0x8000 != 0;
            let mut point = POINT { x: 0, y: 0 };
            unsafe {
                GetCursorPos(&mut point);
            }

            if !mouse_down {
                // Release ends the gesture. macOS gets an explicit `cancel` from its
                // bridge, but on Windows the ring only closes when a WebView2 OLE drop
                // or leave arrives, so a drag that Explorer never turned into an OLE
                // session (text, a shortcut, a drag started outside a shell folder)
                // would leave the ring on screen for good. Let the webview decide: it
                // knows whether OLE ever handed it files.
                if armed {
                    if let Some(window) = app.get_webview_window("radial") {
                        let _ = window.emit("radial://dragend", ());
                    }
                }
                armed = false;
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
                if !reported_foreground {
                    reported_foreground = true;
                    eprintln!("shift: press started inside Shift itself, ignoring this gesture");
                }
                thread::sleep(std::time::Duration::from_millis(33));
                continue;
            }
            reported_foreground = false;

            let shift = unsafe { GetAsyncKeyState(VK_SHIFT as i32) } as u16 & 0x8000 != 0;
            // Keep the drag gesture consistent with the Alt+Shift global shortcuts.
            let alt = unsafe { GetAsyncKeyState(VK_MENU as i32) } as u16 & 0x8000 != 0;
            let threshold_x = unsafe { GetSystemMetrics(SM_CXDRAG) }.max(4);
            let threshold_y = unsafe { GetSystemMetrics(SM_CYDRAG) }.max(4);

            if !armed {
                if !drag_enabled(&app) {
                    if !reported_disabled {
                        reported_disabled = true;
                        eprintln!("shift: drag ring is disabled in settings, ignoring the gesture");
                    }
                    thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                reported_disabled = false;
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
                tools_mode = alt;
                let (x, y) = physical_to_logical(&app, point);
                let mode = if tools_mode { "tools" } else { "formats" };
                match show_radial(&app, Vec::new(), mode, Some((x, y)), true) {
                    Ok(_) => eprintln!(
                        "shift: armed {mode} ring at logical ({x:.0}, {y:.0}) from physical ({}, {}) with threshold {threshold_x}x{threshold_y}",
                        point.x, point.y
                    ),
                    Err(error) => eprintln!("shift: could not open the {mode} ring: {error}"),
                }
            } else if shift && alt != tools_mode {
                tools_mode = alt;
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
    let logical = (point.x as f64 / scale, point.y as f64 / scale);
    // set_position(LogicalPosition) is scaled back up by tao using the *window's*
    // scale factor, not the cursor monitor's (tao window.rs set_outer_position). On a
    // mixed-DPI desktop those differ, which shifts the ring by the ratio between them,
    // so record both plus the match that produced the divisor.
    let window_scale = app
        .get_webview_window("radial")
        .and_then(|window| window.scale_factor().ok())
        .unwrap_or(1.0);
    let matched = match monitor {
        Some(monitor) => format!(
            "monitor {}x{} at {},{} scale {}",
            monitor.size().width,
            monitor.size().height,
            monitor.position().x,
            monitor.position().y,
            monitor.scale_factor()
        ),
        None => format!(
            "no monitor contained the point ({} monitors)",
            monitors.len()
        ),
    };
    eprintln!(
        "shift: ring coords physical ({}, {}) -> logical ({:.0}, {:.0}); {matched}; window scale {window_scale}",
        point.x, point.y, logical.0, logical.1
    );
    logical
}

/// Whether the Shift-drag gesture may open a ring. Read by both the macOS
/// bridge reader and the Windows poller, so it is not platform-gated.
#[allow(dead_code)] // unused on Linux, which has no drag monitor yet
fn drag_enabled(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| {
            state
                .drag_enabled
                .load(std::sync::atomic::Ordering::Relaxed)
        })
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
        "cancel" => hide(app),
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
    #[cfg(target_os = "windows")]
    {
        let title = window
            .title()
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|error| format!("error({error})"));
        let visible = window
            .is_visible()
            .map(|value| value.to_string())
            .unwrap_or_else(|error| format!("error({error})"));
        let position = window
            .outer_position()
            .map(|value| format!("{},{}", value.x, value.y))
            .unwrap_or_else(|error| format!("error({error})"));
        let size = window
            .outer_size()
            .map(|value| format!("{}x{}", value.width, value.height))
            .unwrap_or_else(|error| format!("error({error})"));
        let scale = window
            .scale_factor()
            .map(|value| value.to_string())
            .unwrap_or_else(|error| format!("error({error})"));
        eprintln!(
            "shift: radial window after show: title={title} visible={visible} position={position} size={size} scale={scale}"
        );
    }
    // A drop has already finished when the main panel opens a ring. Focusing
    // that ring enables normal hover and keyboard handling. During a live
    // Finder drag the non-activating window must stay unfocused.
    if !dragging {
        let _ = window.set_focus();
        // The window being key is not enough: AppKit delivers keys to the window's
        // first responder, which re-showing leaves on tao's own view rather than the
        // page, so Escape would never reach the ring. See `crate::focus_page`.
        crate::focus_page(app, "radial");
    }
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

/// The ring's own commands decode image previews, so they run off the main thread
/// and the ring never waits on a batch of large photos.
#[tauri::command]
pub async fn get_radial_context(app: AppHandle) -> Result<Option<RadialContext>, String> {
    tauri::async_runtime::spawn_blocking(move || radial_context(&app))
        .await
        .map_err(|error| error.to_string())?
}

fn radial_context(app: &AppHandle) -> Result<Option<RadialContext>, String> {
    let state = app.state::<AppState>();
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
    let dragging = *state
        .radial_dragging
        .lock()
        .map_err(|error| error.to_string())?;
    Ok(Some(RadialContext {
        file_info: crate::files::inspect_paths(&files)?,
        files,
        mode,
        dragging,
    }))
}

#[tauri::command]
pub fn hide_radial(app: AppHandle) -> Result<(), String> {
    hide(&app);
    Ok(())
}

#[tauri::command]
pub async fn show_files_in_radial(
    app: AppHandle,
    files: Vec<String>,
    mode: Option<String>,
) -> Result<RadialContext, String> {
    if files.is_empty() {
        return Err("No files selected.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        show_radial(
            &app,
            files,
            mode.as_deref().unwrap_or("actions"),
            None,
            false,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
