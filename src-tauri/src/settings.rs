use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

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
    /// Requested launch-at-login state; the platform is the source of truth.
    pub autostart: bool,
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
            autostart: false,
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
    pub autostart: bool,
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
    fs::write(
        path,
        serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
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
        // Report what the OS actually has registered, not what we last wrote.
        autostart: crate::autostart::is_enabled(),
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

fn update_directory(app: &AppHandle, kind: &str, path: Option<String>) -> Result<(), String> {
    let mut settings = load(app);
    let normalized = match path {
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
            Some(directory.to_string_lossy().to_string())
        }
        _ => None,
    };
    match kind {
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
    save(app, &settings)
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

#[tauri::command]
pub fn get_app_settings(app: AppHandle) -> Result<SettingsSnapshot, String> {
    snapshot(&app)
}

#[tauri::command]
pub fn set_output_directory(
    app: AppHandle,
    kind: String,
    path: Option<String>,
) -> Result<SettingsSnapshot, String> {
    update_directory(&app, &kind, path)?;
    snapshot(&app)
}

#[tauri::command]
pub fn set_file_output_mode(
    app: AppHandle,
    mode: FileOutputMode,
) -> Result<SettingsSnapshot, String> {
    let mut settings = load(&app);
    settings.file_output_mode = mode;
    save(&app, &settings)?;
    snapshot(&app)
}

#[tauri::command]
pub fn set_auto_reveal(app: AppHandle, enabled: bool) -> Result<SettingsSnapshot, String> {
    let mut settings = load(&app);
    settings.auto_reveal = enabled;
    save(&app, &settings)?;
    snapshot(&app)
}

#[tauri::command]
pub fn set_drag_enabled(app: AppHandle, enabled: bool) -> Result<SettingsSnapshot, String> {
    let mut settings = load(&app);
    settings.drag_enabled = enabled;
    save(&app, &settings)?;
    if let Some(state) = app.try_state::<crate::AppState>() {
        state.drag_enabled.store(enabled, std::sync::atomic::Ordering::Relaxed);
    }
    if !enabled {
        crate::drag_bridge::hide(&app);
    }
    snapshot(&app)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<SettingsSnapshot, String> {
    crate::autostart::set_enabled(enabled)?;
    let mut settings = load(&app);
    settings.autostart = enabled;
    save(&app, &settings)?;
    snapshot(&app)
}

#[tauri::command]
pub fn check_output_access(app: AppHandle) -> Result<Vec<WriteCheck>, String> {
    test_output_access(&app)
}

#[tauri::command]
pub fn reveal_output_directory(app: AppHandle, kind: String) -> Result<(), String> {
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
}

#[tauri::command]
pub fn open_files_permission_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_alongside_and_auto_reveal() {
        let settings = AppSettings::default();
        assert_eq!(settings.file_output_mode, FileOutputMode::Alongside);
        assert!(settings.auto_reveal);
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
