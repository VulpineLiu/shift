//! The library of captures and processed files, kept in one place on disk.
//!
//! This used to be a `localStorage` list of eight paths, which meant every window
//! had to remember to report its own results and a cleared webview cache threw the
//! history away. The list now lives in the app data directory, is capped at a few
//! hundred entries, drops files that no longer exist, and is written by Rust at the
//! moment a result is produced — so a capture recorded by the tray, a conversion
//! from the ring and a file received over the LAN all land in the same library
//! without any window having to opt in.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// Entries kept before the oldest unpinned ones fall off. Large enough to be a
/// working history, small enough that the JSON file stays trivial to read.
const CAPACITY: usize = 300;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub path: String,
    /// "image" | "video" | "text" | "file", matching the editor's own kinds.
    /// Defaults so a row imported from an older store, which may not have recorded
    /// it, still parses; every writer re-derives it from the path anyway.
    #[serde(default)]
    pub kind: String,
    /// Milliseconds since the epoch, as the webview already reports them.
    #[serde(default)]
    pub at: i64,
    /// Pinned entries survive both the capacity limit and a missing file.
    #[serde(default)]
    pub pinned: bool,
}

impl HistoryEntry {
    fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            kind: crate::files::editor_kind(Path::new(path)).to_string(),
            at: chrono::Utc::now().timestamp_millis(),
            pinned: false,
        }
    }
}

#[derive(Default)]
pub struct HistoryState {
    entries: Mutex<Vec<HistoryEntry>>,
}

fn history_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("history.json"))
}

fn read(app: &AppHandle) -> Vec<HistoryEntry> {
    history_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write(app: &AppHandle, entries: &[HistoryEntry]) -> Result<(), String> {
    let path = history_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let contents = serde_json::to_vec_pretty(entries).map_err(|error| error.to_string())?;
    // One write, not a rename: this file is a cache of convenience, and a torn
    // write costs a history list rather than a user document.
    fs::write(&path, contents).map_err(|error| error.to_string())
}

/// Entries with deleted files dropped and the list trimmed, pinned entries kept.
fn tidy(mut entries: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    entries.retain(|entry| entry.pinned || Path::new(&entry.path).exists());
    entries.sort_by(|left, right| right.pinned.cmp(&left.pinned).then(right.at.cmp(&left.at)));
    if entries.len() > CAPACITY {
        let pinned = entries.iter().filter(|entry| entry.pinned).count();
        // Never evict a pinned row to make room; trim from the unpinned tail only.
        entries.truncate(CAPACITY.max(pinned));
    }
    entries
}

fn snapshot(app: &AppHandle) -> Vec<HistoryEntry> {
    let state = app.state::<HistoryState>();
    let mut guard = state
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_empty() {
        *guard = tidy(read(app));
    }
    guard.clone()
}

fn publish(app: &AppHandle, entries: Vec<HistoryEntry>) {
    let state = app.state::<HistoryState>();
    if let Ok(mut guard) = state.entries.lock() {
        *guard = entries.clone();
    }
    let _ = app.emit("history://changed", entries);
}

/// Merge imported rows into the library. An entry the library already lists keeps
/// its own timestamp, so an import cannot make yesterday's file look new.
///
/// The kind is re-derived from the path rather than taken from the store being
/// imported from: that field is the importer's own opinion about a file it may not
/// have been able to read, and the current classifier is the one the editor uses.
fn merge(existing: Vec<HistoryEntry>, incoming: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    let mut merged = existing;
    for entry in incoming {
        if entry.path.trim().is_empty() {
            continue;
        }
        if merged.iter().any(|have| have.path == entry.path) {
            continue;
        }
        merged.push(HistoryEntry {
            kind: crate::files::editor_kind(Path::new(&entry.path)).to_string(),
            ..entry
        });
    }
    tidy(merged)
}

/// Record files that just became available. Called by the result pipeline rather
/// than by whichever window happens to be open.
pub fn record(app: &AppHandle, paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    let mut entries = snapshot(app);
    for path in paths {
        if path.trim().is_empty() || !Path::new(path).exists() {
            continue;
        }
        entries.retain(|entry| entry.path != *path);
        entries.push(HistoryEntry::new(path));
    }
    let entries = tidy(entries);
    if let Err(error) = write(app, &entries) {
        eprintln!("Could not save the capture history: {error}");
    }
    publish(app, entries);
}

/// Emit a finished file action *and* record it, so the two can never drift apart.
pub fn publish_result(app: &AppHandle, outcome: crate::models::ActionOutcome) {
    record(app, &outcome.outputs);
    let _ = app.emit("app://file-result", outcome);
}

#[tauri::command]
pub async fn history_list(app: AppHandle) -> Result<Vec<HistoryEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(snapshot(&app)))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn history_record(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<Vec<HistoryEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        record(&app, &paths);
        Ok(snapshot(&app))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn history_set_pinned(
    app: AppHandle,
    path: String,
    pinned: bool,
) -> Result<Vec<HistoryEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut entries = snapshot(&app);
        for entry in entries.iter_mut() {
            if entry.path == path {
                entry.pinned = pinned;
            }
        }
        let entries = tidy(entries);
        write(&app, &entries)?;
        publish(&app, entries.clone());
        Ok(entries)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Forget the entry without touching the file on disk.
#[tauri::command]
pub async fn history_remove(app: AppHandle, path: String) -> Result<Vec<HistoryEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut entries = snapshot(&app);
        entries.retain(|entry| entry.path != path);
        write(&app, &entries)?;
        publish(&app, entries.clone());
        Ok(entries)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Adopt a list a previous version kept somewhere else. Until 0.1 the library lived
/// in the webview's own storage, which a reinstall does not survive, so the first
/// launch of a newer build would otherwise show an empty list over files that are
/// all still on disk.
#[tauri::command]
pub async fn history_import(
    app: AppHandle,
    entries: Vec<HistoryEntry>,
) -> Result<Vec<HistoryEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let entries = merge(snapshot(&app), entries);
        write(&app, &entries)?;
        publish(&app, entries.clone());
        Ok(entries)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn history_clear(app: AppHandle) -> Result<Vec<HistoryEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Pinned rows survive a clear: the button promises an empty list, not an
        // abandoned set of favourites.
        let entries: Vec<HistoryEntry> = snapshot(&app)
            .into_iter()
            .filter(|entry| entry.pinned)
            .collect();
        write(&app, &entries)?;
        publish(&app, entries.clone());
        Ok(entries)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, at: i64, pinned: bool) -> HistoryEntry {
        HistoryEntry {
            path: path.into(),
            kind: "image".into(),
            at,
            pinned,
        }
    }

    #[test]
    fn a_row_from_the_older_webview_store_parses() {
        // What 0.1.0 kept in localStorage: a path, a kind it guessed, and a
        // timestamp. No pin, since that column did not exist yet.
        let here = real_path();
        let json = format!(r#"[{{"path":"{here}","kind":"image","at":1730000000000}}]"#);
        let legacy: Vec<HistoryEntry> = serde_json::from_str(&json).unwrap();
        assert_eq!(legacy.len(), 1);
        assert!(!legacy[0].pinned);
        let merged = merge(vec![], legacy);
        assert_eq!(merged.len(), 1);
        // The importer's guess is replaced by what this build's classifier says.
        assert_eq!(merged[0].kind, "file");
        assert_eq!(merged[0].at, 1_730_000_000_000);
    }

    #[test]
    fn an_import_adds_unknown_paths_and_never_refreshes_a_timestamp() {
        let here = real_path();
        // The folder holding the test binary: a second path that really exists,
        // since a tidy pass drops anything that does not.
        let other = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let merged = merge(
            vec![entry(&here, 500, false)],
            vec![
                // Same file, older stamp: the library's own row wins.
                entry(&here, 100, false),
                entry(&other, 400, false),
            ],
        );
        assert_eq!(merged.len(), 2, "the imported path should have landed");
        let kept = merged.iter().find(|e| e.path == here).unwrap();
        assert_eq!(kept.at, 500, "an import must not refresh a timestamp");
    }

    #[test]
    fn an_import_drops_entries_that_point_at_nothing() {
        let missing = format!("/nonexistent/shift-import-{}.png", std::process::id());
        let merged = merge(vec![], vec![entry(&missing, 100, false)]);
        assert!(merged.is_empty());
    }

    #[test]
    fn an_import_keeps_a_pinned_flag_from_the_older_store() {
        let here = real_path();
        let merged = merge(vec![], vec![entry(&here, 100, true)]);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].pinned);
    }

    #[test]
    fn entries_kind_follows_the_editor_classification() {
        assert_eq!(HistoryEntry::new("/x/a.png").kind, "image");
        assert_eq!(HistoryEntry::new("/x/a.mov").kind, "video");
        assert_eq!(HistoryEntry::new("/x/a.md").kind, "text");
        assert_eq!(HistoryEntry::new("/x/a.bin").kind, "file");
    }

    #[test]
    fn a_missing_file_leaves_the_list() {
        let missing = format!("/nonexistent/shift-{}.png", std::process::id());
        let tidyed = tidy(vec![entry(&missing, 2, false)]);
        assert!(tidyed.is_empty());
    }

    #[test]
    fn a_pinned_entry_outlives_its_file() {
        let missing = format!("/nonexistent/shift-{}.png", std::process::id());
        let tidyed = tidy(vec![entry(&missing, 2, true)]);
        assert_eq!(tidyed.len(), 1);
    }

    /// A tidy pass only keeps paths that exist, so tests point every entry at the
    /// test binary itself and vary just the timestamp.
    fn real_path() -> String {
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    #[test]
    fn the_newest_entry_comes_first() {
        let here = real_path();
        let tidyed = tidy(vec![
            entry(&here, 1, false),
            entry(&here, 5, false),
            entry(&here, 3, false),
        ]);
        let times: Vec<i64> = tidyed.iter().map(|item| item.at).collect();
        assert_eq!(times, vec![5, 3, 1]);
    }

    #[test]
    fn pinned_entries_sort_ahead_and_are_never_trimmed() {
        let here = real_path();
        let mut entries: Vec<HistoryEntry> = (0..CAPACITY + 40)
            .map(|index| entry(&here, index as i64, false))
            .collect();
        entries.push(entry(&here, 1, true));
        let tidyed = tidy(entries);
        assert!(tidyed[0].pinned, "pinned rows sort first");
        assert!(tidyed.len() <= CAPACITY + 1);
        assert!(tidyed.iter().any(|item| item.pinned));
    }

    #[test]
    fn the_library_round_trips_through_json() {
        let entries = vec![
            entry("/tmp/a.png", 10, false),
            entry("/tmp/b.mov", 20, true),
        ];
        let text = serde_json::to_string(&entries).unwrap();
        let parsed: Vec<HistoryEntry> = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(parsed[1].pinned);
        assert_eq!(parsed[0].at, 10);
    }

    #[test]
    fn an_older_file_without_pinned_still_parses() {
        let parsed: Vec<HistoryEntry> =
            serde_json::from_str(r#"[{"path":"/tmp/a.png","kind":"image","at":5}]"#).unwrap();
        assert!(!parsed[0].pinned, "a missing field defaults to unpinned");
    }
}
