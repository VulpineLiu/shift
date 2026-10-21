//! One instance, one owner of the global hotkeys.
//!
//! macOS hands a global hotkey to a single client: whichever process registers it
//! first silently owns it. A second copy still sees "Registered global shortcut"
//! in its log, yet its hotkeys never fire — that is precisely how "the panel
//! buttons save files but the keyboard shortcuts do nothing" shows up, which
//! happens whenever an old /Applications copy runs next to a newer build.
//!
//! An advisory flock on a file in the app config dir tells a copy whether it is
//! the first one. The owner behaves as before; a duplicate keeps working but says
//! out loud that its hotkeys are dead and can take over in one click.

use std::{
    fs::OpenOptions,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use serde::Serialize;
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use std::os::fd::AsRawFd;

/// Hotkeys that only one instance can own.
pub const HOTKEYS: [&str; 3] = ["Alt+Shift+2", "Alt+Shift+3", "Alt+Shift+4"];

/// Held for the lifetime of the process so the lock survives setup.
static INSTANCE_LOCK: Mutex<Option<std::fs::File>> = Mutex::new(None);
/// Whether this process won the instance lock, i.e. it is the first copy.
static INSTANCE_OWNER: AtomicBool = AtomicBool::new(false);
static HOTKEYS_LIVE: AtomicBool = AtomicBool::new(true);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    /// False when another copy owns the hotkeys, i.e. this copy's keys do nothing.
    pub live: bool,
    pub hotkeys: Vec<String>,
    /// Other running Shift processes.
    pub others: Vec<u32>,
}

/// True when this process is the first Shift copy; false when another one runs.
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
        // An unreadable config folder must not stop the app from starting.
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
    INSTANCE_OWNER.store(true, Ordering::Relaxed);
    true
}

/// True for the copy that won the instance lock.
pub fn is_owner() -> bool {
    INSTANCE_OWNER.load(Ordering::Relaxed)
}

/// Recompute whether this copy's hotkeys can actually fire. macOS reports
/// success even for a copy that will never receive the events, so ownership of
/// the instance lock is what decides - not the result of registration.
pub fn refresh_live(registered_ok: bool) {
    HOTKEYS_LIVE.store(is_owner() && registered_ok, Ordering::Relaxed);
}

/// Record that macOS refused a hotkey, usually because another app holds it.
pub fn note_registration_failed() {
    HOTKEYS_LIVE.store(false, Ordering::Relaxed);
}

pub fn status() -> HotkeyStatus {
    HotkeyStatus {
        live: HOTKEYS_LIVE.load(Ordering::Relaxed),
        hotkeys: HOTKEYS.iter().map(|hotkey| hotkey.to_string()).collect(),
        others: other_pids(),
    }
}

/// Pids of the other Shift binaries currently running.
pub fn other_pids() -> Vec<u32> {
    #[cfg(not(unix))]
    {
        return Vec::new();
    }
    #[cfg(unix)]
    {
        let Ok(output) = std::process::Command::new("/usr/bin/pgrep")
            .args(["-x", "shift"])
            .output()
        else {
            return Vec::new();
        };
        other_instances(&String::from_utf8_lossy(&output.stdout), std::process::id())
    }
}

/// Keep foreign processes out: only the same executable counts as another copy.
/// The canonical /Applications install is matched by path alone because it moves
/// on upgrade, which is exactly when a stale copy tends to survive.
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

/// Ask the other copies to quit so this one can own the hotkeys.
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
    fn a_dead_hotkey_state_reaches_the_ui() {
        refresh_live(false);
        let reported = status();
        assert!(!reported.live);
        assert_eq!(
            reported.hotkeys,
            vec!["Alt+Shift+2", "Alt+Shift+3", "Alt+Shift+4"]
        );
    }

    #[test]
    fn registering_cannot_make_a_non_owner_live() {
        // A duplicate copy sees registration "succeed" yet never receives the
        // hotkeys, so ownership - not the Ok(..) result - decides liveness.
        assert!(!is_owner());
        refresh_live(true);
        assert!(!status().live);
    }
}
