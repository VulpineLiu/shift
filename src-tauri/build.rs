//! Build-time wiring for the git-backed update channel.
//!
//! The update feed is this repository itself, so the manifest URL is derived from
//! the `origin` remote: `https://host/owner/repo/raw/branch/<release-branch>/
//! shift-update.json`. GitHub uses `/raw/<branch>/` and GitLab `/-/raw/<branch>/`;
//! everything else (Gitea, Forgejo) uses the Gitea form. A `SHIFT_UPDATE_URL`
//! environment variable at build time overrides the derivation, and a source tree
//! with no remote simply builds an app whose updater reports "no feed".
//!
//! The feed's *authenticity* comes from `SHIFT_UPDATE_PUBKEY`: a 64-byte hex ed25519
//! public key baked into the binary, against which the manifest's own signature is
//! checked. A build without it still verifies artifact integrity (size + SHA256) and
//! falls back to trusting the HTTPS connection, which is what it does today.

use std::{path::Path, process::Command};

const RELEASE_BRANCH: &str = "releases";
const MANIFEST: &str = "shift-update.json";
const PUBKEY_ENV: &str = "SHIFT_UPDATE_PUBKEY";

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).parent()?)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// `owner/repo` plus the host scheme, from either ssh or https remote forms.
fn parse_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let (host, path) = if let Some(rest) = url.strip_prefix("ssh://") {
        rest.split_once('/')?
    } else if let Some(rest) = url.find("://").map(|index| &url[index + 3..]) {
        rest.split_once('/')?
    } else if let Some((host, path)) = url.split_once(':') {
        (host.strip_prefix("git@")?, path)
    } else {
        return None;
    };
    (!host.is_empty() && !path.is_empty()).then(|| (format!("https://{host}"), path.to_string()))
}

fn raw_directory(host: &str, path: &str) -> String {
    let domain = host.replace("https://", "");
    let segment = if domain.ends_with("github.com") {
        format!("raw/{RELEASE_BRANCH}")
    } else if domain.contains("gitlab") {
        format!("-/raw/{RELEASE_BRANCH}")
    } else {
        format!("raw/branch/{RELEASE_BRANCH}")
    };
    format!("{host}/{path}/{segment}")
}

fn manifest_url() -> Option<String> {
    if let Ok(url) = std::env::var("SHIFT_UPDATE_URL") {
        if !url.trim().is_empty() {
            return Some(url.trim().to_string());
        }
    }
    let (host, path) = parse_remote(&git(&["remote", "get-url", "origin"])?)?;
    Some(format!("{}/{}", raw_directory(&host, &path), MANIFEST))
}

/// The baked publish key, normalised to 64 lowercase hex characters. Anything else
/// is a typo worth failing the build over rather than shipping an updater that
/// silently accepts an unsigned feed.
fn public_key() -> Result<String, String> {
    let raw = match std::env::var(PUBKEY_ENV) {
        Ok(value) => value,
        Err(_) => return Ok(String::new()),
    };
    let value: String = raw
        .trim()
        .chars()
        .filter(|character| !character.is_whitespace() && *character != ':')
        .collect::<String>()
        .to_ascii_lowercase();
    if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{PUBKEY_ENV} must be 64 hex characters (32 bytes)"));
    }
    Ok(value)
}

fn main() {
    println!("cargo:rerun-if-changed=../dist");
    println!("cargo:rerun-if-env-changed=SHIFT_UPDATE_URL");
    println!("cargo:rerun-if-env-changed={PUBKEY_ENV}");
    // The remote can change between builds, so the derived URL refreshes whenever
    // git metadata moves. Cheap enough to re-check on every build.
    println!("cargo:rerun-if-changed=../.git/config");
    println!(
        "cargo:rustc-env=SHIFT_UPDATE_URL={}",
        manifest_url().unwrap_or_default()
    );
    match public_key() {
        Ok(key) => println!("cargo:rustc-env=SHIFT_UPDATE_PUBKEY={key}"),
        Err(error) => panic!("{error}"),
    }
    tauri_build::build()
}
