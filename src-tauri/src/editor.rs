use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
/// The longest edge of a freshly pinned window. A pin is a reference, so it must
/// not cover the screen the user is working on until they ask for more.
const PIN_SMALL_EDGE: f64 = 520.0;
/// How much of the monitor's work area an enlarged pin may take. Leaving a margin
/// keeps the window's own edges and the desktop around it reachable.
const PIN_FIT_FRACTION: f64 = 0.9;
/// How far past its own pixels an enlarged pin may grow. The preview is at most
/// 1040 px across, so a small picture pushed any further is mush rather than detail.
const PIN_MAX_GROWTH: f64 = 3.0;
/// Whether the visible pin is currently enlarged. The window is reused rather than
/// rebuilt, so this is the only record of which of the two sizes it is showing.
static PIN_ENLARGED: AtomicBool = AtomicBool::new(false);
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

/// The window size in logical pixels that shows a `preview`-sized image, keeping
/// its aspect ratio.
///
/// Small is the freshly pinned reference: never larger than the picture itself,
/// never longer than 520 px on its longest edge. Enlarged fills up to
/// `PIN_FIT_FRACTION` of the monitor's work area and is allowed to grow past the
/// picture's own pixels, because "放大" means making it bigger to read, not
/// resampling it losslessly. `work_area` of `None` means the monitor could not be
/// read; that answers with the small size rather than a guess, since a pin sized
/// off a phantom screen is one the user cannot reach.
fn pin_window_size(
    preview: (u32, u32),
    work_area: Option<(f64, f64)>,
    enlarged: bool,
) -> (f64, f64) {
    let width = f64::from(preview.0.max(1));
    let height = f64::from(preview.1.max(1));
    let scale = match (enlarged, work_area) {
        (true, Some((area_width, area_height))) => (area_width * PIN_FIT_FRACTION / width)
            .min(area_height * PIN_FIT_FRACTION / height)
            .min(PIN_MAX_GROWTH),
        // A pin is never shrunk below the picture's own pixels at either size.
        _ => (PIN_SMALL_EDGE / width.max(height)).min(1.0),
    };
    ((width * scale).max(160.0), (height * scale).max(100.0))
}

/// The part of a monitor a window may be placed on: the screen minus the menu bar
/// and any dock, in the physical pixels the platform positions windows in.
#[derive(Clone, Copy, Debug)]
struct PinArea {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

impl PinArea {
    fn of(monitor: &tauri::Monitor) -> PinArea {
        let area = monitor.work_area();
        PinArea {
            left: f64::from(area.position.x),
            top: f64::from(area.position.y),
            width: f64::from(area.size.width),
            height: f64::from(area.size.height),
        }
    }

    /// How much of it a window may be sized to, in the logical pixels `set_size`
    /// is told in. Only the extent converts this way: the origin is asked in
    /// physical pixels and is left alone.
    fn logical_size(self, scale: f64) -> (f64, f64) {
        (self.width / scale, self.height / scale)
    }
}

/// The monitor a pin is on, with its usable area in both units and the scale factor
/// between them. `current_monitor` is preferred, with the primary monitor as the
/// answer for a window the platform cannot place; `None` when there is no monitor.
struct PinScreen {
    /// Usable extent, in the logical pixels `set_size` is told in.
    size: (f64, f64),
    /// The same rectangle, in the physical pixels `set_position` is told in.
    physical: PinArea,
    scale: f64,
}

fn pin_screen(window: &tauri::WebviewWindow) -> Option<PinScreen> {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor().max(f64::MIN_POSITIVE);
    let physical = PinArea::of(&monitor);
    Some(PinScreen {
        size: physical.logical_size(scale),
        physical,
        scale,
    })
}

/// Where to put an enlarged pin so it stays centred on the window it replaces. All
/// three arguments are physical: growing from the centre is what makes the button
/// read as zooming the picture rather than moving a window, and clamping to the work
/// area is what stops a pin parked against a screen edge from growing away off it.
fn pin_placement(size: (f64, f64), scale: f64, centre: (f64, f64), area: PinArea) -> (i32, i32) {
    let width = size.0 * scale;
    let height = size.1 * scale;
    let edge = |start: f64, length: f64, start_of_area: f64, span: f64| {
        // An oversized window sits on the near edge instead of hanging off the far one.
        if length >= span {
            start_of_area
        } else {
            start.clamp(start_of_area, start_of_area + span - length)
        }
    };
    (
        edge(centre.0 - width / 2.0, width, area.left, area.width).round() as i32,
        edge(centre.1 - height / 2.0, height, area.top, area.height).round() as i32,
    )
}

/// The middle of the window as it stands, in physical pixels, so that changing its
/// size can hold the picture in place instead of sliding it down and right.
fn pin_centre(window: &tauri::WebviewWindow) -> Option<(f64, f64)> {
    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    Some((
        f64::from(position.x) + f64::from(size.width) / 2.0,
        f64::from(position.y) + f64::from(size.height) / 2.0,
    ))
}

/// Enlarge the pin until it fills the screen, or put it back the size it arrived at.
/// One button for both directions, because an enlarged pin has to be shrunk again to
/// be usable as a reference, and a second control would spend hover space on the less
/// common half of the gesture. Returns whether the pin is now enlarged, so the page
/// can label the button the way the window looks.
#[tauri::command]
pub fn zoom_pin(app: AppHandle) -> Result<bool, String> {
    let window = app
        .get_webview_window(PIN_LABEL)
        .ok_or_else(|| "Pin window is unavailable.".to_string())?;
    let state = app.state::<AppState>();
    let preview = state
        .pinned_image
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "Nothing is pinned.".to_string())?;
    let enlarged = !PIN_ENLARGED.load(Ordering::Relaxed);
    let screen =
        pin_screen(&window).ok_or_else(|| "No display was found for this pin.".to_string())?;
    let centre = pin_centre(&window);
    let size = pin_window_size(
        (preview.width, preview.height),
        enlarged.then_some(screen.size),
        enlarged,
    );
    window
        .set_size(tauri::LogicalSize::new(size.0, size.1))
        .map_err(|error| error.to_string())?;
    if let Some(centre) = centre {
        let (x, y) = pin_placement(size, screen.scale, centre, screen.physical);
        window
            .set_position(tauri::PhysicalPosition::new(x, y))
            .map_err(|error| error.to_string())?;
    }
    PIN_ENLARGED.store(enlarged, Ordering::Relaxed);
    Ok(enlarged)
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
    // A fresh pin always opens as the small reference, whatever the last one was
    // showing, so an enlarged pin being replaced does not come back huge.
    PIN_ENLARGED.store(false, Ordering::Relaxed);
    let window = app
        .get_webview_window(PIN_LABEL)
        .ok_or_else(|| "Pin window is unavailable.".to_string())?;
    // The small size is bounded by the picture, not the screen, so it needs no monitor.
    let (window_width, window_height) =
        pin_window_size((preview.width, preview.height), None, false);
    window
        .set_size(tauri::LogicalSize::new(window_width, window_height))
        .map_err(|error| error.to_string())?;
    if let Some((cursor, monitor_scale, work_area)) =
        app.cursor_position().ok().and_then(|cursor| {
            window
                .monitor_from_point(cursor.x, cursor.y)
                .ok()
                .flatten()
                .map(|monitor| (cursor, monitor.scale_factor(), PinArea::of(&monitor)))
        })
    {
        let (x, y) = pin_placement(
            (window_width, window_height),
            monitor_scale,
            (cursor.x, cursor.y),
            work_area,
        );
        window
            .set_position(tauri::PhysicalPosition::new(x, y))
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
    use super::{flatten_for_pin, is_current_pin, pin_placement, pin_window_size, PinArea};
    use image::{DynamicImage, Rgba, RgbaImage};

    /// The screen the tests place pins on: a 1512x982 laptop with a 2x backing
    /// scale and a 37 px menu bar already taken out of the usable height.
    fn retina() -> (PinArea, f64) {
        (
            PinArea {
                left: 0.0,
                top: 37.0,
                width: 3024.0,
                height: 1890.0,
            },
            2.0,
        )
    }

    #[test]
    fn a_fresh_pin_never_exceeds_its_own_picture_or_520_px() {
        // The reference size is a ceiling, not a stretch: a small screenshot pinned
        // at 320 px must stay 320 px rather than arrive blown up and blurry.
        let small = pin_window_size((320, 200), Some((1512.0, 945.0)), false);
        assert_eq!(small, (320.0, 200.0));
        let large = pin_window_size((2400, 1200), Some((1512.0, 945.0)), false);
        assert_eq!(large, (520.0, 260.0));
        // And it is the same answer with no monitor at all, which is what a hidden
        // pin window on a headless or mid-sleep display has to fall back to.
        assert_eq!(pin_window_size((2400, 1200), None, false), large);
    }

    #[test]
    fn enlarging_fills_the_screen_but_stays_inside_it() {
        let (area, scale) = retina();
        let logical = area.logical_size(scale);
        let size = pin_window_size((1040, 520), Some(logical), true);
        assert!(
            size.0 <= logical.0 && size.1 <= logical.1,
            "{size:?} out of {logical:?}"
        );
        // 90% of the usable width is what binds for a 2:1 screenshot, and the point
        // of the button is that the picture really does grow past its preview.
        assert!(size.0 > logical.0 * 0.85, "{size:?} did not grow");
        assert!((size.0 / size.1 - 2.0).abs() < 0.01, "aspect ratio lost");
    }

    #[test]
    fn a_small_picture_is_not_blown_up_to_the_ceiling() {
        // The preview is capped at 1040 px, so an 80 px icon enlarged threefold is
        // already as much softness as this button should sell. Without the cap a
        // pinned emoji would fill the display and dissolve.
        let (area, scale) = retina();
        let size = pin_window_size((80, 80), Some(area.logical_size(scale)), true);
        assert_eq!(size, (240.0, 240.0));
    }

    #[test]
    fn an_enlarged_pin_grows_from_its_centre() {
        let (area, scale) = retina();
        let centre = (800.0, 600.0);
        let placed = pin_placement((600.0, 400.0), scale, centre, area);
        // Centre is preserved in physical pixels: 800 - (600 x 2)/2, 600 - (400 x 2)/2.
        assert_eq!(placed, (200, 200));
    }

    #[test]
    fn a_pin_against_a_screen_edge_grows_inward_not_off_screen() {
        let (area, scale) = retina();
        // Pinned hard against the top-left: enlarging must pull it back inside the
        // work area instead of pushing half the picture under the menu bar.
        let placed = pin_placement((1000.0, 800.0), scale, (100.0, 120.0), area);
        assert_eq!(placed, (0, area.top as i32));
        // And the same at the far corner, where the clamp is against the right edge.
        let far = pin_placement((1000.0, 800.0), scale, (2950.0, 1900.0), area);
        assert_eq!(
            far,
            (
                (area.width - 2000.0) as i32,
                (area.top + area.height - 1600.0) as i32
            )
        );
    }

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
