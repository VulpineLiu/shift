mod capture;
mod autostart;
mod drag_bridge;
mod editor;
mod files;
mod hotkeys;
mod models;
mod office;
mod pdf;
mod recording;
mod settings;

use std::{
    process::Child,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
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

fn report_error(app: &AppHandle, message: impl Into<String>) {
    let message = message.into();
    eprintln!("{message}");
    let _ = app.emit("app://error", message);
}

fn start_capture_from_event(app: &AppHandle, mode: &str) {
    if let Err(error) = capture::begin_capture(app, mode) {
        report_error(app, error);
        show_main(app);
    }
}

fn start_fullscreen_capture(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Err(error) = capture::capture_fullscreen_core(app, &state) {
        report_error(app, error);
        show_main(app);
    }
}

fn toggle_recording(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        start_capture_from_event(app, "recording");
        return;
    };
    let active = state
        .recording
        .lock()
        .map(|recording| recording.is_some())
        .unwrap_or(false);
    if active {
        if let Err(error) = recording::stop(app, &state) {
            report_error(app, error);
        }
    } else {
        start_capture_from_event(app, "recording");
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Shift", true, None::<&str>)?;
    let capture = MenuItem::with_id(app, "capture", "Capture area", true, Some("Alt+Shift+2"))?;
    let fullscreen = MenuItem::with_id(
        app,
        "fullscreen",
        "Capture full screen",
        true,
        Some("Alt+Shift+4"),
    )?;
    let record = MenuItem::with_id(app, "record", "Record area", true, Some("Alt+Shift+3"))?;
    let ocr = MenuItem::with_id(app, "ocr", "OCR an image…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Shift", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &capture,
            &fullscreen,
            &record,
            &ocr,
            &separator,
            &quit,
        ],
    )?;

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
fn reclaim_hotkeys(app: AppHandle) -> hotkeys::HotkeyStatus {
    // Take the instance lock as well, so this copy is the owner once the others die.
    let _ = hotkeys::acquire_instance(&app);
    for shortcut in hotkeys::HOTKEYS {
        let _ = app.global_shortcut().unregister(shortcut);
    }
    let _ = register_shortcuts(&app);
    hotkeys::status()
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
    for shortcut in [
        "Alt+Shift+2",
        "Alt+Shift+3",
        "Alt+Shift+4",
    ] {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(shortcut_plugin)
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            files::inspect_files,
            files::perform_file_action,
            files::reveal_file,
            files::existing_files,
            files::open_file,
            files::read_file_data_url,
            files::read_text_file,
            files::save_image_data,
            files::render_pdf_pages,
            files::save_text_near,
            settings::get_app_settings,
            settings::set_output_directory,
            settings::set_file_output_mode,
            settings::set_auto_reveal,
            settings::set_drag_enabled,
            settings::set_autostart,
            settings::check_output_access,
            settings::reveal_output_directory,
            settings::open_files_permission_settings,
            capture::get_capture_session,
            capture::screen_capture_access_status,
            capture::start_capture,
            capture::cancel_capture,
            capture::capture_crop,
            capture::capture_fullscreen,
            capture::copy_file_to,
            capture::open_screen_recording_settings,
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
            drag_bridge::get_radial_context,
            drag_bridge::hide_radial,
            drag_bridge::show_files_in_radial,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            if !hotkeys::acquire_instance(app.handle()) {
                // A first copy already owns the hotkeys, so this copy's keyboard
                // shortcuts can never fire. Stay usable, but let the UI say so
                // instead of exiting silently: Settings offers a one-click takeover.
                hotkeys::note_registration_failed();
                eprintln!("Another Shift instance is running; the global hotkeys belong to that copy.");
            }

            // Restore the persisted switch before the drag monitors start.
            let persisted = settings::load(app.handle());
            if let Some(state) = app.try_state::<AppState>() {
                state
                    .drag_enabled
                    .store(persisted.drag_enabled, Ordering::Relaxed);
            }
            if persisted.autostart != autostart::is_enabled() {
                if let Err(error) = autostart::set_enabled(persisted.autostart) {
                    eprintln!("Could not sync launch at login: {error}");
                }
            }

            setup_tray(app.handle())?;
            register_shortcuts(app.handle())?;
            drag_bridge::spawn(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if matches!(window.label(), "main" | "editor" | "radial") {
                    api.prevent_close();
                    let _ = window.hide();
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
        assert_eq!(shortcut_action(&Shortcut::from_str("Alt+Shift+Space").unwrap()), None);
    }

    #[test]
    fn tray_icon_is_created_once_from_code_only() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(config["app"].get("trayIcon").is_none());
        let production = include_str!("lib.rs").split("#[cfg(test)]").next().unwrap();
        assert_eq!(production.matches("TrayIconBuilder::with_id").count(), 1);
    }
}
