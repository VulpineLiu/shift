mod autostart;
mod capture;
mod drag_bridge;
mod editor;
mod files;
mod history;
mod hotkeys;
mod locale;
mod markdown;
mod models;
mod office;
mod pdf;
mod process;
mod recording;
mod result_card;
mod runtime;
mod settings;
mod share;
mod update;

use std::{
    process::Child,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, RunEvent, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use models::{CaptureSessionInfo, EditorInfo};
use recording::RecordingSession;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HeadlessActionRequest {
    action: String,
    paths: Vec<String>,
    target: Option<String>,
    output_dir: Option<String>,
    report: String,
}

pub struct AppState {
    pub capture: Mutex<Option<CaptureSessionInfo>>,
    pub recording: Mutex<Option<RecordingSession>>,
    pub editor: Mutex<Option<EditorInfo>>,
    pub radial_files: Mutex<Vec<String>>,
    pub radial_mode: Mutex<String>,
    pub radial_origin: Mutex<Option<(f64, f64)>>,
    pub radial_dragging: Mutex<bool>,
    pub bridge: Mutex<Option<Child>>,
    /// Mirrors settings.drag_enabled so the drag monitors can check it cheaply.
    pub drag_enabled: AtomicBool,
    pub pinned_image: Mutex<Option<editor::PinnedPreview>>,
    /// Shift windows a capture hid, so ending the capture can put them back.
    pub capture_restore: Mutex<Vec<String>>,
}

/// The language the native tray is showing. The webview sends this before any
/// settings disk read is needed, so a language click never waits on storage and
/// the tray never rebuilds from a stale file snapshot.
static TRAY_LANGUAGE: OnceLock<Mutex<&'static str>> = OnceLock::new();

fn tray_language() -> &'static str {
    TRAY_LANGUAGE
        .get()
        .and_then(|language| language.lock().ok().map(|value| *value))
        .unwrap_or_else(locale::system_language)
}

pub(crate) fn set_tray_language(next: &'static str) {
    let holder = TRAY_LANGUAGE.get_or_init(|| Mutex::new(next));
    if let Ok(mut current) = holder.lock() {
        *current = next;
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            capture: Mutex::new(None),
            recording: Mutex::new(None),
            editor: Mutex::new(None),
            radial_files: Mutex::new(Vec::new()),
            radial_mode: Mutex::new("actions".into()),
            radial_origin: Mutex::new(None),
            radial_dragging: Mutex::new(false),
            drag_enabled: AtomicBool::new(true),
            bridge: Mutex::new(None),
            pinned_image: Mutex::new(None),
            capture_restore: Mutex::new(Vec::new()),
        }
    }
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Hand the keyboard to the page itself.
///
/// `WebviewWindow::set_focus` only makes the window key, and AppKit keeps sending
/// key events to whatever the window's first responder is. On macOS that is tao's
/// own content view: wry injects the WKWebView as a *subview* and points the
/// responder at it once, when the web view is built, and every later
/// `set_simple_fullscreen` (its `toggle_style_mask`) or window re-show moves it
/// back to `TaoView`. The page then never receives Escape or Enter, which is why
/// the capture overlay cannot be cancelled by keyboard and the ring ignores it.
pub(crate) fn focus_page(app: &AppHandle, label: &str) {
    if let Some(webview) = app.get_webview(label) {
        let _ = webview.set_focus();
    }
}

fn report_error(app: &AppHandle, message: impl Into<String>) {
    let message = message.into();
    eprintln!("{message}");
    let _ = app.emit("app://error", message);
}

/// Run capture work off the main thread. The closure gets its own `AppHandle`,
/// so the thread outlives the tray or hotkey callback that started it.
fn spawn_capture_work(
    app: &AppHandle,
    work: impl FnOnce(AppHandle) -> Result<(), String> + Send + 'static,
) {
    let worker = app.clone();
    let reporter = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || work(worker)).await;
        if let Err(error) = result.unwrap_or_else(|_| Err("Capture was interrupted.".into())) {
            report_error(&reporter, error);
            show_main(&reporter);
        }
    });
}

fn start_capture_from_event(app: &AppHandle, mode: &str) {
    let mode = mode.to_string();
    spawn_capture_work(app, move |app| {
        capture::begin_capture(&app, &mode).map(|_| ())
    });
}

fn start_fullscreen_capture(app: &AppHandle) {
    spawn_capture_work(app, move |app| {
        let state = app.state::<AppState>();
        capture::capture_fullscreen_core(&app, &state).map(|_| ())
    });
}

fn toggle_recording(app: &AppHandle) {
    let recording = app
        .try_state::<AppState>()
        .and_then(|state| state.recording.lock().ok().map(|session| session.is_some()))
        .unwrap_or(false);
    if !recording {
        start_capture_from_event(app, "recording");
        return;
    }
    // Stopping waits for the encoder to finalize the file, up to a few seconds.
    spawn_capture_work(app, move |app| {
        let state = app.state::<AppState>();
        recording::stop(&app, &state).map(|_| ())
    });
}

/// The tray belongs to Rust, so it cannot read the language from the webview
/// bundle. Build it from the in-memory language; never touch settings.json from
/// the native UI path.
fn tray_menu(app: &AppHandle, language: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let titles = locale::tray_titles(language);
    let open = MenuItem::with_id(app, "open", titles.open, true, None::<&str>)?;
    let capture = MenuItem::with_id(app, "capture", titles.capture, true, Some("Alt+Shift+2"))?;
    let fullscreen = MenuItem::with_id(
        app,
        "fullscreen",
        titles.fullscreen,
        true,
        Some("Alt+Shift+4"),
    )?;
    let record = MenuItem::with_id(app, "record", titles.record, true, Some("Alt+Shift+3"))?;
    let ocr = MenuItem::with_id(app, "ocr", titles.ocr, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", titles.quit, true, Some("CmdOrCtrl+Q"))?;
    let mut items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        vec![&open, &capture, &fullscreen, &record, &ocr];
    // A known newer release gets its own row, so the update is reachable without
    // opening a window. It disappears again once the check reports "current".
    let update = match update::tray_title(app) {
        Some(title) => Some(MenuItem::with_id(app, "update", title, true, None::<&str>)?),
        None => None,
    };
    if let Some(update) = &update {
        items.push(update);
    }
    items.push(&separator);
    items.push(&quit);
    let menu = Menu::with_items(app, &items)?;
    Ok(menu)
}

/// Rebuild the small menu and hand it to the tray in one operation. Updating
/// each item separately waits once per item on the native main thread; a single
/// `set_menu` reduces that to one hop.
pub fn refresh_tray_language(app: &AppHandle) -> Result<(), String> {
    let Some(tray) = app.tray_by_id("shift") else {
        return Ok(());
    };
    let language = tray_language();
    let menu = tray_menu(app, language).map_err(|error| error.to_string())?;
    tray.set_menu(Some(menu))
        .map_err(|error| format!("Could not update the tray menu: {error}"))?;
    Ok(())
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let persisted = settings::load(app);
    let language = locale::preferred(Some(&persisted.language));
    set_tray_language(language);
    let menu = tray_menu(app, language)?;

    let mut tray = TrayIconBuilder::with_id("shift")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Shift")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "capture" => start_capture_from_event(app, "screenshot"),
            "fullscreen" => start_fullscreen_capture(app),
            "record" => toggle_recording(app),
            "ocr" => {
                show_main(app);
                let _ = app.emit("app://open-ocr", ());
            }
            "update" => {
                show_main(app);
                let _ = app.emit("app://open-updates", ());
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Take back the capture hotkeys after duplicate copies were quit. macOS routes a
/// hotkey to whichever client registered it first, so the survivor must register again.
#[tauri::command]
async fn reclaim_hotkeys(app: AppHandle) -> hotkeys::HotkeyStatus {
    tauri::async_runtime::spawn_blocking(move || {
        // Take the instance lock as well, so this copy is the owner once the
        // others die.
        let _ = hotkeys::acquire_instance(&app);
        for shortcut in hotkeys::HOTKEYS {
            let _ = app.global_shortcut().unregister(shortcut);
        }
        let _ = register_shortcuts(&app);
        hotkeys::status()
    })
    .await
    .unwrap_or_else(|_| hotkeys::status())
}

fn shortcut_action(shortcut: &Shortcut) -> Option<&'static str> {
    if shortcut.matches(Modifiers::ALT | Modifiers::SHIFT, Code::Digit2) {
        Some("capture")
    } else if shortcut.matches(Modifiers::ALT | Modifiers::SHIFT, Code::Digit3) {
        Some("record")
    } else if shortcut.matches(Modifiers::ALT | Modifiers::SHIFT, Code::Digit4) {
        Some("fullscreen")
    } else {
        None
    }
}

fn register_shortcuts(app: &AppHandle) -> tauri::Result<()> {
    let mut all_ok = true;
    for shortcut in ["Alt+Shift+2", "Alt+Shift+3", "Alt+Shift+4"] {
        match app.global_shortcut().register(shortcut) {
            Ok(()) => {
                eprintln!("Registered global shortcut: {shortcut}");
            }
            Err(error) => {
                eprintln!("Could not register global shortcut {shortcut}: {error}");
                all_ok = false;
            }
        }
    }
    hotkeys::refresh_live(all_ok);
    Ok(())
}

/// Runs one file action without starting Tauri. This exists for unattended
/// Windows integration tests, where the conversion engines must be exercised
/// independently from the WebView and radial gesture.
fn run_headless_action() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--headless-action") {
        return None;
    }
    let Some(request_path) = args.next() else {
        return Some(2);
    };
    let request = match std::fs::read_to_string(&request_path)
        .map_err(|error| error.to_string())
        .and_then(|text| {
            serde_json::from_str::<HeadlessActionRequest>(&text).map_err(|error| error.to_string())
        }) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("headless action request failed: {error}");
            return Some(2);
        }
    };
    let output_dir = request.output_dir.as_deref().map(std::path::Path::new);
    let result = files::perform_action(
        &request.action,
        &request.paths,
        request.target.as_deref(),
        output_dir,
        None,
    );
    let report = match &result {
        Ok(outcome) => serde_json::json!({
            "ok": true,
            "outcome": outcome,
            "error": null,
        }),
        Err(error) => serde_json::json!({
            "ok": false,
            "outcome": null,
            "error": error,
        }),
    };
    let report_path = std::path::Path::new(&request.report);
    if let Some(parent) = report_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let payload = match serde_json::to_vec_pretty(&report) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("headless action report serialization failed: {error}");
            return Some(2);
        }
    };
    if let Err(error) = std::fs::write(report_path, payload) {
        eprintln!("headless action report failed: {error}");
        return Some(2);
    }
    Some(if result.is_ok() { 0 } else { 1 })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Some(code) = run_headless_action() {
        std::process::exit(code);
    }

    let shortcut_plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            match shortcut_action(shortcut) {
                Some("capture") => start_capture_from_event(app, "screenshot"),
                Some("fullscreen") => start_fullscreen_capture(app),
                Some("record") => toggle_recording(app),
                _ => {}
            }
        })
        .build();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(shortcut_plugin)
        .manage(AppState::default())
        .manage(share::ShareState::default())
        .manage(history::HistoryState::default())
        .manage(update::UpdateState::default())
        .invoke_handler(tauri::generate_handler![
            files::inspect_files,
            files::perform_file_action,
            files::reveal_file,
            files::open_file,
            files::read_text_file,
            files::read_image_data,
            files::save_image_data,
            files::render_pdf_pages,
            files::cleanup_pdf_pages,
            files::save_text_near,
            history::history_list,
            history::history_record,
            history::history_set_pinned,
            history::history_remove,
            history::history_import,
            history::history_clear,
            settings::get_app_settings,
            settings::set_output_directory,
            settings::set_file_output_mode,
            settings::set_auto_reveal,
            settings::set_drag_enabled,
            settings::set_share_enabled,
            settings::set_autostart,
            settings::check_output_access,
            settings::set_language,
            settings::set_update_check_enabled,
            settings::set_setup_reviewed,
            settings::get_wedge_orders,
            settings::reveal_output_directory,
            update::update_check,
            update::update_status,
            update::update_download,
            update::update_install,
            update::update_clear,
            settings::open_files_permission_settings,
            runtime::machine_info,
            runtime::install_runtime_component,
            capture::get_capture_session,
            capture::screen_capture_access_status,
            capture::start_capture,
            capture::cancel_capture,
            capture::capture_crop,
            capture::capture_fullscreen,
            capture::copy_file_to,
            capture::open_screen_recording_settings,
            recording::recording_capabilities,
            recording::start_recording,
            recording::stop_recording,
            recording::recording_status,
            hotkeys::hotkey_status,
            reclaim_hotkeys,
            hotkeys::quit_other_instances,
            editor::get_editor_session,
            editor::close_editor,
            editor::open_in_editor,
            editor::pin_image,
            editor::get_pinned_image,
            editor::pin_ready,
            editor::close_pin,
            drag_bridge::get_radial_context,
            drag_bridge::hide_radial,
            drag_bridge::show_files_in_radial,
            share::share_answer,
            share::share_refresh,
            share::share_send,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // WebView2 does not reliably initialise drag-and-drop for windows
            // created hidden. The transparent radial can be shown at startup
            // and hidden here without becoming visible to the user.
            if let Some(radial) = app.get_webview_window("radial") {
                let _ = radial.hide();
            }

            if !hotkeys::acquire_instance(app.handle()) {
                // A first copy already owns the hotkeys, so this copy's keyboard
                // shortcuts can never fire. Stay usable, but let the UI say so
                // instead of exiting silently: Settings offers a one-click takeover.
                hotkeys::note_registration_failed();
                eprintln!(
                    "Another Shift instance is running; the global hotkeys belong to that copy."
                );
            }

            // Restore the persisted switch before the drag monitors start.
            let persisted = settings::load(app.handle());
            if let Some(state) = app.try_state::<AppState>() {
                state
                    .drag_enabled
                    .store(persisted.drag_enabled, Ordering::Relaxed);
            }
            share::set_enabled(app.handle(), persisted.share_enabled);
            if persisted.autostart != autostart::is_enabled() {
                if let Err(error) = autostart::set_enabled(persisted.autostart) {
                    eprintln!("Could not sync launch at login: {error}");
                }
            }

            setup_tray(app.handle())?;
            register_shortcuts(app.handle())?;
            drag_bridge::spawn(app.handle());
            share::spawn(app.handle());
            update::spawn_startup_check(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if matches!(window.label(), "main" | "editor" | "radial" | "pin") {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            // An incoming-file card that disappears without an answer means the
            // user dismissed it; tell the sender now instead of after its timeout.
            if let WindowEvent::Destroyed = event {
                if window.label().starts_with("share-") {
                    window
                        .app_handle()
                        .state::<share::ShareState>()
                        .reject_notice(window.label());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Shift");

    app.run(|app, event| {
        if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
            if let Some(state) = app.try_state::<AppState>() {
                recording::stop_on_exit(app, &state);
                if let Ok(mut bridge) = state.bridge.lock() {
                    if let Some(mut child) = bridge.take() {
                        let _ = child.kill();
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn global_shortcuts_use_parsed_hotkeys() {
        let capture = Shortcut::from_str("Alt+Shift+2").unwrap();
        let record = Shortcut::from_str("Alt+Shift+3").unwrap();
        let fullscreen = Shortcut::from_str("Alt+Shift+4").unwrap();
        assert_eq!(shortcut_action(&capture), Some("capture"));
        assert_eq!(shortcut_action(&record), Some("record"));
        assert_eq!(shortcut_action(&fullscreen), Some("fullscreen"));
    }

    #[test]
    fn file_menu_hotkey_is_not_listening() {
        // The radial menu is driven by chosen files, so no hotkey reads the
        // file manager selection any more.
        assert_eq!(
            shortcut_action(&Shortcut::from_str("Alt+Shift+Space").unwrap()),
            None
        );
    }

    #[test]
    fn no_child_process_is_started_without_hiding_its_console() {
        // A Windows GUI build owns no console, so a bare `Command::new` allocates
        // a visible one: the "black window keeps blinking" report. Every spawn in
        // the app must go through a process helper that hides it.
        let sources = [
            ("autostart.rs", include_str!("autostart.rs")),
            ("capture.rs", include_str!("capture.rs")),
            ("drag_bridge.rs", include_str!("drag_bridge.rs")),
            ("editor.rs", include_str!("editor.rs")),
            ("files.rs", include_str!("files.rs")),
            ("history.rs", include_str!("history.rs")),
            ("hotkeys.rs", include_str!("hotkeys.rs")),
            ("pdf.rs", include_str!("pdf.rs")),
            ("recording.rs", include_str!("recording.rs")),
            ("result_card.rs", include_str!("result_card.rs")),
            ("runtime.rs", include_str!("runtime.rs")),
            ("settings.rs", include_str!("settings.rs")),
            ("share.rs", include_str!("share.rs")),
            ("update.rs", include_str!("update.rs")),
        ];
        for (name, source) in sources {
            // Tests may spawn freely; only shipped code needs the helper.
            let production = source
                .split("#[cfg(test)]")
                .next()
                .unwrap()
                .split("#[cfg(all(test")
                .next()
                .unwrap();
            for (index, line) in production.lines().enumerate() {
                let bare = line.contains("Command::new(")
                    && !line.contains("process::command(")
                    && !line.contains("process::hidden(");
                assert!(
                    !bare,
                    "{name}:{} starts a process without the console helper: {line}",
                    index + 1
                );
            }
        }
    }

    #[test]
    fn tray_icon_is_created_once_from_code_only() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(config["app"].get("trayIcon").is_none());
        let production = include_str!("lib.rs").split("#[cfg(test)]").next().unwrap();
        assert_eq!(production.matches("TrayIconBuilder::with_id").count(), 1);
    }

    #[test]
    fn the_single_instance_guard_runs_before_other_plugins() {
        // Plugins run in registration order. A second Windows process has to be
        // rejected before dialogs, shortcuts or windows can be initialised.
        let production = include_str!("lib.rs").split("#[cfg(test)]").next().unwrap();
        let guard = production
            .find(".plugin(tauri_plugin_single_instance::init")
            .expect("the single-instance plugin must be registered");
        let next = production
            .find(".plugin(tauri_plugin_opener::init")
            .expect("the opener plugin must be registered");
        assert!(guard < next);
        assert!(production[guard..next].contains("show_main(app)"));
    }

    #[test]
    fn a_window_shown_for_capture_or_ring_hands_the_keyboard_to_its_page() {
        // AppKit sends key events to the window's first responder, and showing a
        // window leaves that on tao's own view instead of the WKWebView, so Escape
        // never reaches the page. `set_focus` on the window is not enough; only the
        // webview's own focus does it. Regression: ESC cancelled nothing in the
        // capture overlay, and the ring stopped answering it too.
        for (name, source) in [
            ("capture.rs", include_str!("capture.rs")),
            ("drag_bridge.rs", include_str!("drag_bridge.rs")),
        ] {
            let production = source.split("#[cfg(test)]").next().unwrap();
            assert!(
                production.contains("focus_page"),
                "{name} shows a page without giving it the keyboard"
            );
        }
    }

    #[test]
    fn a_finished_capture_gives_the_open_panels_back() {
        // Captures hide whatever Shift windows are open. Without restoring them the
        // user loses an open panel to a screenshot, so both exit paths — the shot
        // ending, and the recorder taking over — are checked here.
        let production = include_str!("capture.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            production.contains("restore_windows(app)"),
            "a dismissed capture must restore the windows it hid"
        );
        let recording = include_str!("recording.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            recording.contains("dismiss_capture_for_recording")
                && recording.contains("capture::restore_windows(app)"),
            "a recording must hold the panels back until the video is written"
        );
    }

    #[test]
    fn windows_startup_uses_the_registry_api_not_reg_exe() {
        let production = include_str!("autostart.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(!production.contains("process::status(\"reg\""));
        assert!(!production.contains("Command::new(\"reg\""));
    }
}
