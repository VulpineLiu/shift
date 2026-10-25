use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{GenericImageView, Rgba, RgbaImage};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    files::editor_kind,
    models::{EditorInfo, RecordingInfo},
    AppState,
};

pub fn show_editor(app: &AppHandle, path: &Path) -> Result<EditorInfo, String> {
    let info = EditorInfo {
        path: path.to_string_lossy().to_string(),
        kind: editor_kind(path).to_string(),
    };
    let state = app.state::<AppState>();
    *state.editor.lock().map_err(|error| error.to_string())? = Some(info.clone());

    let window = app
        .get_webview_window("editor")
        .ok_or_else(|| "Editor window is unavailable.".to_string())?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Shift");
    window
        .set_title(&format!("{name} - Shift"))
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    window
        .emit("editor://open", info.clone())
        .map_err(|error| error.to_string())?;
    Ok(info)
}

pub fn show_recording_editor(
    app: &AppHandle,
    recording: &RecordingInfo,
) -> Result<EditorInfo, String> {
    show_editor(app, Path::new(&recording.output))
}

#[tauri::command]
pub fn get_editor_session(state: tauri::State<'_, AppState>) -> Result<Option<EditorInfo>, String> {
    Ok(state
        .editor
        .lock()
        .map_err(|error| error.to_string())?
        .clone())
}

#[tauri::command]
pub fn close_editor(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("editor") {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn open_in_editor(app: AppHandle, path: String) -> Result<EditorInfo, String> {
    show_editor(&app, Path::new(&path))
}

const PIN_LABEL: &str = "pin";
const PIN_PREVIEW_EDGE: u32 = 1040;
static PIN_SEQUENCE: AtomicU64 = AtomicU64::new(0);
/// Decoding a 100 MP image needs several hundred MiB before it is reduced to
/// the 1040 px preview. Keep the decode bounded so a large photo cannot exhaust
/// memory and stall Windows while the user is pinning it.
const PIN_MAX_PIXELS: u64 = 40_000_000;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinnedPreview {
    pub id: u64,
    pub path: String,
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

fn pin_preview(path: &Path) -> Result<PinnedPreview, String> {
    let (width, height) =
        image::image_dimensions(path).map_err(|error| format!("Could not read image: {error}"))?;
    if u64::from(width) * u64::from(height) > PIN_MAX_PIXELS {
        return Err("This image is too large to pin safely.".into());
    }
    let image = image::open(path).map_err(|error| format!("Could not read image: {error}"))?;
    let preview = flatten_for_pin(image.thumbnail(PIN_PREVIEW_EDGE, PIN_PREVIEW_EDGE));
    let (preview_width, preview_height) = preview.dimensions();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 82)
        .encode_image(&preview)
        .map_err(|error| format!("Could not prepare the pinned image: {error}"))?;
    Ok(PinnedPreview {
        id: 0,
        path: path.to_string_lossy().to_string(),
        data_url: format!("data:image/jpeg;base64,{}", STANDARD.encode(jpeg)),
        width: preview_width,
        height: preview_height,
    })
}

/// JPEG has no alpha channel. Compose transparent pixels onto white first so
/// screenshots with transparent backgrounds cannot turn black or show a blank
/// surface after the preview is encoded.
fn flatten_for_pin(image: image::DynamicImage) -> image::DynamicImage {
    if !image.color().has_alpha() {
        return image;
    }
    let rgba = image.to_rgba8();
    let mut flattened = RgbaImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = f32::from(pixel[3]) / 255.0;
        let blend = |channel: u8| -> u8 {
            (f32::from(channel) * alpha + 255.0 * (1.0 - alpha))
                .round()
                .clamp(0.0, 255.0) as u8
        };
        flattened.put_pixel(
            x,
            y,
            Rgba([blend(pixel[0]), blend(pixel[1]), blend(pixel[2]), 255]),
        );
    }
    image::DynamicImage::ImageRgba8(flattened)
}

fn show_on_main(app: &AppHandle) {
    let app_for_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        let app = app_for_main;
        if let Some(window) = app.get_webview_window(PIN_LABEL) {
            let _ = window.show();
            let _ = window.set_focus();
        }
    });
}

fn is_current_pin(requested: Option<u64>, current: Option<u64>) -> bool {
    matches!((requested, current), (Some(requested), Some(current)) if requested == current)
}

#[tauri::command]
pub async fn pin_image(app: AppHandle, path: String) -> Result<(), String> {
    let source = Path::new(&path).to_path_buf();
    let mut preview = tauri::async_runtime::spawn_blocking(move || {
        if !source.is_file() {
            return Err("The image to pin was not found.".to_string());
        }
        pin_preview(&source)
    })
    .await
    .map_err(|error| error.to_string())??;
    preview.id = PIN_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let max_edge = 520.0_f64;
    let scale = (max_edge / preview.width.max(preview.height) as f64).min(1.0);
    let window_width = (preview.width as f64 * scale).max(160.0);
    let window_height = (preview.height as f64 * scale).max(100.0);
    let window = app
        .get_webview_window(PIN_LABEL)
        .ok_or_else(|| "Pin window is unavailable.".to_string())?;
    window
        .set_size(tauri::LogicalSize::new(window_width, window_height))
        .map_err(|error| error.to_string())?;
    if let Some((cursor, monitor_scale)) = app.cursor_position().ok().and_then(|cursor| {
        window
            .monitor_from_point(cursor.x, cursor.y)
            .ok()
            .flatten()
            .map(|monitor| (cursor, monitor.scale_factor()))
    }) {
        let physical_width = window_width * monitor_scale;
        let physical_height = window_height * monitor_scale;
        window
            .set_position(tauri::PhysicalPosition::new(
                (cursor.x - physical_width / 2.0).round() as i32,
                (cursor.y - physical_height / 2.0).round() as i32,
            ))
            .map_err(|error| error.to_string())?;
    }
    let state = app.state::<AppState>();
    *state
        .pinned_image
        .lock()
        .map_err(|error| error.to_string())? = Some(preview.clone());
    // The page starts dark and pulls the preview when this window becomes
    // visible. Waiting for renderer-side decoding before showing the window
    // deadlocks on both WKWebView and WebView2 because hidden webviews throttle
    // images, animation frames and events.
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    window
        .emit("pin://show", preview)
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Called by the renderer after its bitmap has decoded or failed. A command is
/// used instead of a one-shot event so it cannot arrive before Rust listens.
/// The sequence id keeps a late reply from an older pin from revealing the
/// newer one before it has painted.
#[tauri::command]
pub fn pin_ready(app: AppHandle, id: Option<u64>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let current = state
        .pinned_image
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
        .map(|preview| preview.id);
    if !is_current_pin(id, current) {
        return Ok(());
    }
    show_on_main(&app);
    Ok(())
}

#[tauri::command]
pub fn get_pinned_image(
    state: tauri::State<'_, AppState>,
) -> Result<Option<PinnedPreview>, String> {
    Ok(state
        .pinned_image
        .lock()
        .map_err(|error| error.to_string())?
        .clone())
}

#[tauri::command]
pub async fn close_pin(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(PIN_LABEL) {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{flatten_for_pin, is_current_pin};
    use image::{DynamicImage, Rgba, RgbaImage};

    #[test]
    fn only_the_newest_pin_can_reveal_the_window() {
        assert!(is_current_pin(Some(4), Some(4)));
        assert!(!is_current_pin(None, None));
        assert!(!is_current_pin(None, Some(4)));
        assert!(!is_current_pin(Some(3), Some(4)));
        assert!(!is_current_pin(Some(4), None));
    }

    #[test]
    fn the_pin_window_is_created_before_the_first_pin() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let pin = config["app"]["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|window| window["label"] == "pin")
            .expect("the pin window must exist when the app starts");
        assert_eq!(pin["url"], "index.html?view=pin");
        assert_eq!(pin["visible"], false);
        assert_eq!(pin["transparent"], false);
        assert_eq!(pin["alwaysOnTop"], true);
    }

    #[test]
    fn transparent_pixels_are_flattened_onto_white() {
        let mut image = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 1, Rgba([10, 20, 30, 128]));
        let flattened = flatten_for_pin(DynamicImage::ImageRgba8(image)).to_rgba8();
        assert_eq!(flattened.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(flattened.get_pixel(1, 1).0, [132, 137, 142, 255]);
    }
}
