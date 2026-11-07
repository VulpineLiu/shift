use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Local;
use image::GenericImageView;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State};

use crate::{
    editor,
    files::app_cache_dir,
    models::{ActionOutcome, CaptureSessionInfo, CaptureSource, NormalizedRect},
    result_card, settings, AppState,
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
    Err("errScreenPermission".into())
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
        .ok_or_else(|| "errNoMonitor".to_string())
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
        let status = crate::process::command("/usr/sbin/screencapture")
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
            return Err("errCaptureBlocked".into());
        }
    }

    #[cfg(target_os = "windows")]
    {
        capture_windows_screen(monitor, &output)?;
    }

    #[cfg(target_os = "linux")]
    {
        let status = if let Some(grim) = crate::files::resolve_tool("grim") {
            if let Some(name) = monitor.name() {
                crate::process::command(grim)
                    .args(["-o", name, output.to_string_lossy().as_ref()])
                    .status()
            } else {
                crate::process::command(grim)
                    .arg(output.to_string_lossy().as_ref())
                    .status()
            }
        } else if let Some(gnome_screenshot) = crate::files::resolve_tool("gnome-screenshot") {
            crate::process::command(gnome_screenshot)
                .args(["-f", output.to_string_lossy().as_ref()])
                .status()
        } else {
            return Err("errCaptureToolMissing".into());
        };
        let status = status.map_err(|error| error.to_string())?;
        if !status.success() {
            return Err("errCaptureFailed".into());
        }
    }

    let (width, height) = match image::image_dimensions(&output) {
        Ok(dimensions) => dimensions,
        Err(error) => {
            let _ = fs::remove_file(&output);
            return Err(format!("Invalid capture: {error}"));
        }
    };
    Ok(CaptureSource {
        path: output.to_string_lossy().to_string(),
        width,
        height,
        monitor_x: monitor.position().x as f64,
        monitor_y: monitor.position().y as f64,
        scale_factor: monitor.scale_factor(),
    })
}

#[cfg(target_os = "windows")]
fn capture_windows_screen(monitor: &tauri::Monitor, output: &Path) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
        DIB_RGB_COLORS, HGDIOBJ, SRCCOPY,
    };
    use windows_sys::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    // Capture only the monitor under the cursor. Grabbing the virtual desktop
    // would make a mixed-DPI or multi-monitor setup produce an image whose
    // origin no longer matches the overlay window.
    let x = monitor.position().x;
    let y = monitor.position().y;
    let width = monitor.size().width as i32;
    let height = monitor.size().height as i32;
    unsafe {
        // Tauri's monitor bounds are physical pixels. Pin this worker thread
        // to per-monitor-v2 while calling GDI; otherwise GetDC(NULL) may use a
        // thread-scaled virtual desktop and the capture no longer aligns with
        // the overlay.
        let previous_awareness =
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let restore_awareness = || {
            if !previous_awareness.is_null() {
                SetThreadDpiAwarenessContext(previous_awareness);
            }
        };
        if width <= 0 || height <= 0 {
            restore_awareness();
            return Err("errWinScreenSize".into());
        }
        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            restore_awareness();
            return Err("errWinDesktopDC".into());
        }
        let memory = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        if memory.is_null() || bitmap.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap as HGDIOBJ);
            }
            if !memory.is_null() {
                DeleteDC(memory);
            }
            ReleaseDC(std::ptr::null_mut(), screen);
            restore_awareness();
            return Err("errWinSurfaceAlloc".into());
        }
        let previous = SelectObject(memory, bitmap as HGDIOBJ);
        let copied = BitBlt(
            memory,
            0,
            0,
            width,
            height,
            screen,
            x,
            y,
            SRCCOPY | CAPTUREBLT,
        );
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative height asks GDI for top-down rows, matching PNG.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [windows_sys::Win32::Graphics::Gdi::RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            }],
        };
        let mut pixels = vec![0_u8; width as usize * height as usize * 4];
        let rows = GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            pixels.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(memory, previous);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);

        if copied == 0 || rows == 0 {
            restore_awareness();
            return Err("errWinSurfaceCopy".into());
        }
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        let Some(image) = image::RgbaImage::from_raw(width as u32, height as u32, pixels) else {
            restore_awareness();
            return Err("errWinSurfaceBuffer".into());
        };
        let result = image
            .save_with_format(output, image::ImageFormat::Png)
            .map_err(|error| format!("Could not save the screenshot: {error}"));
        restore_awareness();
        result
    }
}

fn screenshot_directory(app: &AppHandle) -> Result<PathBuf, String> {
    settings::screenshot_dir(app)
}

/// Freeze the screen and raise the overlay for one capture mode.
///
/// The Shift windows go down before the grab so the frozen frame holds nothing of
/// Shift, and come back whether the overlay ends in a file, a cancel, or a failure.
pub fn begin_capture(app: &AppHandle, mode: &str) -> Result<CaptureSessionInfo, String> {
    ensure_screen_capture_access()?;
    hide_shift_windows(app);
    let result = begin_overlay(app, mode);
    if result.is_err() {
        // A capture that never reached the screen must not leave the panel hidden.
        restore_windows(app);
    }
    result
}

fn begin_overlay(app: &AppHandle, mode: &str) -> Result<CaptureSessionInfo, String> {
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
    // Showing the overlay is not the same as giving it the keyboard: see `focus_page`.
    crate::focus_page(app, "capture");
    window
        .emit("capture://ready", info.clone())
        .map_err(|error| error.to_string())?;
    Ok(info)
}

/// True while a region recording is on screen. `recording::stop` holds the session
/// lock while it finishes the file and restores the windows, so this reads the HUD
/// rather than that mutex — locking it from here would self-deadlock.
fn recording_active(app: &AppHandle) -> bool {
    app.get_webview_window("recorder")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

/// The labels a capture clears off the screen, so the grab holds nothing of Shift.
const CAPTURE_COVERED: [&str; 3] = ["main", "radial", "editor"];

/// Hide the Shift windows, remembering which ones were open. Without that record a
/// capture could only guess on the way back, which is how "the panel was open, I
/// took a screenshot, and the panel is gone" happens.
fn hide_shift_windows(app: &AppHandle) {
    let mut hidden = Vec::new();
    for label in CAPTURE_COVERED {
        if let Some(window) = app.get_webview_window(label) {
            if window.is_visible().unwrap_or(false) {
                hidden.push(label.to_string());
            }
            let _ = window.hide();
        }
    }
    if let Some(state) = app.try_state::<AppState>() {
        // Only a fresh capture writes the record. A second overlay raised on top of
        // the first, or a shot taken mid-recording, must leave the pending one alone
        // rather than trade the open panel for an empty list.
        let overlay_up = app
            .get_webview_window("capture")
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
        if !overlay_up && !recording_active(app) {
            if let Ok(mut restore) = state.capture_restore.lock() {
                *restore = hidden;
            }
        }
    }
}

/// Put back the windows this capture hid. Called when the capture overlay leaves,
/// whether the shot was taken, cancelled, or handed to the recorder.
pub fn restore_windows(app: &AppHandle) {
    // Mid-recording the panel stays down, or it would appear inside the shot; the
    // recorder restores it when the video is written.
    if recording_active(app) {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(mut restore) = state.capture_restore.lock() else {
        return;
    };
    let mut restored_main = false;
    for label in restore.drain(..) {
        restored_main |= label == "main";
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.show();
        }
    }
    // The panel the capture came from is the thing the user was working in, so it
    // takes the keyboard back. A result card or editor opened after this still
    // focuses itself, so it wins when it is the more recent request.
    if restored_main {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_focus();
            crate::focus_page(app, "main");
        }
    }
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
        return Err("errNoSelection".into());
    }
    let cropped = image.crop_imm(x, y, crop_width.min(width - x), crop_height.min(height - y));
    let directory = screenshot_directory(app)?;
    let stem = format!("Shift-{}", Local::now().format("%Y%m%d-%H%M%S"));
    let output = unique_capture_path(&directory, &stem)?;
    cropped
        .save_with_format(&output, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(output)
}

fn unique_capture_path(directory: &Path, stem: &str) -> Result<PathBuf, String> {
    crate::files::unique_output_in(Path::new(stem), Some(directory), "", "png")
}

pub fn dismiss_capture(app: &AppHandle) {
    close_overlay(app, true)
}

/// End the overlay but leave the panel hidden, for the recorder: a restored window
/// would land inside the region being filmed. `recording::stop` restores it instead.
pub fn dismiss_capture_for_recording(app: &AppHandle) {
    close_overlay(app, false)
}

fn close_overlay(app: &AppHandle, restore: bool) {
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
    if restore {
        restore_windows(app);
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

/// Capture work runs off the main thread: freezing a screen, decoding the grab
/// and writing the crop all take longer than a frame budget.
#[tauri::command]
pub async fn start_capture(app: AppHandle, mode: String) -> Result<CaptureSessionInfo, String> {
    tauri::async_runtime::spawn_blocking(move || begin_capture(&app, &mode))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn cancel_capture(app: AppHandle) -> Result<(), String> {
    dismiss_capture(&app);
    Ok(())
}

#[tauri::command]
pub async fn capture_crop(app: AppHandle, rect: NormalizedRect) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        finish_capture(&app, &state, &rect)
    })
    .await
    .map_err(|error| error.to_string())?
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
    hand_off(app, &output);
    crate::history::publish_result(
        app,
        ActionOutcome {
            outputs: vec![output.to_string_lossy().to_string()],
            message: "outcomeSaved".into(),
        },
    );
    Ok(output.to_string_lossy().to_string())
}

/// Show the result card for a finished capture. The card is allowed to fail
/// silently: a screenshot that saved is more important than an overlay that could
/// not be built, and the file is already on disk and copied by then. The editor is
/// the fallback so a capture is never left with no way to act on it.
fn hand_off(app: &AppHandle, output: &Path) {
    if result_card::show(app, output).is_err() {
        let _ = editor::show_editor(app, output);
    }
}

pub fn capture_fullscreen_core(app: &AppHandle, state: &AppState) -> Result<String, String> {
    ensure_screen_capture_access()?;
    hide_shift_windows(app);
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
pub async fn capture_fullscreen(app: AppHandle) -> Result<String, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let result = capture_fullscreen_core(&app, &state);
        if result.is_err() {
            restore_windows(&app);
        }
        result
    })
    .await
    .map_err(|error| error.to_string())?;
    result
}

#[tauri::command]
pub async fn copy_file_to(source: String, destination: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        fs::copy(Path::new(&source), Path::new(&destination))
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn open_screen_recording_settings() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(open_screen_recording_settings_sync)
        .await
        .map_err(|error| error.to_string())?
}

fn open_screen_recording_settings_sync() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = crate::process::command("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("Could not open Screen Recording settings.".into())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Screen Recording settings are macOS-specific.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_captures_in_the_same_second_do_not_overwrite() {
        let directory =
            std::env::temp_dir().join(format!("shift-capture-name-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let stem = "Shift-20260101-120000";
        let first = unique_capture_path(&directory, stem).unwrap();
        fs::write(&first, b"first").unwrap();
        let second = unique_capture_path(&directory, stem).unwrap();
        assert_ne!(first, second);
        assert_eq!(second.file_name().unwrap(), "Shift-20260101-120000 2.png");
        assert_eq!(fs::read(&first).unwrap(), b"first");
        fs::remove_dir_all(directory).ok();
    }
}
