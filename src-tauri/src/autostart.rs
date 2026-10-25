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
#[cfg(any(target_os = "macos", all(unix, not(target_os = "macos"))))]
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
    let Some(path) = agent_plist() else {
        return false;
    };
    if !path.exists() {
        return false;
    }
    // A plist that still points at an old location is a leftover from a moved
    // app, so the toggle has to be used again to register the new path.
    let Some(current) = executable_path() else {
        return false;
    };
    fs::read_to_string(&path)
        .map(|plist| plist.contains(&current.to_string_lossy().to_string()))
        .unwrap_or(false)
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

/// The app to launch. On macOS this is the executable inside the `.app`
/// bundle: a LaunchAgent cannot run a bundle, it can only run a program.
/// Pointing it at the bundle makes launchd fail with `EX_CONFIG`, so the
/// executable is used instead. It keeps the same bundle identity, icon and
/// macOS TCC permission the user approved.
fn executable_path() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
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
    let modern = crate::process::command("launchctl")
        .args(["bootstrap", domain.as_str(), &path.to_string_lossy()])
        .status();
    if !matches!(modern, Ok(status) if status.success()) {
        let _ = crate::process::command("launchctl")
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
    let _ = crate::process::command("launchctl")
        .args(["bootout", domain.as_str(), &path.to_string_lossy()])
        .status();
    let _ = crate::process::command("launchctl")
        .args(["unload", &path.to_string_lossy()])
        .status();
    fs::remove_file(&path).map_err(|error| error.to_string())
}

// -------------------------------------------------------------- Windows

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(target_os = "windows")]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Access the Run key in process. Spawning `reg.exe` for every settings
/// snapshot made a GUI app flash a console window and added tens of
/// milliseconds to each settings action.
#[cfg(target_os = "windows")]
fn query_windows_run_key() -> bool {
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
            REG_SZ,
        },
    };

    let subkey = wide(RUN_KEY);
    let name = wide("Shift");
    let mut key: HKEY = std::ptr::null_mut();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if opened == ERROR_FILE_NOT_FOUND {
        return false;
    }
    if opened != ERROR_SUCCESS || key.is_null() {
        return false;
    }
    let mut kind = 0_u32;
    let mut size = 0_u32;
    let query = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    let exists = query == ERROR_SUCCESS && kind == REG_SZ && size > 0;
    unsafe { RegCloseKey(key) };
    exists
}

#[cfg(target_os = "windows")]
fn set_windows_run_key(enabled: bool) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
            KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
        },
    };

    let subkey = wide(RUN_KEY);
    let name = wide("Shift");
    let mut key: HKEY = std::ptr::null_mut();
    let opened = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if opened != ERROR_SUCCESS || key.is_null() {
        return Err(format!(
            "Could not open the Windows startup registry key ({opened})."
        ));
    }

    let result = if enabled {
        let target = executable_path().ok_or_else(|| "Could not locate Shift.".to_string())?;
        // The Run key executes the string as written; quote paths containing spaces.
        let value = wide(&format!("\"{}\"", target.display()));
        let bytes = unsafe {
            std::slice::from_raw_parts(
                value.as_ptr().cast::<u8>(),
                value.len() * std::mem::size_of::<u16>(),
            )
        };
        unsafe {
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        }
    } else {
        unsafe { RegDeleteValueW(key, name.as_ptr()) }
    };
    unsafe { RegCloseKey(key) };
    if result != ERROR_SUCCESS {
        return Err(format!(
            "Could not {} the Windows startup entry ({result}).",
            if enabled { "create" } else { "remove" }
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_agent_points_at_an_executable_not_a_bundle() {
        let path = agent_plist().unwrap();
        // A LaunchAgent cannot run an `.app` bundle, so the plist must name a
        // program inside it.
        let target = executable_path().unwrap();
        assert!(
            target.extension().and_then(|value| value.to_str()) != Some("app"),
            "the agent must not point at a bundle: {target:?}"
        );
        assert!(path.ends_with(format!("{LABEL}.plist")));
    }
}
