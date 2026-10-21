use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use chrono::Local;
use image::GenericImageView;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State};

use crate::{
    editor,
    files::app_cache_dir,
    models::{ActionOutcome, CaptureSessionInfo, CaptureSource, NormalizedRect},
    settings, AppState,
};

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[cfg(target_os = "macos")]
fn ensure_screen_capture_access() -> Result<(), String> {
    unsafe {
        if screen_capture_access_allowed() || CGRequestScreenCaptureAccess() {
            return Ok(());
        }
    }
    Err(
        "Screen Recording permission is required. Enable Shift in System Settings > Privacy & Security > Screen Recording, then restart Shift if macOS asks."
            .into(),
    )
}

#[cfg(not(target_os = "macos"))]
fn ensure_screen_capture_access() -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn screen_capture_access_allowed() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

#[cfg(not(target_os = "macos"))]
fn screen_capture_access_allowed() -> bool {
    true
}

#[tauri::command]
pub fn screen_capture_access_status() -> bool {
    screen_capture_access_allowed()
}

fn monitor_at_cursor(app: &AppHandle) -> Result<tauri::Monitor, String> {
    let cursor = app.cursor_position().map_err(|error| error.to_string())?;
    let monitors = app
        .available_monitors()
        .map_err(|error| error.to_string())?;
    monitors
        .into_iter()
        .find(|monitor| {
            let x = monitor.position().x as f64;
            let y = monitor.position().y as f64;
            cursor.x >= x
                && cursor.y >= y
                && cursor.x <= x + monitor.size().width as f64
                && cursor.y <= y + monitor.size().height as f64
        })
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or_else(|| "No monitor found under the cursor.".to_string())
}

fn capture_screen(app: &AppHandle, monitor: &tauri::Monitor) -> Result<CaptureSource, String> {
    let directory = app_cache_dir(app)?;
    let filename = format!("screen-{}.png", Local::now().format("%Y%m%d-%H%M%S-%3f"));
    let output = directory.join(filename);

    #[cfg(target_os = "macos")]
    {
        let scale = monitor.scale_factor();
        let region = format!(
            "{},{},{},{}",
            (monitor.position().x as f64 / scale).round() as i64,
            (monitor.position().y as f64 / scale).round() as i64,
            (monitor.size().width as f64 / scale).round() as u32,
            (monitor.size().height as f64 / scale).round() as u32,
        );
        let status = Command::new("/usr/sbin/screencapture")
            .args([
                "-x",
                "-R",
                &region,
                "-t",
                "png",
                output.to_string_lossy().as_ref(),
            ])
            .status()
            .map_err(|error| format!("Could not launch screencapture: {error}"))?;
        if !status.success() {
            return Err(
                "Screen capture was cancelled or blocked. Allow Screen Recording in System Settings."
                    .into(),
            );
        }
    }

    #[cfg(target_os = "windows")]
    {
        let script = format!(
            r#"Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$point = [System.Windows.Forms.Cursor]::Position
$screen = [System.Windows.Forms.Screen]::FromPoint($point)
$bounds = $screen.Bounds
$bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$bitmap.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()"#,
            output.to_string_lossy().replace('\'', "''")
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .status()
            .map_err(|error| format!("Could not launch screen capture: {error}"))?;
        if !status.success() || !output.exists() {
            return Err("Screen capture failed.".into());
        }
    }

    #[cfg(target_os = "linux")]
    {
        let status = if let Some(grim) = crate::files::resolve_tool("grim") {
            if let Some(name) = monitor.name() {
                Command::new(grim)
                    .args(["-o", name, output.to_string_lossy().as_ref()])
                    .status()
            } else {
                Command::new(grim)
                    .arg(output.to_string_lossy().as_ref())
                    .status()
            }
        } else if let Some(gnome_screenshot) = crate::files::resolve_tool("gnome-screenshot") {
            Command::new(gnome_screenshot)
                .args(["-f", output.to_string_lossy().as_ref()])
                .status()
        } else {
            return Err("Install grim or gnome-screenshot to capture the screen on Linux.".into());
        };
        let status = status.map_err(|error| error.to_string())?;
        if !status.success() {
            return Err("Screen capture failed.".into());
        }
    }

    let (width, height) =
        image::image_dimensions(&output).map_err(|error| format!("Invalid capture: {error}"))?;
    Ok(CaptureSource {
        path: output.to_string_lossy().to_string(),
        width,
        height,
        monitor_x: monitor.position().x as f64,
        monitor_y: monitor.position().y as f64,
        scale_factor: monitor.scale_factor(),
    })
}

fn screenshot_directory(app: &AppHandle) -> Result<PathBuf, String> {
    settings::screenshot_dir(app)
}

pub fn begin_capture(app: &AppHandle, mode: &str) -> Result<CaptureSessionInfo, String> {
    ensure_screen_capture_access()?;
    for label in ["main", "radial", "editor"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(90));
    let monitor = monitor_at_cursor(app)?;
    let source = capture_screen(app, &monitor)?;
    let info = CaptureSessionInfo {
        mode: mode.to_string(),
        source: source.clone(),
    };
    let state = app.state::<AppState>();
    *state.capture.lock().map_err(|error| error.to_string())? = Some(info.clone());

    let window = app
        .get_webview_window("capture")
        .ok_or_else(|| "Capture window is unavailable.".to_string())?;
    let scale = monitor.scale_factor();
    window
        .set_position(LogicalPosition::new(
            monitor.position().x as f64 / scale,
            monitor.position().y as f64 / scale,
        ))
        .map_err(|error| error.to_string())?;
    window
        .set_size(LogicalSize::new(
            monitor.size().width as f64 / scale,
            monitor.size().height as f64 / scale,
        ))
        .map_err(|error| error.to_string())?;
    #[cfg(target_os = "macos")]
    {
        window
            .set_simple_fullscreen(true)
            .map_err(|error| error.to_string())?;
    }
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    window
        .emit("capture://ready", info.clone())
        .map_err(|error| error.to_string())?;
    Ok(info)
}

pub fn crop_session(
    app: &AppHandle,
    state: &AppState,
    rect: &NormalizedRect,
) -> Result<PathBuf, String> {
    let session = state
        .capture
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "No capture session is active.".to_string())?;
    let image = image::open(&session.source.path)
        .map_err(|error| format!("Could not read the screenshot: {error}"))?;
    let (width, height) = image.dimensions();
    let x = (rect.x.clamp(0.0, 1.0) * width as f64).round() as u32;
    let y = (rect.y.clamp(0.0, 1.0) * height as f64).round() as u32;
    let crop_width = (rect.width.clamp(0.0, 1.0) * width as f64).round() as u32;
    let crop_height = (rect.height.clamp(0.0, 1.0) * height as f64).round() as u32;
    if crop_width < 2 || crop_height < 2 || x >= width || y >= height {
        return Err("Select an area first.".into());
    }
    let cropped = image.crop_imm(x, y, crop_width.min(width - x), crop_height.min(height - y));
    let directory = screenshot_directory(app)?;
    let name = format!("Shift-{}.png", Local::now().format("%Y%m%d-%H%M%S"));
    let output = directory.join(name);
    cropped
        .save_with_format(&output, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(output)
}

pub fn dismiss_capture(app: &AppHandle) {
    let source = app
        .try_state::<AppState>()
        .and_then(|state| {
            state
                .capture
                .lock()
                .ok()
                .and_then(|mut session| session.take())
        })
        .map(|session| session.source.path);
    if let Some(window) = app.get_webview_window("capture") {
        #[cfg(target_os = "macos")]
        let _ = window.set_simple_fullscreen(false);
        let _ = window.hide();
    }
    if let Some(path) = source {
        let _ = fs::remove_file(path);
    }
}

#[tauri::command]
pub fn get_capture_session(
    state: State<'_, AppState>,
) -> Result<Option<CaptureSessionInfo>, String> {
    Ok(state
        .capture
        .lock()
        .map_err(|error| error.to_string())?
        .clone())
}

#[tauri::command]
pub fn start_capture(app: AppHandle, mode: String) -> Result<CaptureSessionInfo, String> {
    begin_capture(&app, &mode)
}

#[tauri::command]
pub fn cancel_capture(app: AppHandle) -> Result<(), String> {
    dismiss_capture(&app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub fn capture_crop(
    app: AppHandle,
    state: State<'_, AppState>,
    rect: NormalizedRect,
) -> Result<String, String> {
    finish_capture(&app, &state, &rect)
}

fn finish_capture(
    app: &AppHandle,
    state: &AppState,
    rect: &NormalizedRect,
) -> Result<String, String> {
    let output = crop_session(app, state, rect)?;
    {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        if let Ok(image) = tauri::image::Image::from_path(&output) {
            let _ = app.clipboard().write_image(&image);
        }
    }
    dismiss_capture(app);
    editor::show_editor(app, &output)?;
    let _ = app.emit(
        "app://file-result",
        ActionOutcome {
            outputs: vec![output.to_string_lossy().to_string()],
            message: "Screenshot saved.".into(),
        },
    );
    Ok(output.to_string_lossy().to_string())
}

pub fn capture_fullscreen_core(app: &AppHandle, state: &AppState) -> Result<String, String> {
    ensure_screen_capture_access()?;
    for label in ["main", "radial", "editor"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(90));
    let monitor = monitor_at_cursor(app)?;
    let source = capture_screen(app, &monitor)?;
    *state.capture.lock().map_err(|error| error.to_string())? = Some(CaptureSessionInfo {
        mode: "screenshot".into(),
        source,
    });
    finish_capture(
        app,
        state,
        &NormalizedRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
    )
}

#[tauri::command]
pub fn capture_fullscreen(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let result = capture_fullscreen_core(&app, &state);
    if result.is_err() {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    result
}

#[tauri::command]
pub fn copy_file_to(source: String, destination: String) -> Result<(), String> {
    fs::copy(Path::new(&source), Path::new(&destination))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_screen_recording_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            return Ok(());
        }
        return Err("Could not open Screen Recording settings.".into());
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Screen Recording settings are macOS-specific.".into())
    }
}
