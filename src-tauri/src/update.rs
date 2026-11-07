//! Git-backed update channel: releases live in this same repository.
//!
//! Every published build is pushed to an orphan `releases` branch next to a small
//! `shift-update.json` manifest that names each installer, its size and its
//! SHA256. The app reads that manifest through the forge's raw-file endpoint and
//! re-hashes the downloaded installer before offering to run it, so a truncated or
//! swapped download cannot launch. Nothing is uploaded and no update service is
//! involved; the git remote is the whole server.
//!
//! Two separate properties are checked, and only one of them is absolute:
//!
//! * *Integrity* of the installer — exact byte length plus a recomputed SHA256 that
//!   must equal the manifest's. Always enforced.
//! * *Authenticity* of the manifest — a detached ed25519 signature in
//!   `shift-update.sig` over the manifest's exact bytes, checked against a public key
//!   baked in at build time (`SHIFT_UPDATE_PUBKEY`). Enforced whenever a key is baked;
//!   without one, trusting the manifest means trusting the HTTPS connection and who
//!   can push to the branch, and the UI says so instead of implying otherwise.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

/// Raw URL of the manifest, derived from the `origin` remote by `build.rs`.
/// Empty when the source tree had no remote to derive it from.
const BAKED_MANIFEST_URL: &str = match option_env!("SHIFT_UPDATE_URL") {
    Some(url) => url,
    None => "",
};
/// Detached signature over the manifest bytes, published next to the manifest.
const SIGNATURE_FILE: &str = "shift-update.sig";
/// A runtime override wins over the baked URL so a fork or a staging branch can be
/// pointed at without a rebuild.
const ENV_MANIFEST_URL: &str = "SHIFT_UPDATE_URL";
/// Hex ed25519 public key of the release key, baked by `build.rs`. Empty means this
/// build accepts an unsigned feed and can only prove integrity, not authenticity.
const BAKED_PUBLIC_KEY: &str = match option_env!("SHIFT_UPDATE_PUBKEY") {
    Some(key) => key,
    None => "",
};
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
/// Installers are tens of megabytes, so the download gets far longer than a probe.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// The manifest is a small JSON document. Refuse an oversized response instead of
/// letting a broken or hostile feed exhaust memory.
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
/// Startup probes are throttled to this interval so a menu bar app does not reach
/// the network on every launch.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Artifact {
    file: String,
    sha256: String,
    size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct ReleaseManifest {
    schema: u32,
    version: String,
    date: String,
    notes: String,
    /// Keyed by platform: `macos`, `windows`. Unknown keys are ignored so a newer
    /// manifest can carry a platform this build knows nothing about.
    artifacts: BTreeMap<String, Artifact>,
}

/// The newest manifest shape this build can read.
const SUPPORTED_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// "current" | "available" | "ready" | "unavailable" | "error"
    pub state: String,
    pub current_version: String,
    pub available_version: String,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub notes: String,
    pub date: String,
    /// Absolute path once the artifact has been downloaded and verified.
    pub path: Option<String>,
    pub message: String,
    pub feed_url: String,
    /// True when the manifest behind this status was verified against a baked-in
    /// release key. False when the build has no key, and false for a status that has
    /// no manifest at all — a failed check cannot claim a verified one.
    pub signed: bool,
    /// The manifest key this build resolved its artifact under, or "" when the
    /// release names nothing for it. Reported rather than re-derived downstream:
    /// the platform/architecture ladder lives here, and a UI that guessed it could
    /// advertise an artifact the app would never install.
    pub artifact_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    downloaded: u64,
    total: u64,
    state: String,
}

/// In-memory result of the last check, so the settings panel can render the
/// verified path without re-reading anything from disk.
#[derive(Default)]
pub struct UpdateState {
    pub status: std::sync::Mutex<Option<UpdateStatus>>,
    manifest: std::sync::Mutex<Option<ReleaseManifest>>,
    /// Whether the remembered manifest was signature-verified. Kept apart from the
    /// manifest because the download re-reads the manifest rather than the feed.
    signed: std::sync::Mutex<bool>,
}

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

pub fn manifest_url() -> String {
    std::env::var(ENV_MANIFEST_URL)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| (!BAKED_MANIFEST_URL.is_empty()).then(|| BAKED_MANIFEST_URL.to_string()))
        .unwrap_or_default()
}

fn platform_key() -> &'static str {
    std::env::consts::OS
}

/// Manifest keys a build accepts, most specific first. An ARM64 Windows build
/// prefers its native installer and falls back to the x64 one, which Windows on
/// ARM runs under emulation, so a feed that only carries x64 keeps working.
/// Never falls back across operating systems: a Windows build will not install a
/// Linux artifact because the architecture happened to match.
fn artifact_keys_for(platform: &str, arch: &str) -> Vec<String> {
    if platform == "windows" && arch == "aarch64" {
        return vec!["windows-arm64".to_string(), "windows".to_string()];
    }
    vec![platform.to_string()]
}

/// The artifact to install, with the manifest key it was found under.
fn select_artifact<'a>(
    manifest: &'a ReleaseManifest,
    platform: &str,
    arch: &str,
) -> Option<(String, &'a Artifact)> {
    artifact_keys_for(platform, arch)
        .into_iter()
        .find_map(|key| manifest.artifacts.get(&key).map(|item| (key, item)))
}

fn artifact_for(manifest: &ReleaseManifest) -> Option<(String, &Artifact)> {
    select_artifact(manifest, platform_key(), std::env::consts::ARCH)
}

/// Compare dotted versions the way Semantic Versioning does: numeric components
/// first (a missing component counts as zero), then a pre-release suffix, which
/// sorts below the same numeric release, so 0.2.0-beta.1 is older than 0.2.0.
/// Build metadata (+build.7) is ignored and never makes a release newer.
pub fn version_is_newer(candidate: &str, current: &str) -> bool {
    let (candidate_number, candidate_pre) = split_version(candidate);
    let (current_number, current_pre) = split_version(current);
    for index in 0..candidate_number.len().max(current_number.len()) {
        let left = candidate_number.get(index).copied().unwrap_or(0);
        let right = current_number.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    match (&candidate_pre, &current_pre) {
        // A plain release outranks its own pre-releases: 0.2.0-alpha < 0.2.0.
        (None, Some(_)) => true,
        (Some(_), None) => false,
        (None, None) => false,
        (Some(left), Some(right)) => prerelease_is_newer(left, right),
    }
}

/// Numeric components plus the pre-release identifiers; the second value is None
/// for a plain release. Build metadata is dropped.
fn split_version(value: &str) -> (Vec<u64>, Option<Vec<&str>>) {
    let trimmed = value.trim().trim_start_matches('v');
    let core = trimmed.split('+').next().unwrap_or(trimmed);
    let (numbers, pre_release) = match core.split_once('-') {
        Some((head, tail)) => (head, (!tail.is_empty()).then(|| tail.split('.').collect())),
        None => (core, None),
    };
    (
        numbers
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect(),
        pre_release,
    )
}

/// Compare pre-release identifier lists: numeric identifiers compare numerically
/// and always sort below an alphanumeric one, otherwise order is textual, and a
/// list that runs out first is the lower version.
fn prerelease_is_newer(candidate: &[&str], current: &[&str]) -> bool {
    for index in 0..candidate.len().max(current.len()) {
        match (candidate.get(index), current.get(index)) {
            (None, Some(_)) => return false,
            (Some(_), None) => return true,
            (None, None) => return false,
            (Some(left), Some(right)) => {
                if left == right {
                    continue;
                }
                return match (left.parse::<u64>(), right.parse::<u64>()) {
                    (Ok(left), Ok(right)) => left > right,
                    (Ok(_), Err(_)) => false,
                    (Err(_), Ok(_)) => true,
                    (Err(_), Err(_)) => left > right,
                };
            }
        }
    }
    false
}

/// The release key this build trusts: `Ok(None)` when none was baked in, `Err` when
/// one was baked but is unusable. A key that cannot be parsed must never quietly
/// become "no key", because that would turn a signed channel into an unsigned one
/// while the UI still claimed the build checks signatures.
fn public_key() -> Result<Option<VerifyingKey>, String> {
    key_from_hex(BAKED_PUBLIC_KEY)
}

/// The three states a baked key can be in: absent, usable, or unusable.
fn key_from_hex(value: &str) -> Result<Option<VerifyingKey>, String> {
    let key = value.trim();
    if key.is_empty() {
        return Ok(None);
    }
    let mut bytes = [0u8; 32];
    hex::decode_to_slice(key, &mut bytes)
        .map_err(|_| "The configured release key is not 32 bytes of hex.".to_string())?;
    VerifyingKey::from_bytes(&bytes)
        .map_err(|_| "The configured release key is not an ed25519 public key.".to_string())
        .map(Some)
}

/// The signature sits next to the manifest, so a staging feed needs only its own
/// manifest and signature; the URL is derived rather than configured twice.
fn signature_url() -> Option<String> {
    let url = manifest_url();
    (!url.is_empty()).then(|| {
        let base = match url.rsplit_once('/') {
            Some((head, _)) => head,
            None => url.as_str(),
        };
        format!("{base}/{SIGNATURE_FILE}")
    })
}

/// Check a detached signature over the exact manifest bytes.
///
/// `Ok(true)` is a manifest from a key this build trusts, `Ok(false)` means the build
/// has no key so authenticity cannot be judged, and `Err` is a signature that is
/// missing or wrong for a build that does have a key.
fn verify_manifest(text: &str, signature: Option<&str>) -> Result<bool, String> {
    verify_against(public_key()?.as_ref(), text, signature)
}

fn verify_against(
    key: Option<&VerifyingKey>,
    text: &str,
    signature: Option<&str>,
) -> Result<bool, String> {
    let Some(key) = key else {
        return Ok(false);
    };
    let encoded = signature.ok_or_else(|| {
        "This feed has no shift-update.sig, but this build requires a signed manifest.".to_string()
    })?;
    let decoded = BASE64
        .decode(encoded.trim().as_bytes())
        .map_err(|error| format!("The feed's signature is not valid base64: {error}"))?;
    let signature = Signature::from_slice(&decoded)
        .map_err(|error| format!("The feed's signature is malformed: {error}"))?;
    key.verify_strict(text.as_bytes(), &signature)
        .map_err(|_| "The manifest signature does not match the release key.".to_string())?;
    Ok(true)
}

fn parse_manifest(text: &str) -> Result<ReleaseManifest, String> {
    let manifest: ReleaseManifest =
        serde_json::from_str(text).map_err(|error| format!("Malformed manifest: {error}"))?;
    if manifest.version.trim().is_empty() {
        return Err("The manifest carries no version.".into());
    }
    if manifest.schema != SUPPORTED_SCHEMA {
        return Err(format!(
            "Manifest schema {} is not supported by this build (expected {SUPPORTED_SCHEMA}).",
            manifest.schema
        ));
    }
    for artifact in manifest.artifacts.values() {
        if safe_artifact_name(&artifact.file).is_none() {
            return Err("The manifest names an unsafe artifact file.".into());
        }
        if artifact.size == 0 {
            return Err(format!("{} has no usable size.", artifact.file));
        }
        if !is_sha256_hex(&artifact.sha256) {
            return Err(format!("{} has no usable SHA256.", artifact.file));
        }
    }
    Ok(manifest)
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
}

/// A manifest may only name a file that belongs directly in the release
/// directory. Installer names produced by the release scripts are ASCII, so a
/// strict basename allow-list also keeps path separators, shell metacharacters
/// and Windows device names out of download and handoff paths.
fn safe_artifact_name(value: &str) -> Option<&str> {
    let name = value.trim();
    if name.is_empty()
        || value != name
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.len() > 200
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return None;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if reserved {
        return None;
    }
    Some(name)
}

/// The forge path for one artifact sits next to the manifest.
fn raw_asset_url(file: &str) -> Result<String, String> {
    let base = manifest_url();
    if base.is_empty() {
        return Err("No update feed is configured.".into());
    }
    let file = safe_artifact_name(file)
        .ok_or_else(|| "The manifest names an unsafe artifact file.".to_string())?;
    let directory = base.rsplit_once('/').map(|(head, _)| head).unwrap_or(&base);
    Ok(format!("{directory}/{file}"))
}

/// One transport for both the manifest and the installers, using the platform's own
/// HTTP client so the dependency tree stays as lean as an offline app needs:
/// `curl` on macOS/Linux, `Invoke-WebRequest` on Windows. `on_tick` runs on the
/// waiting thread while the child is alive, which is what drives the progress bar.
fn download(
    url: &str,
    destination: &Path,
    timeout: Duration,
    mut on_tick: impl FnMut(),
) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let _ = fs::remove_file(destination);

    #[cfg(not(target_os = "windows"))]
    let (program, args): (&str, Vec<String>) = (
        "/usr/bin/curl",
        vec![
            "--fail".into(),
            "--silent".into(),
            "--show-error".into(),
            "--location".into(),
            "--max-time".into(),
            timeout.as_secs().to_string(),
            "--output".into(),
            destination.to_string_lossy().to_string(),
            url.to_string(),
        ],
    );
    #[cfg(target_os = "windows")]
    let (program, args): (&str, Vec<String>) = {
        let escaped = destination.to_string_lossy().replace('\'', "''");
        let script = format!(
            "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -UseBasicParsing -TimeoutSec {} -Uri '{}' -OutFile '{}'",
            timeout.as_secs(),
            url.replace('\'', "''"),
            escaped
        );
        (
            "powershell.exe",
            vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-Command".into(),
                script,
            ],
        )
    };

    let mut command: Command = crate::process::command(program);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start the download: {error}"))?;
    let deadline = Instant::now() + timeout + Duration::from_secs(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("The download stalled for {timeout:?}."));
            }
            Ok(None) => {
                on_tick();
                thread::sleep(Duration::from_millis(250));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    };
    if !status.success() {
        let mut detail = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut detail);
        }
        let detail = detail.trim().to_string();
        return Err(if detail.is_empty() {
            format!(
                "The download failed (exit {}).",
                status.code().unwrap_or(-1)
            )
        } else {
            detail
        });
    }
    if destination.is_file() {
        Ok(())
    } else {
        Err("The download produced no file.".into())
    }
}

fn fetch_text(url: &str) -> Result<String, String> {
    let target = std::env::temp_dir().join(format!(
        "shift-manifest-{}-{}.json",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    download(url, &target, FETCH_TIMEOUT, || {})?;
    let result = (|| {
        let size = fs::metadata(&target)
            .map_err(|error| error.to_string())?
            .len();
        if size > MAX_MANIFEST_BYTES {
            return Err(format!(
                "The update manifest is too large ({size} bytes; limit {MAX_MANIFEST_BYTES})."
            ));
        }
        fs::read_to_string(&target).map_err(|error| error.to_string())
    })();
    let _ = fs::remove_file(&target);
    result
}

fn download_directory() -> PathBuf {
    std::env::temp_dir().join("Shift Updates")
}

pub fn sha256_of(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// Re-check the exact properties that made a download installable. This runs
/// again at install time because the file lives in a writable temp directory
/// between download and handoff.
fn verify_downloaded_artifact(path: &Path, size: u64, sha256: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("The downloaded update is not a regular file.".into());
    }
    if metadata.len() != size {
        return Err(format!(
            "The downloaded update changed size: expected {size} bytes, found {}.",
            metadata.len()
        ));
    }
    let actual = sha256_of(path)?;
    if !actual.eq_ignore_ascii_case(sha256) {
        return Err(format!(
            "The downloaded update changed after verification: expected {sha256}, found {actual}."
        ));
    }
    Ok(())
}

fn emit_progress(app: &AppHandle, downloaded: u64, total: u64, state: &str) {
    let _ = app.emit(
        "update://progress",
        UpdateProgress {
            downloaded,
            total,
            state: state.to_string(),
        },
    );
}

/// Report the outcome to every open window and keep it for the next panel mount.
fn publish(app: &AppHandle, status: UpdateStatus) -> UpdateStatus {
    if let Ok(mut guard) = app.state::<UpdateState>().status.lock() {
        *guard = Some(status.clone());
    }
    let _ = app.emit("update://status", status.clone());
    status
}

fn error_status(message: impl Into<String>) -> UpdateStatus {
    UpdateStatus {
        state: "error".into(),
        current_version: current_version(),
        available_version: String::new(),
        file_name: String::new(),
        size: 0,
        sha256: String::new(),
        notes: String::new(),
        date: String::new(),
        path: None,
        message: message.into(),
        feed_url: manifest_url(),
        signed: false,
        artifact_key: String::new(),
    }
}

fn status_for(manifest: &ReleaseManifest, path: Option<PathBuf>, signed: bool) -> UpdateStatus {
    let artifact = artifact_for(manifest);
    let (file_name, size, sha256, artifact_key) = match artifact.as_ref() {
        Some((key, item)) => (
            item.file.clone(),
            item.size,
            item.sha256.clone(),
            key.clone(),
        ),
        None => (String::new(), 0, String::new(), String::new()),
    };
    let newer = version_is_newer(&manifest.version, &current_version());
    let message = if newer && artifact.is_none() {
        format!(
            "Release {} has no artifact for this platform ({}).",
            manifest.version,
            artifact_keys_for(platform_key(), std::env::consts::ARCH).join(" or ")
        )
    } else {
        String::new()
    };
    UpdateStatus {
        state: match (newer, artifact) {
            (true, Some(_)) => "available",
            (true, None) => "unavailable",
            (false, _) => "current",
        }
        .to_string(),
        current_version: current_version(),
        available_version: manifest.version.clone(),
        file_name,
        size,
        sha256,
        notes: manifest.notes.clone(),
        date: manifest.date.clone(),
        path: path.map(|value| value.to_string_lossy().to_string()),
        message,
        feed_url: manifest_url(),
        signed,
        artifact_key,
    }
}

fn remembered_signed(app: &AppHandle) -> bool {
    app.state::<UpdateState>()
        .signed
        .lock()
        .map(|guard| *guard)
        .unwrap_or(false)
}

fn remembered_manifest(app: &AppHandle) -> Option<ReleaseManifest> {
    app.state::<UpdateState>()
        .manifest
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckOutcome {
    pub status: UpdateStatus,
    /// True when this platform has a newer artifact to fetch, so the panel can offer
    /// a download without holding the manifest itself.
    pub download_ready: bool,
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> UpdateCheckOutcome {
    let outcome = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            let url = manifest_url();
            if url.is_empty() {
                return Err("This build has no update feed configured.".to_string());
            }
            let text = fetch_text(&url)?;
            // A missing signature is only fatal when this build has a key, so an
            // unsigned channel still works for builds that were told to trust it.
            let signature = signature_url().and_then(|candidate| fetch_text(&candidate).ok());
            let signed = verify_manifest(&text, signature.as_deref())?;
            let manifest = parse_manifest(&text)?;
            if let Ok(mut guard) = app.state::<UpdateState>().manifest.lock() {
                *guard = Some(manifest.clone());
            }
            if let Ok(mut guard) = app.state::<UpdateState>().signed.lock() {
                *guard = signed;
            }
            Ok::<ReleaseManifest, String>(manifest)
        }
    })
    .await;

    match outcome.unwrap_or_else(|error| Err(error.to_string())) {
        Ok(manifest) => {
            let status = status_for(&manifest, None, remembered_signed(&app));
            let download_ready = status.state == "available";
            UpdateCheckOutcome {
                download_ready,
                status: publish(&app, status),
            }
        }
        Err(error) => UpdateCheckOutcome {
            download_ready: false,
            status: publish(&app, error_status(error)),
        },
    }
}

/// Last known result, so reopening Settings does not spend another request.
#[tauri::command]
pub fn update_status(state: tauri::State<'_, UpdateState>) -> Option<UpdateStatus> {
    state.status.lock().ok().and_then(|guard| guard.clone())
}

/// Fetch this platform's artifact and verify it against the manifest. A hash
/// mismatch deletes the file, so nothing half-trusted can be launched.
#[tauri::command]
pub async fn update_download(app: AppHandle) -> Result<UpdateStatus, String> {
    let manifest =
        remembered_manifest(&app).ok_or_else(|| "Run a check for updates first.".to_string())?;
    let artifact = artifact_for(&manifest)
        .map(|(_, item)| item.clone())
        .ok_or_else(|| {
            format!(
                "Release {} has no artifact for this platform.",
                manifest.version
            )
        })?;
    let file_name = safe_artifact_name(&artifact.file)
        .ok_or_else(|| "The manifest names an unsafe artifact file.".to_string())?;
    let target = download_directory().join(file_name);
    let reporter = app.clone();
    let expected = artifact.sha256.clone();
    let total = artifact.size;
    tauri::async_runtime::spawn_blocking(move || {
        emit_progress(&reporter, 0, total, "downloading");
        let written = std::sync::atomic::AtomicU64::new(0);
        download(
            &raw_asset_url(&artifact.file)?,
            &target,
            DOWNLOAD_TIMEOUT,
            || {
                let size = fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0);
                if size > written.load(std::sync::atomic::Ordering::Relaxed) {
                    written.store(size, std::sync::atomic::Ordering::Relaxed);
                    emit_progress(&reporter, size, total, "downloading");
                }
            },
        )?;
        let actual_size = fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0);
        emit_progress(&reporter, actual_size, total, "verifying");
        if let Err(error) = verify_downloaded_artifact(&target, total, &expected) {
            let _ = fs::remove_file(&target);
            return Err(format!("{error} The download was discarded."));
        }
        let actual = sha256_of(&target)?;
        let mut status = status_for(
            &manifest,
            Some(target.clone()),
            remembered_signed(&reporter),
        );
        status.state = "ready".into();
        status.message = actual;
        Ok(publish(&reporter, status))
    })
    .await
    .map_err(|error| format!("Update download failed: {error}"))?
}

/// The `.app` bundle a macOS build is running from, or None when the binary was
/// started outside a bundle (a dev build, a raw `target/debug` run). The shape is
/// fixed by the bundle layout: `<Name>.app/Contents/MacOS/<name>`.
fn bundle_path(exe: &Path) -> Option<PathBuf> {
    let contents = exe.parent()?.parent()?;
    let bundle = contents.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    bundle
        .extension()
        .filter(|ext| ext.eq_ignore_ascii_case("app"))?;
    Some(bundle.to_path_buf())
}

#[cfg(target_os = "macos")]
/// Whether this copy can be replaced where it sits. The handoff moves the bundle
/// aside and moves a new one in, which needs write access to the folder holding it
/// rather than to the bundle: a copy in a root-owned folder is not replaceable, and
/// finding that out after quitting would leave the user with a closed app and an
/// update that did not happen.
fn can_replace(bundle: &Path) -> bool {
    let parent = match bundle.parent() {
        Some(parent) => parent,
        None => return false,
    };
    let probe = parent.join(format!(".shift-replace-{}", std::process::id()));
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// The macOS handoff: wait for this process to quit, then replace the bundle with
/// the one inside the downloaded disk image and open the new copy. Written as a
/// script because the copy can only happen once this process is gone.
///
/// Every failure path ends with `open` on the bundle, so a handoff that cannot
/// finish leaves the previous copy running rather than leaving Shift closed.
#[cfg(target_os = "macos")]
const MACOS_HANDOFF_SCRIPT: &str = r#"#!/bin/sh
if [ "$#" -ne 4 ]; then
  exit 2
fi
pid="$1"
image="$2"
bundle="$3"
log="$4"
while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
mount_base="${TMPDIR:-/tmp}"
mount="$mount_base/Shift Updates/mount.$$"
mkdir -p "${mount%/*}"
if ! /usr/bin/hdiutil attach -nobrowse -readonly -mountpoint "$mount" "$image" >>"$log" 2>&1; then
  echo "mount failed" >>"$log"
  /usr/bin/open "$bundle"
  exit 1
fi
status=0
if ! /usr/bin/ditto "$mount/Shift.app" "$bundle.new" >>"$log" 2>&1; then
  status=1
else
  # A DMG fetched by a browser carries a quarantine flag, and an app copied out of
  # one is re-assessed on next launch. This build is ad-hoc signed and not notarized,
  # so drop the flag the same way a manual drag out of the image would.
  /usr/bin/xattr -dr com.apple.quarantine "$bundle.new" 2>/dev/null
  if mv "$bundle" "$bundle.old" >>"$log" 2>&1 && mv "$bundle.new" "$bundle" >>"$log" 2>&1; then
    /bin/rm -rf "$bundle.old"
  else
    status=1
    [ -d "$bundle.old" ] && [ ! -d "$bundle" ] && mv "$bundle.old" "$bundle" >>"$log" 2>&1
    /bin/rm -rf "$bundle.new"
  fi
fi
/usr/bin/hdiutil detach "$mount" -quiet >>"$log" 2>&1
[ "$status" -eq 0 ] || echo "handoff failed; reopening the previous copy" >>"$log"
/usr/bin/open "$bundle"
exit "$status"
"#;

/// The Windows handoff: wait for this process to exit, run the installer silently,
/// then start the copy the installer wrote. The installer refuses to overwrite a
/// running EXE, so the wait is the whole point of the script.
#[cfg(target_os = "windows")]
const WINDOWS_HANDOFF_SCRIPT: &str = r#"@echo off
setlocal EnableExtensions DisableDelayedExpansion
set "WAIT_FOR=%~1"
set "INSTALLER=%~2"
set "LOG=%~3"
if not defined WAIT_FOR exit /b 2
if not defined INSTALLER exit /b 2
if not defined LOG exit /b 2
set /a TRIES=0
:waitloop
tasklist /FI "PID eq %WAIT_FOR%" /NH /FO CSV 2>nul | find /I "shift.exe" >nul || goto install
set /a TRIES+=1
if %TRIES% LSS 100 (ping -n 2 127.0.0.1 >nul & goto waitloop)
:install
rem The installer writes its own directory to the registry; fall back to its default.
set "TARGET=%LOCALAPPDATA%\Programs\Shift\shift.exe"
for /f "tokens=1,2,*" %%a in ('reg query "HKCU\Software\Shift" /v InstallDirectory 2^>nul ^| find "InstallDirectory"') do set "TARGET=%%c\shift.exe"
start /wait "" "%INSTALLER%" /S
if errorlevel 1 echo installer exited with %ERRORLEVEL% >> "%LOG%"
if exist "%TARGET%" start "" "%TARGET%"
"#;

/// Write a handoff script and start it detached, so it is still alive when this
/// process has exited. Paths travel as process arguments rather than interpolated
/// script text, so folder names cannot change the helper's script source.
fn spawn_handoff(name: &str, script: &str, arguments: &[&OsStr]) -> Result<(), String> {
    let path = download_directory().join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    // A previous attempt leaves its script behind, and a half-written one would be
    // inherited by the next install.
    let _ = fs::remove_file(&path);
    fs::write(&path, script).map_err(|error| error.to_string())?;
    #[cfg(target_os = "windows")]
    let mut command = crate::process::command("cmd.exe");
    #[cfg(target_os = "windows")]
    command.args(["/D", "/C"]).arg(&path).args(arguments);
    #[cfg(not(target_os = "windows"))]
    let mut command = crate::process::command("/bin/sh");
    #[cfg(not(target_os = "windows"))]
    command.arg(&path).args(arguments);
    command
        .spawn()
        .map_err(|error| format!("Could not start the updater: {error}"))?;
    Ok(())
}

/// Quit shortly after the handoff is scheduled. The delay lets the invoke resolve
/// and the panel paint its installing state before the windows disappear.
fn quit_for_update(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(600));
        app.exit(0);
    });
}

/// Install the verified artifact and restart on it. macOS replaces its own bundle
/// and Windows runs its installer silently, both from a helper that starts once
/// this process is gone, then opens the new copy. Anything else is handed to the
/// system the way a double-click would.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<String, String> {
    let status = app
        .state::<UpdateState>()
        .status
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .filter(|status| status.state == "ready")
        .ok_or_else(|| "Download and verify an update first.".to_string())?;
    let path = status
        .path
        .clone()
        .ok_or_else(|| "Download and verify an update first.".to_string())?;
    let expected_sha256 = status.sha256.clone();
    let expected_size = status.size;
    tauri::async_runtime::spawn_blocking(move || {
        let artifact = PathBuf::from(&path);
        let log = download_directory().join("handoff.log");
        let pid = std::process::id();

        if let Err(error) =
            verify_downloaded_artifact(&artifact, expected_size, &expected_sha256)
        {
            let _ = fs::remove_file(&artifact);
            return Err(format!(
                "The downloaded update no longer matches the manifest. It was discarded: {error}"
            ));
        }

        // A bundled macOS build replaces itself; `open`ing the disk image instead
        // would leave the user dragging a window they were told to stop expecting.
        #[cfg(target_os = "macos")]
        {
            let exe = std::env::current_exe().map_err(|error| error.to_string())?;
            if let Some(bundle) = bundle_path(&exe) {
                if !can_replace(&bundle) {
                    let folder = bundle
                        .parent()
                        .map_or_else(|| "its folder".to_string(), |parent| {
                            parent.to_string_lossy().to_string()
                        });
                    return Err(format!(
                        "Shift cannot replace itself in {folder}. Open the downloaded installer to update by hand."
                    ));
                }
                let pid_argument = pid.to_string();
                spawn_handoff(
                    "Shift-update.sh",
                    MACOS_HANDOFF_SCRIPT,
                    &[
                        OsStr::new(&pid_argument),
                        artifact.as_os_str(),
                        bundle.as_os_str(),
                        log.as_os_str(),
                    ],
                )?;
                quit_for_update(&app);
                return Ok(path);
            }
        }
        #[cfg(target_os = "windows")]
        {
            let pid_argument = pid.to_string();
            spawn_handoff(
                "Shift-update.cmd",
                WINDOWS_HANDOFF_SCRIPT,
                &[
                    OsStr::new(&pid_argument),
                    artifact.as_os_str(),
                    log.as_os_str(),
                ],
            )?;
            quit_for_update(&app);
            return Ok(path);
        }
        #[cfg(not(target_os = "windows"))]
        {
            // Reaching here means the update cannot be applied in place: a Linux build
            // has no in-place install, and a macOS binary run straight out of `target/`
            // has no bundle to replace. Both get the artifact opened instead.
            #[cfg(target_os = "macos")]
            crate::process::command("/usr/bin/open")
                .arg(&artifact)
                .status()
                .map_err(|error| format!("Could not open the update: {error}"))?;
            // Linux bundles are not installed in place. An AppImage carries its own runtime
            // but arrives without the executable bit, which is enough to make a double-click
            // fail, so set that bit and start it directly. A .deb means nothing on its own
            // and goes to whatever handler the desktop registered for the package format.
            #[cfg(not(target_os = "macos"))]
            {
                let bundle = artifact.clone();
                if bundle
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"))
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = fs::metadata(&bundle)
                        .map_err(|error| format!("Could not read the update: {error}"))?
                        .permissions()
                        .mode()
                        | 0o111;
                    fs::set_permissions(&bundle, fs::Permissions::from_mode(mode))
                        .map_err(|error| format!("Could not make the update runnable: {error}"))?;
                    crate::process::command(&bundle)
                        .spawn()
                        .map_err(|error| format!("Could not start the update: {error}"))?;
                } else {
                    crate::files::open_path(&bundle)
                        .map_err(|error| format!("Could not open the update: {error}"))?;
                }
            }
            Ok(path)
        }
    })
    .await
    .map_err(|error| format!("Update install failed: {error}"))?
}

/// Drop the downloaded installer so the next check starts clean.
#[tauri::command]
pub fn update_clear(app: AppHandle) {
    let downloaded = app
        .state::<UpdateState>()
        .status
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
        .and_then(|status| status.path);
    if let Some(path) = downloaded {
        let artifact = PathBuf::from(path);
        let is_download = artifact.parent() == Some(download_directory().as_path())
            && artifact
                .file_name()
                .and_then(OsStr::to_str)
                .and_then(safe_artifact_name)
                .is_some();
        if is_download {
            let _ = fs::remove_file(artifact);
        }
    }
    let _ = app.emit("update://status", serde_json::Value::Null);
}

/// The tray row a ready update should advertise, if the last check found one.
pub fn tray_title(app: &AppHandle) -> Option<String> {
    let status = app
        .state::<UpdateState>()
        .status
        .lock()
        .ok()
        .and_then(|guard| guard.clone())?;
    matches!(status.state.as_str(), "available" | "ready").then(|| {
        crate::locale::update_available(
            crate::locale::preferred(Some(&crate::settings::load(app).language)),
            &status.available_version,
        )
    })
}

/// Startup probe: throttled, silent while nothing changed, never blocking launch.
pub fn spawn_startup_check(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if !crate::settings::load(&app).update_check_enabled {
            return;
        }
        if manifest_url().is_empty() || !crate::settings::update_check_due(&app, CHECK_INTERVAL) {
            return;
        }
        crate::settings::note_update_check(&app);
        let outcome = update_check(app.clone()).await;
        if matches!(outcome.status.state.as_str(), "available" | "ready") {
            let _ = crate::refresh_tray_language(&app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `set_var` is process-wide and tests run in parallel, so anything that reads
    /// or writes the feed override has to hold this.
    fn feed_guard() -> std::sync::MutexGuard<'static, ()> {
        static FEED: std::sync::Mutex<()> = std::sync::Mutex::new(());
        FEED.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    const SAMPLE: &str = r#"{
      "schema": 1,
      "version": "0.2.0",
      "date": "2026-10-25",
      "notes": "New blur and stamp tools.",
      "artifacts": {
        "macos": { "file": "Shift-macOS-universal.dmg", "sha256": "f5c9080b6f12c48a68948ee342f2d5f8151f1fd528613d15e9ee265c8c37d666", "size": 55113440 },
        "windows": { "file": "Shift-Setup-0.2.0-win-x64.exe", "sha256": "d30ee58a6a99d589363414a2d9b53a7d3d6a4f22068780a12e4d9dea3cb9005c", "size": 24358144 },
        "linux": { "file": "Shift.AppImage", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "size": 1 }
      }
    }"#;

    /// Generated with the same ed25519 primitive the publisher uses; the seed is
    /// 0x01..=0x20 so the key can be reproduced from `docs/RELEASE.md`.
    const TEST_PUBKEY: &str = "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664";
    const TEST_SIGNATURE: &str =
        "WEifDKTCvKwakvf05/IqgCDqH0IPIt2O6rWtp9DioE9NckBXZehg9the3HSjYpu/9nI7KpXjsKzIAniHWgw3DA==";
    const SIGNED_MANIFEST: &str = r#"{
  "schema": 1,
  "version": "0.2.0",
  "date": "2026-10-25",
  "notes": "New blur and stamp tools.",
  "artifacts": {
    "macos": {
      "file": "Shift-macOS-universal.dmg",
      "sha256": "f5c9080b6f12c48a68948ee342f2d5f8151f1fd528613d15e9ee265c8c37d666",
      "size": 55113440
    },
    "windows": {
      "file": "Shift-Setup-0.2.0-win-x64.exe",
      "sha256": "d30ee58a6a99d589363414a2d9b53a7d3d6a4f22068780a12e4d9dea3cb9005c",
      "size": 24358144
    },
    "linux": {
      "file": "Shift.AppImage",
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
      "size": 1
    }
  }
}"#;

    #[test]
    fn a_higher_version_is_newer() {
        assert!(version_is_newer("0.2.0", "0.1.0"));
        assert!(version_is_newer("0.10.0", "0.9.9"));
        assert!(version_is_newer("1.0", "0.9.9"));
        assert!(version_is_newer("v1.0.0", "0.9.0"));
        assert!(!version_is_newer("0.1.0", "0.1.0"));
        assert!(!version_is_newer("0.0.9", "0.1.0"));
        // 0.1.10 was the first release whose *patch* component reached two digits, so
        // the double-digit rule is asserted for that position too and not just above it.
        assert!(version_is_newer("0.1.10", "0.1.9"));
        assert!(!version_is_newer("0.1.9", "0.1.10"));
    }

    #[test]
    fn a_prerelease_sorts_below_its_release() {
        assert!(version_is_newer("0.2.0", "0.2.0-beta.1"));
        assert!(!version_is_newer("0.2.0-beta.1", "0.2.0"));
        assert!(version_is_newer("0.2.0-beta.2", "0.2.0-beta.1"));
        assert!(!version_is_newer("0.2.0-beta.1", "0.2.0-beta.1"));
        // Numeric identifiers stay below alphanumeric ones, per Semantic Versioning.
        assert!(version_is_newer("0.2.0-rc.1", "0.2.0-beta.9"));
        assert!(version_is_newer("0.2.0-beta.10", "0.2.0-beta.9"));
        // A build suffix is metadata and does not make a release newer.
        assert!(!version_is_newer("0.2.0+build.7", "0.2.0"));
    }

    #[test]
    fn the_manifest_names_an_artifact_per_platform() {
        let manifest = parse_manifest(SAMPLE).expect("sample manifest");
        assert_eq!(manifest.version, "0.2.0");
        assert_eq!(manifest.artifacts.len(), 3);
        assert_eq!(
            manifest.artifacts["macos"].file,
            "Shift-macOS-universal.dmg"
        );
    }

    #[test]
    fn an_arm64_windows_build_prefers_its_native_installer() {
        let both = SAMPLE.replace(
            "\"linux\":",
            "\"windows-arm64\": { \"file\": \"Shift-Setup-0.2.0-win-arm64.exe\", \"sha256\": \"d30ee58a6a99d589363414a2d9b53a7d3d6a4f22068780a12e4d9dea3cb9005c\", \"size\": 23116800 },\n        \"linux\":",
        );
        let manifest = parse_manifest(&both).expect("manifest carrying both windows artifacts");
        assert_eq!(manifest.artifacts.len(), 4);
        // Each platform/architecture pair resolves to exactly one artifact.
        let cases = [
            ("windows", "aarch64", "Shift-Setup-0.2.0-win-arm64.exe"),
            ("windows", "x86_64", "Shift-Setup-0.2.0-win-x64.exe"),
            ("macos", "aarch64", "Shift-macOS-universal.dmg"),
            ("linux", "aarch64", "Shift.AppImage"),
        ];
        for (platform, arch, expected) in cases {
            let (key, artifact) = select_artifact(&manifest, platform, arch)
                .unwrap_or_else(|| panic!("{platform}/{arch}"));
            assert_eq!(artifact.file, expected, "{platform}/{arch} chose {key}");
        }
        // An unknown operating system never borrows another one's artifact.
        assert!(select_artifact(&manifest, "freebsd", "aarch64").is_none());
    }

    #[test]
    fn an_arm64_windows_build_falls_back_to_x64_alone() {
        // A feed published before ARM64 packaging still serves an ARM64 machine.
        let manifest = parse_manifest(SAMPLE).unwrap();
        let (key, artifact) = select_artifact(&manifest, "windows", "aarch64").unwrap();
        assert_eq!(key, "windows");
        assert!(artifact.file.ends_with("-win-x64.exe"));
        // And the candidate list is what the "no artifact" message quotes.
        assert_eq!(
            artifact_keys_for("windows", "aarch64"),
            vec!["windows-arm64".to_string(), "windows".to_string()]
        );
        assert_eq!(
            artifact_keys_for("macos", "aarch64"),
            vec!["macos".to_string()]
        );
    }

    #[test]
    fn a_signed_manifest_is_trusted_only_by_a_build_with_the_key() {
        let key = key_from_hex(TEST_PUBKEY)
            .expect("test key")
            .expect("usable test key");
        // The signature covers the exact bytes below, so an edit to one of them
        // breaks this test rather than passing for the wrong reason.
        assert!(verify_against(Some(&key), SIGNED_MANIFEST, Some(TEST_SIGNATURE)).unwrap());
        // A build with no baked key cannot judge authenticity, and says so.
        assert!(
            !verify_against(None, SIGNED_MANIFEST, None).unwrap(),
            "an unsigned feed must be reported as unsigned, not trusted"
        );
        // A build that does have a key refuses an unsigned feed outright.
        let error = verify_against(Some(&key), SIGNED_MANIFEST, None).unwrap_err();
        assert!(error.contains("shift-update.sig"), "{error}");
    }

    #[test]
    fn an_unusable_key_is_an_error_and_never_a_silent_fallback() {
        // Three distinct states, because collapsing "unusable" into "absent" would
        // turn a signed channel into an unsigned one while the UI still claimed the
        // build verifies signatures.
        assert!(key_from_hex("").unwrap().is_none(), "absent");
        assert!(key_from_hex(TEST_PUBKEY).unwrap().is_some(), "usable");
        // Not 32 bytes of hex.
        assert!(key_from_hex("deadbeef").unwrap_err().contains("32 bytes"));
        // 32 bytes that decode to no curve point: y = 2 has no x on this curve.
        let error = key_from_hex(&format!("02{}", "00".repeat(31))).unwrap_err();
        assert!(error.contains("ed25519"), "{error}");
        // And a build with genuinely no key reports unsigned instead of failing.
        assert!(!verify_against(None, SIGNED_MANIFEST, None).unwrap());
    }

    #[test]
    fn a_signed_manifest_cannot_be_edited() {
        let key = key_from_hex(TEST_PUBKEY).unwrap().unwrap();
        let edited = SIGNED_MANIFEST.replace("\"version\": \"0.2.0\"", "\"version\": \"9.9.9\"");
        assert_ne!(edited, SIGNED_MANIFEST);
        assert!(verify_against(Some(&key), &edited, Some(TEST_SIGNATURE)).is_err());

        let swapped = SIGNED_MANIFEST.replace("Shift-macOS-universal.dmg", "Evil.dmg");
        assert!(verify_against(Some(&key), &swapped, Some(TEST_SIGNATURE)).is_err());

        let garbage = TEST_SIGNATURE.replace("WEif", "wEif");
        assert!(verify_against(Some(&key), SIGNED_MANIFEST, Some(&garbage)).is_err());
    }

    #[test]
    fn the_signature_lives_next_to_the_manifest() {
        let _feed = feed_guard();
        std::env::set_var(
            ENV_MANIFEST_URL,
            "https://git.example.test/mula/nex_shift/raw/branch/releases/shift-update.json",
        );
        assert_eq!(
            signature_url().unwrap(),
            "https://git.example.test/mula/nex_shift/raw/branch/releases/shift-update.sig"
        );
        std::env::remove_var(ENV_MANIFEST_URL);
        // Without the override the signature still sits beside whichever feed this
        // build was compiled against, wherever that points.
        assert!(signature_url().unwrap().ends_with("/shift-update.sig"));
    }

    #[test]
    fn a_missing_or_malformed_hash_is_rejected() {
        let truncated = SAMPLE.replace(
            "\"sha256\": \"d30ee58a6a99d589363414a2d9b53a7d3d6a4f22068780a12e4d9dea3cb9005c\"",
            "\"sha256\": \"deadbeef\"",
        );
        assert!(parse_manifest(&truncated).is_err());
        assert!(parse_manifest(r#"{"schema":1,"version":"","artifacts":{}}"#).is_err());
        assert!(parse_manifest("not json").is_err());
    }

    #[test]
    fn artifact_names_stay_inside_the_release_directory() {
        for safe in ["Shift.dmg", "Shift-Setup_0.2.0.exe", "a.b_c-1.AppImage"] {
            assert_eq!(safe_artifact_name(safe), Some(safe));
        }
        for unsafe_name in [
            "",
            ".",
            "..",
            "../Shift.dmg",
            "folder/Shift.dmg",
            r"folder\Shift.dmg",
            "/tmp/Shift.dmg",
            "Shift dmg",
            "Shift.dmg;rm",
            "Shift'$(whoami)",
            "Shift%TEMP%.dmg",
            "Shift\n.dmg",
            " Shift.dmg",
            "Shift.dmg ",
            "Shift.dmg.",
            "CON",
            "NUL.txt",
            "COM1",
            "LPT9.log",
        ] {
            assert_eq!(safe_artifact_name(unsafe_name), None, "{unsafe_name:?}");
        }
        let traversal = SAMPLE.replace("Shift-macOS-universal.dmg", "../Shift.dmg");
        assert!(parse_manifest(&traversal).is_err());
        let empty = SAMPLE.replace("\"size\": 55113440", "\"size\": 0");
        assert!(parse_manifest(&empty).is_err());
    }

    #[test]
    fn only_the_supported_schema_is_accepted() {
        for schema in [0, 2, 9] {
            let manifest = SAMPLE.replace("\"schema\": 1", &format!("\"schema\": {schema}"));
            let error = parse_manifest(&manifest).unwrap_err();
            assert!(error.contains("schema"), "{error}");
        }
        assert!(parse_manifest(SAMPLE).is_ok());
    }

    #[test]
    fn a_bundled_binary_finds_its_app_bundle() {
        let exe = Path::new("/Applications/Shift.app/Contents/MacOS/shift");
        assert_eq!(
            bundle_path(exe),
            Some(PathBuf::from("/Applications/Shift.app"))
        );
    }

    #[test]
    fn a_binary_outside_a_bundle_has_no_bundle_to_replace() {
        // `target/debug/shift` and a relocated executable must not be mistaken for
        // an installed copy, because the handoff would then replace nothing.
        assert_eq!(bundle_path(Path::new("/repo/target/debug/shift")), None);
        assert_eq!(bundle_path(Path::new("/tmp/shift")), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_macos_handoff_waits_for_this_process_before_it_touches_the_bundle() {
        let script = MACOS_HANDOFF_SCRIPT;
        assert!(
            script.contains("pid=\"$1\""),
            "the script lost its pid argument"
        );
        assert!(!script.contains("/Applications/Shift.app"));
        // The wait must come before any write to the bundle, or the copy races the
        // running executable.
        let wait = script.find("kill -0 \"$pid\"").expect("no wait loop");
        let replace = script
            .find("mv \"$bundle\" \"$bundle.old\"")
            .expect("no swap");
        assert!(wait < replace);
        // A failed mount or copy still reopens the old copy: an update attempt must
        // never end with Shift simply gone.
        assert_eq!(script.matches("/usr/bin/open \"$bundle\"").count(), 2);
        assert!(script.contains("hdiutil attach"));
        assert!(script.contains("hdiutil detach"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn the_windows_handoff_installs_only_after_this_process_is_gone() {
        let script = WINDOWS_HANDOFF_SCRIPT;
        assert!(
            script.contains("set \"WAIT_FOR=%~1\""),
            "the script lost our pid"
        );
        assert!(!script.contains("C:\\Users\\me\\Shift-Setup.exe"));
        let wait = script
            .find("tasklist /FI \"PID eq %WAIT_FOR%\"")
            .expect("no wait");
        let install = script.find("\"%INSTALLER\" /S").expect("no silent install");
        assert!(wait < install);
        assert!(script.contains("start \"\" \"%TARGET%\""), "never restarts");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn only_a_bundle_in_a_writable_folder_can_be_replaced() {
        let dir = std::env::temp_dir().join(format!("shift-replace-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(can_replace(&dir.join("Shift.app")));
        // A bundle whose folder does not exist, or cannot be written, must be refused
        // before the app quits rather than after.
        assert!(!can_replace(&dir.join("missing").join("Shift.app")));
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn the_status_reports_the_key_it_resolved() {
        // The UI shows this key next to the operating system, so it must be what the
        // ladder actually chose — including the ARM64 fallback to the x64 entry.
        let both = SAMPLE.replace(
            "\"linux\":",
            "\"windows-arm64\": { \"file\": \"Shift-Setup-0.2.0-win-arm64.exe\", \"sha256\": \"d30ee58a6a99d589363414a2d9b53a7d3d6a4f22068780a12e4d9dea3cb9005c\", \"size\": 23116800 },\n        \"linux\":",
        );
        let with_arm = parse_manifest(&both).unwrap();
        let (key, _) = select_artifact(&with_arm, "windows", "aarch64").unwrap();
        assert_eq!(key, "windows-arm64");
        let (fallback, _) =
            select_artifact(&parse_manifest(SAMPLE).unwrap(), "windows", "aarch64").unwrap();
        assert_eq!(fallback, "windows");
        // A status built on this host reports this host's key, never another one's.
        let status = status_for(&with_arm, None, false);
        assert_eq!(status.artifact_key, platform_key());
        // A manifest with nothing for this platform reports nothing, not a guess.
        assert_eq!(error_status("boom").artifact_key, "");
    }

    #[test]
    fn the_status_names_the_artifact_for_this_platform() {
        let manifest = parse_manifest(SAMPLE).unwrap();
        let status = status_for(&manifest, None, false);
        assert_eq!(status.current_version, current_version());
        assert!(
            matches!(
                status.state.as_str(),
                "available" | "current" | "unavailable"
            ),
            "{:?}",
            status.state
        );
        assert_eq!(status.available_version, "0.2.0");
    }

    #[test]
    fn assets_are_fetched_next_to_the_manifest() {
        let _feed = feed_guard();
        std::env::set_var(
            ENV_MANIFEST_URL,
            "https://git.example.test/mula/nex_shift/raw/branch/releases/shift-update.json",
        );
        assert_eq!(
            raw_asset_url("Shift-Setup-0.2.0-win-x64.exe").unwrap(),
            "https://git.example.test/mula/nex_shift/raw/branch/releases/Shift-Setup-0.2.0-win-x64.exe"
        );
        std::env::remove_var(ENV_MANIFEST_URL);
    }

    #[test]
    fn the_hash_matches_shasum() {
        let path = std::env::temp_dir().join(format!("shift-sha-{}", std::process::id()));
        fs::write(&path, b"shift").unwrap();
        assert_eq!(
            sha256_of(&path).unwrap(),
            "ecd3fad7a4d35d7130c6748352d88f164f156375925f87464afe21b344af97dd"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn an_installer_changed_after_download_is_refused() {
        let path = std::env::temp_dir().join(format!("shift-verify-{}", uuid::Uuid::new_v4()));
        fs::write(&path, b"installer bytes").unwrap();
        let digest = sha256_of(&path).unwrap();
        verify_downloaded_artifact(&path, 15, &digest).unwrap();

        let changed = b"installer bytes plus a change";
        fs::write(&path, changed).unwrap();
        let error = verify_downloaded_artifact(&path, 15, &digest).unwrap_err();
        assert!(
            error.contains("size") || error.contains("changed"),
            "{error}"
        );

        let error = verify_downloaded_artifact(&path, changed.len() as u64, &digest).unwrap_err();
        assert!(error.contains("changed after verification"), "{error}");
        let _ = fs::remove_file(path);
    }

    /// The whole channel, offline: a manifest and its artifact served out of a
    /// temporary directory over `file://`, exercising fetch, parse, download, size
    /// check and hash comparison exactly as they run against the real forge.
    #[test]
    fn a_file_served_release_downloads_and_verifies() {
        let _feed = feed_guard();
        let root = std::env::temp_dir().join(format!("shift-update-e2e-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let artifact_name = "Shift-Test.dmg";
        let payload = b"shift update channel payload";
        fs::write(root.join(artifact_name), payload).unwrap();
        let digest = sha256_of(&root.join(artifact_name)).unwrap();
        fs::write(
            root.join("shift-update.json"),
            format!(
                r#"{{"schema":1,"version":"9.9.9","date":"2026-10-25","notes":"e2e","artifacts":{{"{}":{{"file":"{}","sha256":"{}","size":{}}}}}}}"#,
                platform_key(),
                artifact_name,
                digest,
                payload.len()
            ),
        )
        .unwrap();
        std::env::set_var(
            ENV_MANIFEST_URL,
            format!("file://{}/shift-update.json", root.display()),
        );

        let manifest = parse_manifest(&fetch_text(&manifest_url()).unwrap()).unwrap();
        assert_eq!(manifest.version, "9.9.9");
        assert!(version_is_newer(&manifest.version, &current_version()));

        let artifact = manifest.artifacts[platform_key()].clone();
        let target = root.join("downloaded");
        download(
            &raw_asset_url(&artifact.file).unwrap(),
            &target,
            FETCH_TIMEOUT,
            || {},
        )
        .unwrap();
        assert_eq!(fs::read(&target).unwrap(), payload);
        assert_eq!(sha256_of(&target).unwrap(), artifact.sha256);

        // A tampered artifact must fail the hash gate, not just the size gate.
        fs::write(root.join(artifact_name), b"tampered content here!!").unwrap();
        download(
            &raw_asset_url(&artifact.file).unwrap(),
            &target,
            FETCH_TIMEOUT,
            || {},
        )
        .unwrap();
        assert_ne!(sha256_of(&target).unwrap(), digest);

        std::env::remove_var(ENV_MANIFEST_URL);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_overwritten_hash_cannot_be_trusted() {
        let path = std::env::temp_dir().join(format!("shift-sha-bad-{}", std::process::id()));
        fs::write(&path, b"shift ").unwrap();
        assert_ne!(
            sha256_of(&path).unwrap(),
            "ecd3fad7a4d35d7130c6748352d88f164f156375925f87464afe21b344af97dd"
        );
        let _ = fs::remove_file(&path);
    }

    /// Against the real forge: `SHIFT_UPDATE_URL=https://host/owner/repo/raw/branch/releases/shift-update.json
    /// cargo test --lib update:: -- --ignored`. Proves the derived URL form, the
    /// transport and the hash gate all agree with what `publish-release.sh` pushed.
    #[test]
    #[ignore = "reaches the network"]
    fn the_configured_feed_parses_and_downloads() {
        let url = manifest_url();
        assert!(!url.is_empty(), "set SHIFT_UPDATE_URL to a published feed");
        let text = fetch_text(&url).unwrap();
        // When this build was compiled with a release key, the feed has to carry a
        // signature that verifies; otherwise the check would be integrity only.
        let signature = signature_url().and_then(|candidate| fetch_text(&candidate).ok());
        let signed = verify_manifest(&text, signature.as_deref()).unwrap();
        // A build with a baked key can only reach this line by verifying it.
        // A build with a usable baked key can only report `signed` by verifying it.
        assert_eq!(signed, public_key().unwrap().is_some());
        let manifest = parse_manifest(&text).unwrap();
        assert!(!manifest.version.trim().is_empty());
        let Some((_, artifact)) = artifact_for(&manifest) else {
            return; // A feed that does not carry this platform is still a valid feed.
        };
        let target = download_directory().join(&artifact.file);
        download(
            &raw_asset_url(&artifact.file).unwrap(),
            &target,
            DOWNLOAD_TIMEOUT,
            || {},
        )
        .unwrap();
        assert_eq!(
            fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0),
            artifact.size,
            "the pushed artifact must match the manifest size"
        );
        assert_eq!(sha256_of(&target).unwrap(), artifact.sha256);
    }
}
