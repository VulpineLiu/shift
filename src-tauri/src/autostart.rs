//! Launch-at-login without an extra plugin.
//!
//! `tauri-plugin-autostart` would normally own this, but it is not vendored in
//! this workspace, so each platform uses its own standard mechanism: a
//! LaunchAgent property list on macOS, the `HKCU\...\Run` key on Windows, and
//! an autostart `.desktop` entry on Linux. Every one of them is reversible from
//! the same Settings toggle.

use std::path::PathBuf;

// Each helper is only pulled in where it is actually used, so no platform
// carries dead imports.
#[cfg(target_os = "macos")]
use std::{fs, process::Command};
#[cfg(target_os = "windows")]
use std::process::Command;
#[cfg(all(unix, not(target_os = "macos")))]
use std::fs;

/// Registers or unregisters Shift as a login item.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    platform_set(enabled)
}

/// Whether the platform mechanism currently lists Shift as a login item.
pub fn is_enabled() -> bool {
    platform_is_enabled()
}

#[cfg(target_os = "macos")]
fn platform_set(enabled: bool) -> Result<(), String> {
    if enabled {
        install_macos_agent()
    } else {
        remove_macos_agent()
    }
}

#[cfg(target_os = "macos")]
fn platform_is_enabled() -> bool {
    agent_plist().is_some_and(|path| path.exists())
}

#[cfg(target_os = "windows")]
fn platform_set(enabled: bool) -> Result<(), String> {
    set_windows_run_key(enabled)
}

#[cfg(target_os = "windows")]
fn platform_is_enabled() -> bool {
    query_windows_run_key()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_set(enabled: bool) -> Result<(), String> {
    if enabled {
        install_linux_autostart()
    } else {
        remove_linux_autostart()
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_is_enabled() -> bool {
    linux_autostart_file().is_some_and(|path| path.exists())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
fn platform_set(_enabled: bool) -> Result<(), String> {
    Err("Launch at login is not supported on this platform.".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
fn platform_is_enabled() -> bool {
    false
}

/// The app to launch. Inside a `.app` bundle this is the bundle itself, so the
/// Dock name, icon and macOS TCC identity match what the user approved.
fn executable_path() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    let mut cursor = current.as_path();
    while let Some(parent) = cursor.parent() {
        if parent.extension().and_then(|value| value.to_str()) == Some("app") {
            return Some(parent.to_path_buf());
        }
        cursor = parent;
    }
    Some(current)
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
const LABEL: &str = "app.nex.shift";

#[cfg(target_os = "macos")]
fn agent_plist() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist")),
    )
}

#[cfg(target_os = "macos")]
fn install_macos_agent() -> Result<(), String> {
    let path = agent_plist().ok_or_else(|| "Could not locate your home folder.".to_string())?;
    let target = executable_path().ok_or_else(|| "Could not locate Shift.".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{target}</string>
  </array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
        target = target
            .to_string_lossy()
            .replace('&', "&amp;")
            .replace('<', "&lt;"),
    );
    fs::write(&path, plist).map_err(|error| format!("Could not write the login item: {error}"))?;

    let domain = format!("gui/{}", unsafe { libc::getuid() });
    // Modern launchctl first, legacy load as a fallback. Either one succeeding
    // is enough, and the plist also takes effect at the next login regardless.
    let modern = Command::new("launchctl")
        .args(["bootstrap", domain.as_str(), &path.to_string_lossy()])
        .status();
    if !matches!(modern, Ok(status) if status.success()) {
        let _ = Command::new("launchctl")
            .args(["load", &path.to_string_lossy()])
            .status();
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn remove_macos_agent() -> Result<(), String> {
    let Some(path) = agent_plist() else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let _ = Command::new("launchctl")
        .args(["bootout", domain.as_str(), &path.to_string_lossy()])
        .status();
    let _ = Command::new("launchctl")
        .args(["unload", &path.to_string_lossy()])
        .status();
    fs::remove_file(&path).map_err(|error| error.to_string())
}

// -------------------------------------------------------------- Windows

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(target_os = "windows")]
fn registry_command(arguments: &[&str]) -> Result<std::process::ExitStatus, String> {
    Command::new("reg")
        .args(arguments)
        .status()
        .map_err(|error| format!("Could not run reg.exe: {error}"))
}

#[cfg(target_os = "windows")]
fn query_windows_run_key() -> bool {
    matches!(
        registry_command(&["query", RUN_KEY, "/v", "Shift"]),
        Ok(status) if status.success()
    )
}

#[cfg(target_os = "windows")]
fn set_windows_run_key(enabled: bool) -> Result<(), String> {
    if enabled {
        let target = executable_path().ok_or_else(|| "Could not locate Shift.".to_string())?;
        // Quote the path: it usually contains "Program Files" or a user name
        // with spaces, and the Run key performs no quoting of its own.
        let value = format!("\"{}\"", target.display());
        registry_command(&["add", RUN_KEY, "/v", "Shift", "/t", "REG_SZ", "/d", &value, "/f"])?;
    } else {
        registry_command(&["delete", RUN_KEY, "/v", "Shift", "/f"])?;
    }
    Ok(())
}

// ---------------------------------------------------------------- Linux

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_autostart_file() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("autostart").join("shift.desktop"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn install_linux_autostart() -> Result<(), String> {
    let path = linux_autostart_file().ok_or_else(|| "Could not locate ~/.config.".to_string())?;
    let target = executable_path().ok_or_else(|| "Could not locate Shift.".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let exec = target.to_string_lossy();
    fs::write(
        &path,
        format!(
            "[Desktop Entry]\nType=Application\nName=Shift\nExec=\"{exec}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        ),
    )
    .map_err(|error| error.to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn remove_linux_autostart() -> Result<(), String> {
    let Some(path) = linux_autostart_file() else {
        return Ok(());
    };
    if path.exists() {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    Ok(())
}
