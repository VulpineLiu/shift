use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, MutexGuard, OnceLock},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// The switches only need their own value back. Rebuilding the full settings
/// snapshot probes the login item and creates three folders, which can block on
/// a network or cloud-synced volume and makes the switch feel slow.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchState {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchUpdate {
    pub state: SwitchState,
    /// The platform is the source of truth for launch-at-login.
    pub autostart: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileOutputMode {
    #[default]
    Alongside,
    Custom,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub file_output_mode: FileOutputMode,
    pub file_output_dir: Option<String>,
    pub screenshot_dir: Option<String>,
    pub recording_dir: Option<String>,
    pub auto_reveal: bool,
    /// Master switch for the drag-to-open radial rings.
    pub drag_enabled: bool,
    /// Whether this copy announces itself to other Shift copies on the LAN.
    pub share_enabled: bool,
    /// Requested launch-at-login state; the platform is the source of truth.
    pub autostart: bool,
    /// UI language, "" until the user picks one. Unset follows the system.
    pub language: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            file_output_mode: FileOutputMode::Alongside,
            file_output_dir: None,
            screenshot_dir: None,
            recording_dir: None,
            auto_reveal: true,
            drag_enabled: true,
            share_enabled: true,
            autostart: false,
            language: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub file_output_mode: FileOutputMode,
    pub file_output_dir: String,
    pub screenshot_dir: String,
    pub recording_dir: String,
    pub auto_reveal: bool,
    pub drag_enabled: bool,
    pub share_enabled: bool,
    pub autostart: bool,
    pub language: String,
    pub default_file_output_dir: String,
    pub default_screenshot_dir: String,
    pub default_recording_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteCheck {
    pub kind: String,
    pub path: String,
    pub ok: bool,
    pub message: String,
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?
        .join("settings.json"))
}

/// Every settings write is a read-modify-write of one JSON file. Without a
/// process-wide guard, two quick toggle clicks can both read the old snapshot
/// and the last writer silently restores the other switch.
fn settings_write_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn load(app: &AppHandle) -> AppSettings {
    settings_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let contents = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temporary, contents).map_err(|error| error.to_string())?;

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };

        let source: Vec<u16> = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let destination: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let moved = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved == 0 {
            let error = std::io::Error::last_os_error();
            let _ = fs::remove_file(&temporary);
            return Err(format!("Could not replace settings.json: {error}"));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        fs::rename(&temporary, &path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Write one setting without losing a concurrent switch. The guard covers the
/// whole load-mutate-save cycle, while each command still runs on a blocking
/// worker so the webview remains responsive.
fn update_settings<T>(
    app: &AppHandle,
    work: impl FnOnce(&mut AppSettings) -> Result<T, String>,
) -> Result<T, String> {
    let _guard = settings_write_lock();
    let mut settings = load(app);
    let value = work(&mut settings)?;
    save(app, &settings)?;
    Ok(value)
}

fn default_directory(app: &AppHandle, kind: &str) -> Result<PathBuf, String> {
    let base = match kind {
        "screenshot" => app.path().picture_dir().or_else(|_| app.path().home_dir()),
        "recording" => app.path().video_dir().or_else(|_| app.path().home_dir()),
        _ => app.path().download_dir().or_else(|_| app.path().home_dir()),
    }
    .map_err(|error| error.to_string())?;
    Ok(base.join("Shift"))
}

fn configured_directory(
    app: &AppHandle,
    kind: &str,
    configured: Option<&str>,
) -> Result<PathBuf, String> {
    let directory = configured
        .map(PathBuf::from)
        .unwrap_or(default_directory(app, kind)?);
    ensure_directory(directory)
}

fn ensure_directory(directory: PathBuf) -> Result<PathBuf, String> {
    fs::create_dir_all(&directory).map_err(|error| {
        format!(
            "Could not create {}. Choose another output folder in Settings. {error}",
            directory.display()
        )
    })?;
    Ok(directory)
}

pub fn screenshot_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let settings = load(app);
    configured_directory(app, "screenshot", settings.screenshot_dir.as_deref())
}

pub fn recording_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let settings = load(app);
    configured_directory(app, "recording", settings.recording_dir.as_deref())
}

pub fn files_output_dir(app: &AppHandle) -> Result<Option<PathBuf>, String> {
    let settings = load(app);
    if settings.file_output_mode != FileOutputMode::Custom {
        return Ok(None);
    }
    configured_directory(app, "files", settings.file_output_dir.as_deref()).map(Some)
}

pub fn auto_reveal(app: &AppHandle) -> bool {
    load(app).auto_reveal
}

/// Landing folder for files shared from another machine over the LAN. Shared files
/// join the normal Shift output folder so they also show up under "Recent".
pub fn received_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let settings = load(app);
    match settings.file_output_mode {
        FileOutputMode::Custom => {
            configured_directory(app, "files", settings.file_output_dir.as_deref())
        }
        FileOutputMode::Alongside => configured_directory(app, "files", None),
    }
}

pub fn snapshot(app: &AppHandle) -> Result<SettingsSnapshot, String> {
    let settings = load(app);
    Ok(SettingsSnapshot {
        file_output_mode: settings.file_output_mode,
        file_output_dir: match settings.file_output_mode {
            FileOutputMode::Alongside => default_directory(app, "files")?
                .to_string_lossy()
                .to_string(),
            FileOutputMode::Custom => {
                configured_directory(app, "files", settings.file_output_dir.as_deref())?
                    .to_string_lossy()
                    .to_string()
            }
        },
        screenshot_dir: screenshot_dir(app)?.to_string_lossy().to_string(),
        recording_dir: recording_dir(app)?.to_string_lossy().to_string(),
        auto_reveal: settings.auto_reveal,
        drag_enabled: settings.drag_enabled,
        share_enabled: settings.share_enabled,
        // Report what the OS actually has registered, not what we last wrote.
        autostart: crate::autostart::is_enabled(),
        language: settings.language.clone(),
        default_file_output_dir: default_directory(app, "files")?
            .to_string_lossy()
            .to_string(),
        default_screenshot_dir: default_directory(app, "screenshot")?
            .to_string_lossy()
            .to_string(),
        default_recording_dir: default_directory(app, "recording")?
            .to_string_lossy()
            .to_string(),
    })
}

fn normalize_directory(path: Option<String>) -> Result<Option<String>, String> {
    match path {
        Some(value) if !value.trim().is_empty() => {
            let directory = PathBuf::from(value);
            if directory.exists() && !directory.is_dir() {
                return Err("The selected output path is not a folder.".into());
            }
            fs::create_dir_all(&directory).map_err(|error| {
                format!(
                    "Could not use {}. Choose a writable folder. {error}",
                    directory.display()
                )
            })?;
            Ok(Some(directory.to_string_lossy().to_string()))
        }
        _ => Ok(None),
    }
}

pub fn test_output_access(app: &AppHandle) -> Result<Vec<WriteCheck>, String> {
    let checks = [
        ("screenshot", screenshot_dir(app)?),
        ("recording", recording_dir(app)?),
        (
            "files",
            match files_output_dir(app)? {
                Some(directory) => directory,
                None => configured_directory(app, "files", None)?,
            },
        ),
    ];
    Ok(checks
        .into_iter()
        .map(|(kind, directory)| {
            let probe = directory.join(format!(".shift-write-test-{}", uuid::Uuid::new_v4()));
            let result = fs::write(&probe, b"Shift");
            let _ = fs::remove_file(&probe);
            WriteCheck {
                kind: kind.into(),
                path: directory.to_string_lossy().to_string(),
                ok: result.is_ok(),
                message: match result {
                    Ok(()) => "Write access confirmed.".into(),
                    Err(error) => format!("{error}. Allow access in System Settings > Privacy & Security > Files and Folders."),
                },
            }
        })
        .collect())
}

// These commands run off the main thread: every one of them touches the disk
// (settings.json, output-directory probes) and one slow folder — a cloud-synced
// or network volume — would otherwise freeze the whole app behind a toggle.
#[tauri::command]
pub async fn get_app_settings(app: AppHandle) -> Result<SettingsSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || snapshot(&app))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_output_directory(
    app: AppHandle,
    kind: String,
    path: Option<String>,
) -> Result<SettingsSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let normalized = normalize_directory(path)?;
        update_settings(&app, |settings| {
            match kind.as_str() {
                "screenshot" => settings.screenshot_dir = normalized,
                "recording" => settings.recording_dir = normalized,
                "files" => {
                    settings.file_output_dir = normalized;
                    settings.file_output_mode = if settings.file_output_dir.is_some() {
                        FileOutputMode::Custom
                    } else {
                        FileOutputMode::Alongside
                    };
                }
                other => return Err(format!("Unknown output type: {other}")),
            }
            Ok(())
        })?;
        snapshot(&app)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_file_output_mode(
    app: AppHandle,
    mode: FileOutputMode,
) -> Result<SettingsSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        update_settings(&app, |settings| {
            settings.file_output_mode = mode;
            Ok(())
        })?;
        snapshot(&app)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_auto_reveal(app: AppHandle, enabled: bool) -> Result<SwitchState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        update_settings(&app, |settings| {
            settings.auto_reveal = enabled;
            Ok(())
        })?;
        Ok(SwitchState { enabled })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_drag_enabled(app: AppHandle, enabled: bool) -> Result<SwitchState, String> {
    if let Some(state) = app.try_state::<crate::AppState>() {
        state
            .drag_enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }
    tauri::async_runtime::spawn_blocking(move || {
        update_settings(&app, |settings| {
            settings.drag_enabled = enabled;
            Ok(())
        })?;
        if !enabled {
            crate::drag_bridge::hide(&app);
        }
        Ok(SwitchState { enabled })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_share_enabled(app: AppHandle, enabled: bool) -> Result<SwitchState, String> {
    crate::share::set_enabled(&app, enabled);
    tauri::async_runtime::spawn_blocking(move || {
        update_settings(&app, |settings| {
            settings.share_enabled = enabled;
            Ok(())
        })?;
        Ok(SwitchState { enabled })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_autostart(app: AppHandle, enabled: bool) -> Result<SwitchUpdate, String> {
    tauri::async_runtime::spawn_blocking(move || {
        update_settings(&app, |settings| {
            crate::autostart::set_enabled(enabled)?;
            settings.autostart = enabled;
            Ok(())
        })?;
        Ok(SwitchUpdate {
            state: SwitchState { enabled },
            autostart: Some(crate::autostart::is_enabled()),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Remember the UI language and re-label the tray, which Rust owns. Return only
/// the stored value: a full settings snapshot would probe the login item and
/// create output folders, adding latency to a language switch.
#[tauri::command]
pub async fn set_language(app: AppHandle, language: String) -> Result<String, String> {
    let normalized = crate::locale::from_setting(Some(&language))
        .ok_or_else(|| "Unknown language.".to_string())?;
    // The tray is visible immediately; persistence must never gate its update.
    crate::set_tray_language(normalized);
    crate::refresh_tray_language(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        update_settings(&app, |settings| {
            settings.language = normalized.to_string();
            Ok(())
        })?;
        Ok(normalized.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn check_output_access(app: AppHandle) -> Result<Vec<WriteCheck>, String> {
    tauri::async_runtime::spawn_blocking(move || test_output_access(&app))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn reveal_output_directory(app: AppHandle, kind: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = match kind.as_str() {
            "screenshot" => screenshot_dir(&app)?,
            "recording" => recording_dir(&app)?,
            "files" => match files_output_dir(&app)? {
                Some(directory) => directory,
                None => configured_directory(&app, "files", None)?,
            },
            other => return Err(format!("Unknown output type: {other}")),
        };
        crate::files::reveal_path(&directory)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn open_files_permission_settings() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        #[cfg(target_os = "macos")]
        {
            let status = crate::process::command("/usr/bin/open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders")
                .status()
                .map_err(|error| error.to_string())?;
            if status.success() {
                return Ok(());
            }
            Err("Could not open Files and Folders settings.".into())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err("Files and Folders settings are macOS-specific.".into())
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_alongside_and_auto_reveal() {
        let settings = AppSettings::default();
        assert_eq!(settings.file_output_mode, FileOutputMode::Alongside);
        assert!(settings.auto_reveal);
        assert!(settings.share_enabled);
    }

    #[test]
    fn output_mode_serializes_for_frontend() {
        assert_eq!(
            serde_json::to_string(&FileOutputMode::Custom).unwrap(),
            "\"custom\""
        );
    }

    #[test]
    fn mode_round_trips_through_json() {
        let settings = AppSettings {
            file_output_mode: FileOutputMode::Custom,
            file_output_dir: Some("/tmp/Shift".into()),
            ..AppSettings::default()
        };
        let text = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&text).unwrap();
        assert_eq!(restored.file_output_mode, FileOutputMode::Custom);
        assert_eq!(restored.file_output_dir.as_deref(), Some("/tmp/Shift"));
    }

    #[test]
    fn configured_directory_creates_the_target() {
        let directory =
            std::env::temp_dir().join(format!("shift-settings-{}", uuid::Uuid::new_v4()));
        let resolved = ensure_directory(directory.clone()).unwrap();
        assert_eq!(resolved, directory);
        assert!(directory.is_dir());
        fs::remove_dir_all(directory).unwrap();
    }
}
