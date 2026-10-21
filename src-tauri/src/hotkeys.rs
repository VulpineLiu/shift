//! One instance, one owner of the global hotkeys.
//!
//! macOS delivers a registered global hotkey to only one process: whichever copy
//! registers first silently owns it, and every later copy keeps logging
//! "Registered global shortcut" while its hotkeys never fire. That is exactly how
//! "the panel buttons work but the keyboard shortcuts do not" shows up, so the app
//! refuses to run twice and can report who holds the hotkeys.

use std::{fs::OpenOptions, sync::Mutex};

use serde::Serialize;
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use std::os::fd::AsRawFd;

/// Hotkeys that must be owned by exactly one instance.
pub const HOTKEYS: [&str; 3] = ["Alt+Shift+2", "Alt+Shift+3", "Alt+Shift+4"];

/// Keeps the advisory lock alive for the lifetime of the process.
static INSTANCE_LOCK: Mutex<Option<std::fs::File>> = Mutex::new(None);
static HOTKEYS_LIVE: Mutex<bool> = Mutex::new(true);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    /// False when registration failed, which means another copy owns the hotkeys.
    pub live: bool,
    pub hotkeys: Vec<String>,
    /// Other running Shift processes.
    pub others: Vec<u32>,
}

/// True when this process may continue; false when another Shift already holds the lock.
pub fn acquire_instance(app: &AppHandle) -> bool {
    let path = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("shift.lock");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(file) = OpenOptions::new().create(true).write(true).open(&path) else {
        // An unreadable config folder must not prevent the app from starting.
        return true;
    };
    #[cfg(unix)]
    let acquired = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    #[cfg(not(unix))]
    let acquired = true;
    if !acquired {
        return false;
    }
    if let Ok(mut guard) = INSTANCE_LOCK.lock() {
        *guard = Some(file);
    }
    true
}

/// Record that a capture hotkey could not be registered.
pub fn note_registration_failed() {
    set_live(false);
}

/// Track whether this instance owns the capture hotkeys.
pub fn set_live(live: bool) {
    if let Ok(mut guard) = HOTKEYS_LIVE.lock() {
        *guard = live;
    }
}

pub fn status() -> HotkeyStatus {
    HotkeyStatus {
        live: *HOTKEYS_LIVE.lock().unwrap_or_else(|error| error.into_inner()),
        hotkeys: HOTKEYS.iter().map(|hotkey| hotkey.to_string()).collect(),
        others: other_pids(),
    }
}

/// Pids of other Shift processes, excluding this one.
pub fn other_pids() -> Vec<u32> {
    let output = std::process::Command::new("/usr/bin/pgrep")
        .args(["-x", "shift"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    other_instances(&String::from_utf8_lossy(&output.stdout), std::process::id())
}

/// Keeps foreign processes out: only the same executable counts as another copy,
/// except for the canonical /Applications install, whose path changes on upgrade.
pub fn other_instances(listing: &str, current: u32) -> Vec<u32> {
    let ours = std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().to_string());
    listing
        .split_whitespace()
        .filter_map(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid != current)
        .filter(|pid| match ours.as_deref() {
            Some(exe) => command_path(*pid).as_deref() == Some(exe) || is_installed(exe),
            None => false,
        })
        .collect()
}

/// The canonical install path moves on upgrade, so matching it by name keeps a
/// stale copy quitable.
fn is_installed(exe: &str) -> bool {
    exe == "/Applications/Shift.app/Contents/MacOS/shift"
}

#[cfg(unix)]
fn command_path(pid: u32) -> Option<String> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

#[cfg(not(unix))]
fn command_path(_pid: u32) -> Option<String> {
    None
}

/// Ask other copies to quit so this one can own the hotkeys. Returns how many accepted.
#[tauri::command]
pub fn quit_other_instances() -> usize {
    let mut stopped = 0;
    for pid in other_pids() {
        #[cfg(unix)]
        if unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) } == 0 {
            stopped += 1;
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
        }
    }
    stopped
}

#[tauri::command]
pub fn hotkey_status() -> HotkeyStatus {
    status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_process_is_never_another_instance() {
        assert!(other_instances("4242", 4242).is_empty());
    }

    #[test]
    fn a_foreign_binary_named_shift_is_left_alone() {
        // /bin/ps reports the real path, so a different build never matches.
        assert!(other_instances("1", 999_999).is_empty());
    }

    #[test]
    fn empty_process_lists_produce_no_targets() {
        assert!(other_instances("", 1).is_empty());
        assert!(other_instances("not-a-pid", 1).is_empty());
    }

    #[test]
    fn registration_failures_are_reachable_from_the_ui() {
        note_registration_failed();
        assert!(!hotkey_status().live);
        assert_eq!(hotkey_status().hotkeys, vec!["Alt+Shift+2", "Alt+Shift+3", "Alt+Shift+4"]);
    }
}
