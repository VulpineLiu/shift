//! The floating card that appears the moment a capture lands.
//!
//! CleanShot's result card is the reason its screenshots end up in the right
//! place: the four things a user does next, Copy, Pin, Edit and Send, are on the
//! card and one click away, so the shot does not have to be re-found in a folder.
//! The card is a short-lived borderless window near the corner of the display the
//! capture came from, and it closes itself, so ignoring it costs nothing.

use std::{path::Path, time::Duration};

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// How long an untouched card stays up. Long enough to act, short enough that a
/// stack of screenshots does not litter the screen.
const LIFETIME: Duration = Duration::from_secs(8);
const WIDTH: f64 = 392.0;
const HEIGHT: f64 = 108.0;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CardInfo {
    path: String,
    name: String,
    /// "image" | "video", which decides whether a thumbnail or an icon is shown.
    kind: &'static str,
    /// Milliseconds before the card hides itself, so the countdown cannot disagree
    /// with the window's own timer.
    ttl_ms: u64,
}

fn kind_of(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        extension.as_str(),
        "mov" | "mp4" | "m4v" | "avi" | "mkv" | "webm"
    ) {
        "video"
    } else {
        "image"
    }
}

fn encode(info: &CardInfo) -> Result<String, String> {
    let bytes = serde_json::to_vec(info).map_err(|error| error.to_string())?;
    Ok(bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}

/// Show the card for one saved capture. A failure here must never swallow the
/// capture, so callers ignore the result.
pub fn show(app: &AppHandle, path: &Path) -> Result<(), String> {
    let info = CardInfo {
        path: path.to_string_lossy().to_string(),
        name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Shift")
            .to_string(),
        kind: kind_of(path),
        ttl_ms: LIFETIME.as_millis() as u64,
    };
    let label = format!("result-{}", uuid::Uuid::new_v4().simple());
    let mut builder = WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App(format!("index.html?view=result-card&info={}", encode(&info)?).into()),
    )
    .title("Shift")
    .inner_size(WIDTH, HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(true)
    .visible(false)
    .focused(false);

    // Bottom-right of the display the capture came from, clear of the menu bar.
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let size = monitor.size().to_logical::<f64>(monitor.scale_factor());
        builder = builder.position(
            (size.width - WIDTH - 26.0).max(12.0),
            (size.height - HEIGHT - 26.0).max(12.0),
        );
    }
    let window = builder.build().map_err(|error| error.to_string())?;
    let _ = window.show();

    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(LIFETIME);
        if let Some(window) = handle.get_webview_window(&label) {
            // Only close if the card is still up; the user may have moved on to the
            // editor, which already took the window down.
            if window.is_visible().unwrap_or(false) {
                let _ = window.close();
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recordings_are_distinguished_from_screenshots() {
        assert_eq!(kind_of(Path::new("/x/Shift Recording.mov")), "video");
        assert_eq!(kind_of(Path::new("/x/clip.mp4")), "video");
        assert_eq!(kind_of(Path::new("/x/shot.png")), "image");
        assert_eq!(kind_of(Path::new("/x/no-extension")), "image");
    }

    #[test]
    fn the_payload_survives_a_url_round_trip() {
        let info = CardInfo {
            path: "/Users/x/图片/Shift 截图.png".into(),
            name: "Shift 截图.png".into(),
            kind: "image",
            ttl_ms: 8000,
        };
        let encoded = encode(&info).unwrap();
        assert!(
            encoded.chars().all(|c| c.is_ascii_hexdigit()),
            "the query string must be URL-safe"
        );
        let bytes: Vec<u8> = (0..encoded.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16).unwrap())
            .collect();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("Shift"), "{text}");
        assert!(text.contains("截图"), "non-ascii paths must survive");
    }
}
