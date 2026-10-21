use std::path::Path;

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

fn hex_encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[tauri::command]
pub fn pin_image(app: AppHandle, path: String) -> Result<(), String> {
    let source = Path::new(&path);
    if !source.is_file() {
        return Err("The image to pin was not found.".into());
    }
    let (width, height) = image::image_dimensions(source)
        .map_err(|error| format!("Could not read image: {error}"))?;
    let max_edge = 520.0_f64;
    let scale = (max_edge / width.max(height) as f64).min(1.0);
    let window_width = (width as f64 * scale).max(160.0);
    let window_height = (height as f64 * scale).max(100.0);
    let label = format!("pin-{}", uuid::Uuid::new_v4());
    let url = format!("index.html?view=pin&path={}", hex_encode(&path));
    let mut builder =
        tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App(url.into()))
            .title("Shift Pin")
            .inner_size(window_width, window_height)
            .min_inner_size(120.0, 80.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .resizable(true)
            .skip_taskbar(true)
            .shadow(true);
    if let Ok(cursor) = app.cursor_position() {
        if let Ok(Some(monitor)) = app.primary_monitor() {
            let monitor_scale = monitor.scale_factor();
            builder = builder.position(
                cursor.x / monitor_scale - window_width / 2.0,
                cursor.y / monitor_scale - window_height / 2.0,
            );
        }
    }
    builder
        .build()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
