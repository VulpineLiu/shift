use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;

use crate::files;
#[cfg(target_os = "macos")]
use crate::pdf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeComponent {
    pub id: String,
    pub name: String,
    pub category: String,
    pub role: String,
    pub source: String,
    pub available: bool,
    pub installable: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub platform: String,
    pub architecture: String,
    pub os_version: String,
    pub app_version: String,
    pub components: Vec<RuntimeComponent>,
}

#[derive(Debug, Clone, Copy)]
struct InstallTarget {
    package_id: &'static str,
    executable: &'static str,
}

/// One argument set handed to `winget install`, tried in the order the installer uses
/// them. A single command line is not enough because two of its flags depend on the
/// package rather than on the caller: without `--source winget` a machine that also
/// carries the msstore source can stop on an agreement that `--silent` never answers,
/// and `--scope user` is refused outright by a package that only installs machine-wide.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WingetAttempt {
    /// Pin the community source and ask for a per-user install explicitly. This is the
    /// rung that can succeed where an unscoped install was refused for want of
    /// administrator rights, and it still needs no elevation.
    UserScope,
    /// The same command without a scope, which is what this app has always sent: WinGet
    /// picks the scope the package itself prefers.
    Unscoped,
    /// Force the machine-wide scope, where Poppler, FFmpeg, ImageMagick and Typst
    /// live. Reaching it means elevating, so it is attempted last.
    MachineScope,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
impl WingetAttempt {
    /// The order a package is offered to `winget`, unelevated. The unscoped shape leads
    /// because it is the command line that has always been sent, so a package that
    /// installs per-user by default is served on the first attempt exactly as before. The
    /// explicit user scope follows as the no-prompt escape hatch for a package whose
    /// default is machine-wide: WinGet rejects an unsupported scope while it is still
    /// choosing an installer, so that rung costs a handshake rather than a download.
    const LADDER: [WingetAttempt; 2] = [WingetAttempt::Unscoped, WingetAttempt::UserScope];

    fn args(self, package_id: &str) -> Vec<String> {
        let scope = match self {
            WingetAttempt::UserScope => Some("user"),
            WingetAttempt::Unscoped => None,
            WingetAttempt::MachineScope => Some("machine"),
        };
        let mut args = vec![
            "install".to_string(),
            "--id".to_string(),
            package_id.to_string(),
            "--exact".to_string(),
            // `winget` and not `msstore`: a store package needs an agreement these
            // flags cannot settle, and none of the components Shift installs live there.
            "--source".to_string(),
            "winget".to_string(),
            "--silent".to_string(),
            "--accept-package-agreements".to_string(),
            "--accept-source-agreements".to_string(),
        ];
        if let Some(scope) = scope {
            args.push("--scope".to_string());
            args.push(scope.to_string());
        }
        args
    }
}

/// What one `winget` run means. WinGet reports several outcomes that are not failures
/// as a non-zero exit code, so `ExitStatus::success()` alone cannot decide: reading
/// "this package is already installed" as an error is exactly what makes the one-click
/// install report failure on a machine where the component is fine.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WingetOutcome {
    /// Installed, or already there. Either way the row's re-detection has the last word.
    Succeeded,
    /// WinGet itself cannot serve the request - disabled app execution alias, no
    /// working source, no network. No package-shaped retry can fix that, so the batch
    /// stops and says which.
    Unrecoverable,
    /// The package needs elevation, which an unelevated attempt cannot supply.
    NeedsElevation,
    /// A real failure of this attempt, worth retrying in another shape.
    Failed,
}

/// WinGet raises its own `APPINSTALLER_CLI_ERROR_*` codes under facility `0x8A15`, and
/// the two elevation outcomes arrive as plain Win32 errors under `0x8007`. Matching the
/// whole code rather than its low 16 bits keeps a Win32 error that happens to share a
/// tail with a WinGet code from being read as the wrong thing.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod winget_codes {
    /// APPINSTALLER_CLI_ERROR_PACKAGE_ALREADY_INSTALLED
    pub const ALREADY_INSTALLED: u32 = 0x8A15_002B;
    /// APPINSTALLER_CLI_ERROR_UPDATE_NOT_APPLICABLE: the package is there and current.
    pub const NO_UPGRADE_APPLICABLE: u32 = 0x8A15_0016;
    /// APPINSTALLER_CLI_ERROR_NO_APPLICABLE_INSTALLER
    pub const NO_APPLICABLE_INSTALLER: u32 = 0x8A15_0061;
    /// APPINSTALLER_CLI_ERROR_INSTALLER_BLOCKED_BY_POLICY
    pub const BLOCKED_BY_POLICY: u32 = 0x8A15_002D;
    /// APPINSTALLER_CLI_ERROR_NO_APPLICATIONS_FOUND
    pub const PACKAGE_NOT_FOUND: u32 = 0x8A15_0018;
    /// APPINSTALLER_CLI_ERROR_SOURCE_NOT_CONFIGURED
    pub const SOURCE_NOT_CONFIGURED: u32 = 0x8A15_00A5;
    /// APPINSTALLER_CLI_ERROR_CLIREPAIR_FAILED: the alias exists but WinGet cannot run.
    pub const CLI_REPAIR_FAILED: u32 = 0x8A15_0003;
    /// APPINSTALLER_CLI_ERROR_PACKAGE_IN_USE: something holds the install target open.
    pub const PACKAGE_IN_USE: u32 = 0x8A15_0064;
    /// ERROR_INSTALL_USEREXIT: the user said no to the elevation prompt.
    pub const ELEVATION_DECLINED: u32 = 0x8007_012C;
    /// ERROR_ELEVATION_REQUIRED
    pub const ELEVATION_REQUIRED: u32 = 0x8007_04C7;
}

/// Classify a `winget` exit code.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn classify_winget_status(code: u32) -> WingetOutcome {
    use winget_codes::*;
    match code {
        0 | ALREADY_INSTALLED | NO_UPGRADE_APPLICABLE => WingetOutcome::Succeeded,
        NO_APPLICABLE_INSTALLER
        | BLOCKED_BY_POLICY
        | PACKAGE_NOT_FOUND
        | SOURCE_NOT_CONFIGURED
        | CLI_REPAIR_FAILED => WingetOutcome::Unrecoverable,
        ELEVATION_DECLINED | ELEVATION_REQUIRED | PACKAGE_IN_USE => WingetOutcome::NeedsElevation,
        _ => WingetOutcome::Failed,
    }
}

/// Second opinion from what `winget` printed, used when the exit code is not one of the
/// codes above. WinGet's own wording is the only place the two outcomes that matter
/// most are distinguishable: an elevation the user refused and a source that was never
/// agreed to both arrive as generic failures.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn classify_winget_output(output: &str) -> Option<WingetOutcome> {
    let text = output.to_ascii_lowercase();
    // "No applicable upgrade was found", "already installed", "Package is already
    // installed" - the component is on the machine whatever the exit code said.
    if text.contains("already installed") || text.contains("no applicable upgrade") {
        return Some(WingetOutcome::Succeeded);
    }
    if text.contains("administrator")
        || text.contains("elevated")
        || text.contains("elevation")
        || text.contains("run as admin")
    {
        return Some(WingetOutcome::NeedsElevation);
    }
    if text.contains("no official windows package manager source")
        || text.contains("found no source")
        || text.contains("source agreement")
        || text.contains("the term 'winget'")
        || text.contains("is not recognized")
        || text.contains("server is unavailable")
        // Reaching for the machine scope cannot fix a machine that cannot download, and
        // an elevation prompt on top of a network failure reads as one more thing broken.
        || text.contains("internet")
        || text.contains("download")
        || text.contains("unable to connect")
        || text.contains("the server returned")
    {
        return Some(WingetOutcome::Unrecoverable);
    }
    None
}

/// Decide from the exit code first, since it is exact, and fall back to the installer's
/// words for everything it leaves ambiguous. A code that is not in the table is a
/// failure of *this* command line, so it stays `Failed` and the next shape is tried;
/// matching the wording only for the phrases above is what keeps a genuine download
/// error from being read as an elevation problem.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn classify_winget_with_output(code: u32, output: &str) -> WingetOutcome {
    match classify_winget_status(code) {
        WingetOutcome::Failed => classify_winget_output(output).unwrap_or(WingetOutcome::Failed),
        outcome => outcome,
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_install_target(id: &str) -> Option<InstallTarget> {
    match id {
        "pandoc" => Some(InstallTarget {
            package_id: "pandoc",
            executable: "pandoc",
        }),
        "media" => Some(InstallTarget {
            package_id: "ffmpeg",
            executable: "ffmpeg",
        }),
        _ => None,
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_install_target(id: &str) -> Option<InstallTarget> {
    match id {
        "pandoc" => Some(InstallTarget {
            package_id: "JohnMacFarlane.Pandoc",
            executable: "pandoc",
        }),
        "media" => Some(InstallTarget {
            package_id: "Gyan.FFmpeg",
            executable: "ffmpeg",
        }),
        "pdf-render" => Some(InstallTarget {
            package_id: "oschwartz10612.Poppler",
            executable: "pdftoppm",
        }),
        // PDF-to-Word uses the same page renderer as PDF-to-image on Windows.
        "pdf-to-word" => Some(InstallTarget {
            package_id: "oschwartz10612.Poppler",
            executable: "pdftoppm",
        }),
        "image-backend" => Some(InstallTarget {
            package_id: "ImageMagick.ImageMagick",
            executable: "magick",
        }),
        "pdf-engine" => Some(InstallTarget {
            package_id: "Typst.Typst",
            executable: "typst",
        }),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn install_target(id: &str) -> Option<InstallTarget> {
    macos_install_target(id)
}

#[cfg(target_os = "windows")]
fn install_target(id: &str) -> Option<InstallTarget> {
    windows_install_target(id)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn install_target(_id: &str) -> Option<InstallTarget> {
    None
}

#[allow(clippy::too_many_arguments)]
fn component(
    id: &str,
    name: impl Into<String>,
    category: &str,
    role: impl Into<String>,
    source: &str,
    available: bool,
    version: Option<String>,
    path: Option<PathBuf>,
) -> RuntimeComponent {
    RuntimeComponent {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        role: role.into(),
        source: source.into(),
        available,
        installable: install_target(id).is_some(),
        version,
        path: path.map(|value| value.to_string_lossy().to_string()),
    }
}

fn command_version(program: &Path, args: &[&str]) -> Option<String> {
    let output =
        crate::process::output_with_timeout(program, args, std::time::Duration::from_secs(4))
            .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout
        .lines()
        .chain(stderr.lines())
        .find_map(meaningful_version_line)
}

fn meaningful_version_line(line: &str) -> Option<String> {
    let line = line.trim_start_matches('\u{feff}').trim();
    (!line.is_empty()).then(|| line.chars().take(180).collect())
}

fn external_component(
    id: &str,
    name: &str,
    category: &str,
    role: impl Into<String>,
    tool: &str,
) -> RuntimeComponent {
    let path = files::resolve_tool(tool);
    component(
        id,
        name,
        category,
        role,
        "external",
        path.is_some(),
        None,
        path,
    )
}

/// The document -> PDF row. macOS prints through the bundled AppKit renderer, and the
/// renderer alone decides the row.
///
/// Regression this shape prevents: the `if let Some(renderer)` once built this component
/// without returning it, so every Mac fell through to the Pandoc row below, which asks
/// for a LaTeX/Typst engine macOS never runs. The panel then reported "Pandoc PDF
/// pipeline" as missing and offered no install button, because `document-pdf` has no
/// install target on macOS. Splitting the two platforms into their own functions makes
/// that fall-through a compile error instead of a silent one.
///
/// Pandoc is still genuinely needed on macOS, but only to turn a picture-bearing DOCX
/// into HTML for this renderer, and it already has its own row and install target in
/// `machine_info`.
#[cfg(target_os = "macos")]
fn document_pdf_component() -> RuntimeComponent {
    let renderer = files::doc_render_bridge();
    component(
        "document-pdf",
        "AppKit document renderer",
        "documents",
        "DOCX / RTF / ODT / TXT / MD / HTML -> PDF (a DOCX with pictures also needs Pandoc, which keeps the images)",
        "bundled",
        renderer.is_some(),
        None,
        renderer,
    )
}

/// Elsewhere the conversion really is Pandoc driving a PDF engine, so the row names
/// whichever engine was found and stays missing until both halves exist. Each half
/// carries its own install target, so a missing row is always fixable from the panel.
#[cfg(not(target_os = "macos"))]
fn document_pdf_component() -> RuntimeComponent {
    let pandoc = files::resolve_tool("pandoc");
    let pdf_engine = files::document_pdf_engine();
    let available = pandoc.is_some() && pdf_engine.is_some();
    let name = match pdf_engine.as_deref().and_then(Path::file_stem) {
        Some(engine) => format!("Pandoc + {}", engine.to_string_lossy()),
        None => "Pandoc PDF pipeline".into(),
    };
    component(
        "document-pdf",
        name,
        "documents",
        "DOCX / RTF / ODT / TXT / MD / HTML -> PDF",
        "external",
        available,
        None,
        pandoc.or(pdf_engine),
    )
}

#[cfg(not(target_os = "macos"))]
fn document_pdf_engine_component() -> RuntimeComponent {
    let path = files::document_pdf_engine();
    let engine = path
        .as_deref()
        .and_then(Path::file_stem)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Typst".into());
    component(
        "pdf-engine",
        format!("Pandoc PDF engine: {engine}"),
        "documents",
        "DOCX / RTF / ODT / TXT / MD / HTML -> PDF",
        "external",
        path.is_some(),
        None,
        path,
    )
}

fn pdf_to_word_component() -> RuntimeComponent {
    #[cfg(target_os = "macos")]
    {
        component(
            "pdf-to-word",
            "Built-in PDF text -> Word",
            "documents",
            "PDF text layer -> DOCX / RTF",
            "bundled",
            true,
            None,
            None,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let path = files::resolve_tool("pdftoppm");
        component(
            "pdf-to-word",
            "Poppler PDF -> Word",
            "documents",
            "PDF pages -> image-preserving DOCX",
            "external",
            path.is_some(),
            None,
            path,
        )
    }
}

fn pdf_page_component() -> RuntimeComponent {
    #[cfg(target_os = "macos")]
    {
        let renderer = pdf::pdf_render_bridge();
        component(
            "pdf-render",
            "PDFKit page renderer",
            "graphics",
            "PDF pages -> PNG / JPG / TIFF",
            "bundled",
            renderer.is_some(),
            None,
            renderer,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        external_component(
            "pdf-render",
            "Poppler pdftoppm",
            "graphics",
            "PDF pages -> PNG / JPG / TIFF",
            "pdftoppm",
        )
    }
}

fn image_backend_component() -> RuntimeComponent {
    #[cfg(target_os = "macos")]
    {
        let path = PathBuf::from("/usr/bin/sips");
        let available = path.is_file();
        component(
            "image-backend",
            "macOS Core Image (sips)",
            "graphics",
            "HEIC / HEIF / AVIF and system image export",
            "system",
            available,
            None,
            Some(path),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        external_component(
            "image-backend",
            "ImageMagick",
            "graphics",
            "HEIC / HEIF / AVIF and system image export",
            "magick",
        )
    }
}

#[cfg(target_os = "windows")]
fn windows_capture_component() -> RuntimeComponent {
    component(
        "screen-capture",
        "Windows GDI capture",
        "capture",
        "Area / full-screen capture",
        "bundled",
        true,
        None,
        None,
    )
}

fn media_component() -> RuntimeComponent {
    let path = files::resolve_tool("ffmpeg");
    component(
        "media",
        "FFmpeg",
        "video",
        "Video / audio conversion, compression and frames",
        "external",
        path.is_some(),
        None,
        path,
    )
}

struct VersionProbe {
    id: String,
    handle: std::thread::JoinHandle<Option<String>>,
}

fn version_args(id: &str) -> Option<&'static [&'static str]> {
    match id {
        "pandoc" | "document-pdf" | "pdf-engine" => Some(&["--version"]),
        "pdf-to-word" | "pdf-render" => Some(&["-v"]),
        "image-backend" => Some(&["--version"]),
        "media" => Some(&["-version"]),
        _ => None,
    }
}

/// Version commands start together and are collected in one pass. Probing them
/// one by one made a single slow executable delay every later row in the panel.
fn fill_component_versions(components: &mut [RuntimeComponent]) {
    let mut probes = Vec::new();
    for component in components.iter() {
        if !component.available || component.source == "bundled" {
            continue;
        }
        let Some(args) = version_args(&component.id) else {
            continue;
        };
        let Some(path) = component.path.as_ref().map(PathBuf::from) else {
            continue;
        };
        if !path.is_file() {
            continue;
        }
        let args = args.to_vec();
        let id = component.id.clone();
        probes.push(VersionProbe {
            id,
            handle: std::thread::spawn(move || command_version(&path, &args)),
        });
    }
    for probe in probes {
        let version = probe.handle.join().ok().flatten();
        if let Some(component) = components
            .iter_mut()
            .find(|component| component.id == probe.id)
        {
            component.version = version;
        }
    }
}

#[tauri::command]
pub async fn machine_info() -> Result<RuntimeInfo, String> {
    tauri::async_runtime::spawn_blocking(machine_info_sync)
        .await
        .map_err(|error| format!("Could not inspect runtime components: {error}"))
}

#[tauri::command]
pub async fn install_runtime_components(component_ids: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || install_runtime_components_sync(&component_ids))
        .await
        .map_err(|error| format!("Could not install runtime components: {error}"))?
}

/// Install a group of components as one request. The package managers hold one lock
/// per tool, so the batch runs one install after another instead of racing itself,
/// and a package that serves several rows (Poppler answers both PDF rows on Windows)
/// is installed once.
///
/// A row that fails does not abandon the rows after it: on Windows the usual reasons are
/// per-package (a declined elevation prompt, one installer that refused), and dropping the
/// rest would punish components that had nothing to do with the failure. Only an answer
/// about WinGet itself - missing, no source, no answer - stops the batch, because every
/// remaining row would spend its full timeout discovering the same thing. Callers refresh
/// afterwards, which is what reports the rows that did land.
fn install_runtime_components_sync(component_ids: &[String]) -> Result<(), String> {
    let mut installed: Vec<&'static str> = Vec::new();
    let mut first_failure: Option<String> = None;
    for component_id in component_ids {
        let target = install_target(component_id)
            .ok_or_else(|| "unsupported-runtime-component".to_string())?;
        if installed.contains(&target.package_id) {
            continue;
        }
        if let Err(error) = install_target_package(target) {
            if ends_the_batch(&error) {
                return Err(error);
            }
            first_failure.get_or_insert(error);
            continue;
        }
        installed.push(target.package_id);
    }
    match first_failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Whether an install failure is a verdict on the package manager rather than on one
/// package, and therefore worth stopping a batch over.
fn ends_the_batch(error: &str) -> bool {
    matches!(
        error,
        "homebrew-missing"
            | "winget-missing"
            | "winget-unavailable"
            | "winget-install-timeout"
            | "winget-launch-failed"
            | "unsupported-runtime-component"
    )
}

fn install_target_package(target: InstallTarget) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let timeout = Duration::from_secs(15 * 60);
        let brew = files::resolve_tool("brew").ok_or_else(|| "homebrew-missing".to_string())?;
        let status =
            crate::process::status_with_timeout(brew, &["install", target.package_id], timeout)
                .map_err(|error| format!("homebrew-install-failed: {error}"))?;
        if !status.success() {
            return Err(format!("homebrew-install-failed: {status}"));
        }
        if files::resolve_tool(target.executable).is_none() {
            return Err("homebrew-install-missing-binary".to_string());
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        install_windows_package(target)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = target;
        Err("runtime-install-unsupported".to_string())
    }
}

/// How long one unelevated `winget` run may take. A package manager that is going to
/// answer at all answers in well under this; the ceiling exists so a run that stops on a
/// prompt it can never be answered cannot spend the whole panel's patience twice.
#[cfg(target_os = "windows")]
const WINGET_TIMEOUT: Duration = Duration::from_secs(8 * 60);

/// Longer for the elevated attempt, whose clock includes the time a person spends
/// reading the UAC prompt.
#[cfg(target_os = "windows")]
const WINGET_ELEVATED_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// Run `winget install` once and say what it meant.
#[cfg(target_os = "windows")]
fn run_winget(
    winget: &Path,
    attempt: WingetAttempt,
    package_id: &str,
) -> Result<WingetOutcome, String> {
    let args = attempt.args(package_id);
    let references: Vec<&str> = args.iter().map(String::as_str).collect();
    // A run that never answered would not answer on the next shape either, so the
    // timeout leaves here as its own code rather than being retried.
    let output = crate::process::output_hidden_with_timeout(winget, &references, WINGET_TIMEOUT)
        .map_err(|error| {
            if error.contains(crate::process::TIMEOUT_MARKER) {
                "winget-install-timeout".to_string()
            } else {
                "winget-launch-failed".to_string()
            }
        })?;
    let code = output.status.code().unwrap_or(-1) as u32;
    let report = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome = classify_winget_with_output(code, &report);
    if outcome != WingetOutcome::Succeeded {
        // The exit code and the installer's own words are worth having in a log, and
        // too long for a sentence: the panel shows a dictionary entry instead.
        eprintln!("winget {code:#010x}: {}", report.trim());
    }
    Ok(outcome)
}

/// Whether the hosting shell said the elevation prompt was refused. PowerShell raises
/// `OperationCanceledException` / "The operation was canceled by the user" for a dismissed
/// UAC dialog, which is the only place that decision appears in words.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn elevation_was_declined(report: &str) -> bool {
    let text = report.to_ascii_lowercase();
    text.contains("canceled by the user")
        || text.contains("cancelled by the user")
        || text.contains("operationwas_canceled")
        || text.contains("this operation requires elevation")
}

/// Install one WinGet package: several command lines, because the flags that decide
/// success are properties of the package rather than of the caller, and one final check
/// of what actually reached disk.
#[cfg(target_os = "windows")]
fn install_windows_package(target: InstallTarget) -> Result<(), String> {
    let winget = files::resolve_tool("winget").ok_or_else(|| "winget-missing".to_string())?;
    for attempt in WingetAttempt::LADDER {
        match run_winget(&winget, attempt, target.package_id)? {
            // Whatever the exit code said, the tool is either on this machine now or it
            // is not, and that is the only question the panel answers.
            WingetOutcome::Succeeded => return verify_windows_install(target),
            WingetOutcome::Unrecoverable => return Err("winget-unavailable".to_string()),
            WingetOutcome::Failed => continue,
            // Keep working down the ladder: a package that answers "needs administrator"
            // to an unscoped install may still install per-user, which is what the next
            // rung asks for. A prompt is only raised once every unelevated shape has been
            // refused, because the prompt itself is what makes an install feel broken to a
            // user who expected a settings switch to just work.
            WingetOutcome::NeedsElevation => continue,
        }
    }
    match elevate_windows_install(target.package_id) {
        // The only outcome the user can act on by pressing the button again.
        Err(ElevationError::Declined) => Err("winget-elevation-declined".to_string()),
        Err(ElevationError::Unavailable) => Err("winget-launch-failed".to_string()),
        Ok(WingetOutcome::Unrecoverable) => Err("winget-unavailable".to_string()),
        // A failure is a failure even when the disk check would also say "not there":
        // the two have different advice, and "restart Shift" is only honest about an
        // install that *reported* success.
        Ok(WingetOutcome::Failed) => Err("winget-install-failed".to_string()),
        // Asking a second time after the machine scope itself was refused would only
        // repeat the refusal, and "restart Shift" would be the wrong advice: the blocker
        // is administrator rights, which is exactly what that message tells the user.
        Ok(WingetOutcome::NeedsElevation) => Err("winget-elevation-declined".to_string()),
        // A machine-scope success lands in a directory the installer itself adds to the
        // machine PATH, which is exactly what `verify_windows_install` re-reads.
        Ok(WingetOutcome::Succeeded) => verify_windows_install(target),
    }
}

/// The panel's answer is only as good as the last check: WinGet reporting success and
/// the executable not existing are both reasons to say "missing", and neither is a
/// reason to claim the install failed when the PATH simply has not been read again.
#[cfg(target_os = "windows")]
fn verify_windows_install(target: InstallTarget) -> Result<(), String> {
    files::refresh_process_path();
    files::reset_tool_cache();
    if files::resolve_tool(target.executable).is_some() {
        return Ok(());
    }
    // An installer that has just finished can still hold the new directory until its
    // own cleanup writes the registry, so look once more after a short pause.
    std::thread::sleep(Duration::from_secs(2));
    files::refresh_process_path();
    files::reset_tool_cache();
    if files::resolve_tool(target.executable).is_some() {
        return Ok(());
    }
    Err("winget-install-missing-binary".to_string())
}

/// What stopped the machine-wide attempt from running at all.
#[cfg(target_os = "windows")]
enum ElevationError {
    /// Windows asked, and the answer was no: a decision the user made, not a malfunction,
    /// and the only one that is fixed by pressing the same button again.
    Declined,
    /// The elevated install could not be started, so nothing was ever refused.
    Unavailable,
}

/// Ask WinGet for the machine-wide install through a shell that can elevate, since a
/// process started without a console cannot raise a UAC prompt of its own.
///
/// `Start-Process -Verb RunAs -Wait -PassThru` is the shape that works from a GUI app:
/// PowerShell shows the prompt, waits for the installer, and hands back its exit code.
/// The elevated `winget` prints into its own console, so its prose never reaches us, but
/// the *hosting* shell's does - and that is where a dismissed prompt announces itself,
/// which no exit code reports reliably.
#[cfg(target_os = "windows")]
fn elevate_windows_install(package_id: &str) -> Result<WingetOutcome, ElevationError> {
    let script = format!(
        "$p = Start-Process -FilePath winget.exe -ArgumentList '{}' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode",
        WingetAttempt::MachineScope
            .args(package_id)
            .iter()
            .map(|argument| format!("'{argument}'"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let output = crate::process::output_hidden_with_timeout(
        powershell(),
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
        WINGET_ELEVATED_TIMEOUT,
    )
    // Either the shell is not there or the prompt was never answered; neither is a
    // refusal, so neither is reported as one.
    .map_err(|_| ElevationError::Unavailable)?;
    let code = output.status.code().unwrap_or(-1) as u32;
    let report = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // PowerShell reports a dismissed prompt as an exception rather than an exit code, and
    // `Start-Process` answers 1223 (ERROR_CANCELLED) when the elevated child reports it.
    if code == 1223 || elevation_was_declined(&report) {
        return Err(ElevationError::Declined);
    }
    // The re-detection is the authority on whether the tool ended up usable, so a success
    // here means "go and look", and a failure is classified for the caller.
    if output.status.success() {
        return Ok(WingetOutcome::Succeeded);
    }
    Ok(classify_winget_with_output(code, &report))
}

#[cfg(target_os = "windows")]
fn powershell() -> PathBuf {
    // The 64-bit path, so a 32-bit host cannot be handed the 32-bit shell whose WinGet
    // view of the machine differs on a few packages.
    let system =
        env::var_os("SystemRoot").unwrap_or_else(|| std::ffi::OsString::from("C:\\Windows"));
    PathBuf::from(system).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

fn machine_info_sync() -> RuntimeInfo {
    let os_version = os_version();
    let mut components = vec![
        component(
            "native-pdf",
            "Built-in PDF engine",
            "documents",
            "PDF merge / split / rotate / compress / metadata",
            "bundled",
            true,
            None,
            None,
        ),
        component(
            "office-parser",
            "Built-in Office parser",
            "documents",
            "PPTX / XLSX / CSV / TSV -> Markdown",
            "bundled",
            true,
            None,
            None,
        ),
        pdf_to_word_component(),
        document_pdf_component(),
        #[cfg(not(target_os = "macos"))]
        document_pdf_engine_component(),
        external_component(
            "pandoc",
            "Pandoc",
            "documents",
            "DOCX / RTF / TXT / MD / HTML conversion",
            "pandoc",
        ),
        component(
            "native-image",
            "Rust image engine",
            "graphics",
            "PNG / JPEG / WebP / TIFF / BMP / GIF",
            "bundled",
            true,
            None,
            None,
        ),
        pdf_page_component(),
        image_backend_component(),
        media_component(),
        component(
            "ocr",
            "Tesseract.js WASM",
            "ocr",
            "English + Simplified Chinese on-device OCR",
            "bundled",
            true,
            None,
            None,
        ),
    ];

    #[cfg(target_os = "macos")]
    components.push(component(
        "textutil",
        "macOS textutil",
        "documents",
        "DOCX / RTF / HTML fallback",
        "system",
        Path::new("/usr/bin/textutil").is_file(),
        None,
        Some(PathBuf::from("/usr/bin/textutil")),
    ));
    #[cfg(target_os = "macos")]
    components.push(component(
        "screen-capture",
        "macOS screencapture",
        "capture",
        "Area/full-screen capture and recording",
        "system",
        Path::new("/usr/sbin/screencapture").is_file(),
        None,
        Some(PathBuf::from("/usr/sbin/screencapture")),
    ));
    #[cfg(target_os = "windows")]
    components.push(windows_capture_component());

    fill_component_versions(&mut components);

    RuntimeInfo {
        platform: match env::consts::OS {
            "macos" => "macOS",
            "windows" => "Windows",
            "linux" => "Linux",
            other => other,
        }
        .into(),
        architecture: env::consts::ARCH.into(),
        os_version,
        app_version: env!("CARGO_PKG_VERSION").into(),
        components,
    }
}

fn os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = crate::process::command("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
        {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !version.is_empty() {
                return version;
            }
        }
        "macOS".into()
    }
    #[cfg(target_os = "windows")]
    {
        windows_os_version()
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(contents) = std::fs::read_to_string("/etc/os-release") {
            if let Some(version) = contents
                .lines()
                .find_map(|line| line.strip_prefix("PRETTY_NAME="))
            {
                return version.trim_matches('"').to_string();
            }
        }
        "Linux".into()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        env::consts::OS.into()
    }
}

#[cfg(target_os = "windows")]
fn windows_os_version() -> String {
    use windows_sys::Win32::System::SystemInformation::{GetVersionExW, OSVERSIONINFOW};

    let mut info: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
    info.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
    let success = unsafe { GetVersionExW(&mut info) };
    if success == 0 {
        return "Windows".into();
    }
    let name = match (info.dwMajorVersion, info.dwMinorVersion) {
        (10, _) => "Windows 10/11",
        (6, 3) => "Windows 8.1",
        (6, 2) => "Windows 8",
        (6, 1) => "Windows 7",
        _ => "Windows",
    };
    format!(
        "{name} {}.{}.{}",
        info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_output_uses_the_first_meaningful_line() {
        assert_eq!(
            meaningful_version_line("\u{feff} Tool 1.2.3"),
            Some("Tool 1.2.3".into())
        );
        assert_eq!(meaningful_version_line("   "), None);
    }

    #[test]
    fn built_in_components_are_always_present() {
        let ids: Vec<String> = machine_info_sync()
            .components
            .into_iter()
            .map(|component| component.id)
            .collect();
        assert!(ids.contains(&"native-image".into()));
        assert!(ids.contains(&"native-pdf".into()));
        assert!(ids.contains(&"ocr".into()));
    }

    #[test]
    fn installable_external_components_map_to_homebrew_formulae() {
        let pandoc = macos_install_target("pandoc").expect("Pandoc should be installable");
        assert_eq!(pandoc.package_id, "pandoc");
        let ffmpeg = macos_install_target("media").expect("FFmpeg should be installable");
        assert_eq!(ffmpeg.package_id, "ffmpeg");
        assert!(macos_install_target("native-pdf").is_none());
    }

    #[test]
    fn windows_external_components_map_to_winget_packages() {
        let pandoc = windows_install_target("pandoc").expect("Pandoc should be installable");
        assert_eq!(pandoc.package_id, "JohnMacFarlane.Pandoc");
        let ffmpeg = windows_install_target("media").expect("FFmpeg should be installable");
        assert_eq!(ffmpeg.package_id, "Gyan.FFmpeg");
        let poppler = windows_install_target("pdf-render").expect("Poppler should be installable");
        assert_eq!(poppler.package_id, "oschwartz10612.Poppler");
        let pdf_to_word =
            windows_install_target("pdf-to-word").expect("Poppler should install PDF-to-Word");
        assert_eq!(pdf_to_word.package_id, "oschwartz10612.Poppler");
        let magick =
            windows_install_target("image-backend").expect("ImageMagick should be installable");
        assert_eq!(magick.package_id, "ImageMagick.ImageMagick");
        let typst = windows_install_target("pdf-engine").expect("Typst should be installable");
        assert_eq!(typst.package_id, "Typst.Typst");
    }

    /// Regression: 0.1.8 built the bundled renderer's component inside `if let
    /// Some(renderer)` but dropped the `return`, so every Mac fell through to the Pandoc
    /// row. That row wants a LaTeX/Typst engine macOS never runs, so the panel read
    /// "Pandoc PDF pipeline: missing" while offering no install button, because
    /// `document-pdf` has no macOS install target. The two platforms now have their own
    /// function, so the fall-through cannot come back silently.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_document_pdf_is_the_bundled_renderer_not_a_pandoc_pipeline() {
        let row = document_pdf_component();
        assert_eq!(row.id, "document-pdf");
        assert_eq!(
            row.source, "bundled",
            "macOS must report its own renderer, not the external Pandoc pipeline"
        );
        assert_eq!(row.name, "AppKit document renderer");
    }

    /// A row the panel marks missing is only honest when the user can do something about
    /// it: either an install button, or a built-in/system component they cannot install.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_missing_component_always_offers_a_way_forward() {
        for component in machine_info_sync().components {
            assert!(
                component.available || component.installable || component.source != "external",
                "{} is reported missing with no install entry",
                component.id
            );
        }
    }

    /// Regression for the Windows one-click install: every command line pinned the
    /// community source, because an unpinned `winget` on a machine that also carries
    /// msstore can stop on an agreement that `--silent` never answers, and the run then
    /// looks identical to a slow install until the deadline kills it.
    #[test]
    fn every_winget_attempt_pins_the_community_source() {
        for attempt in [
            WingetAttempt::UserScope,
            WingetAttempt::Unscoped,
            WingetAttempt::MachineScope,
        ] {
            let args = attempt.args("Gyan.FFmpeg");
            let joined = args.join(" ");
            assert!(joined.contains("--source winget"), "{joined}");
            assert!(joined.contains("--exact"), "{joined}");
            assert!(joined.contains("--accept-source-agreements"), "{joined}");
        }
    }

    /// Elevation is the last resort, so no unelevated rung may ask for the machine scope,
    /// and the rung that goes first has to be the plain command line that this app has
    /// always sent: a package that installs per-user by default must behave exactly as it
    /// did before. The explicit user scope is the escape hatch tried second, for a package
    /// whose default is machine-wide.
    #[test]
    fn the_ladder_escalates_scope_only_when_it_must() {
        let scope = |attempt: WingetAttempt| -> Option<String> {
            let args = attempt.args("pandoc");
            let at = args.iter().position(|arg| arg == "--scope")?;
            args.get(at + 1).cloned()
        };
        assert_eq!(scope(WingetAttempt::Unscoped), None);
        assert_eq!(scope(WingetAttempt::UserScope), Some("user".into()));
        assert_eq!(
            scope(WingetAttempt::MachineScope),
            Some("machine".into()),
            "the escalated attempt must be explicit about the machine scope"
        );
        assert_eq!(
            WingetAttempt::LADDER,
            [WingetAttempt::Unscoped, WingetAttempt::UserScope],
            "neither unelevated rung may request the machine scope"
        );
    }

    /// The heart of the bug: WinGet reports "already installed" as a non-zero exit code.
    /// Reading that as a failure is what made the panel say the one-click install failed
    /// on a machine where the component was already present and working.
    #[test]
    fn an_already_installed_package_is_not_a_failure() {
        use winget_codes::*;
        assert_eq!(classify_winget_status(0), WingetOutcome::Succeeded);
        assert_eq!(
            classify_winget_status(ALREADY_INSTALLED),
            WingetOutcome::Succeeded
        );
        assert_eq!(
            classify_winget_status(NO_UPGRADE_APPLICABLE),
            WingetOutcome::Succeeded
        );
    }

    /// Only elevation-shaped codes may send the installer down the UAC path, and only
    /// package-manager-shaped codes may stop a whole batch. A Win32 error that happens to
    /// share the low 16 bits of a WinGet code must not be confused for one: that is why
    /// the whole code is matched.
    #[test]
    fn exit_codes_are_separated_by_who_can_fix_them() {
        use winget_codes::*;
        assert_eq!(
            classify_winget_status(ELEVATION_REQUIRED),
            WingetOutcome::NeedsElevation
        );
        assert_eq!(
            classify_winget_status(ELEVATION_DECLINED),
            WingetOutcome::NeedsElevation
        );
        assert_eq!(
            classify_winget_status(PACKAGE_IN_USE),
            WingetOutcome::NeedsElevation
        );
        assert_eq!(
            classify_winget_status(CLI_REPAIR_FAILED),
            WingetOutcome::Unrecoverable
        );
        assert_eq!(
            classify_winget_status(SOURCE_NOT_CONFIGURED),
            WingetOutcome::Unrecoverable
        );
        assert_eq!(
            classify_winget_status(BLOCKED_BY_POLICY),
            WingetOutcome::Unrecoverable
        );
        // A plain Win32 code sharing a tail with a WinGet one stays an ordinary failure.
        assert_eq!(classify_winget_status(0x8007_002B), WingetOutcome::Failed);
        assert_eq!(classify_winget_status(0x8007_0005), WingetOutcome::Failed);
    }

    /// Two outcomes that arrive as generic failures are only separable from the
    /// installer's wording, and both change what the user should do next.
    #[test]
    fn the_installer_own_words_break_a_tie() {
        assert_eq!(
            classify_winget_with_output(
                0x8007_0005,
                "Package installation failed: this package requires administrator privileges."
            ),
            WingetOutcome::NeedsElevation
        );
        assert_eq!(
            classify_winget_with_output(0x8007_0005, "No applicable upgrade was found."),
            WingetOutcome::Succeeded
        );
        assert_eq!(
            classify_winget_with_output(0x8007_0005, "Found no source; agreements not accepted."),
            WingetOutcome::Unrecoverable
        );
        // An exact code outranks the prose: an unhelpful log line cannot turn a real
        // success into a retry.
        assert_eq!(
            classify_winget_with_output(0x8A15_002B, "installation failed"),
            WingetOutcome::Succeeded
        );
        assert_eq!(
            classify_winget_with_output(0x8007_0057, "something nobody has read yet"),
            WingetOutcome::Failed
        );
        // A machine that cannot download must stop here. Left to fall through as an
        // ordinary failure it would reach the elevated attempt and raise a UAC prompt
        // that cannot possibly help, which is how a network outage gets reported as
        // "installation failed" on a machine where the answer was "check your Wi-Fi".
        for report in [
            "Internet connection failed. Check your network.",
            "Download of installer failed.",
            "Unable to connect to the server.",
        ] {
            assert_eq!(
                classify_winget_with_output(0x8A15_0101, report),
                WingetOutcome::Unrecoverable,
                "{report} should not escalate to an elevation prompt"
            );
        }
    }

    /// A declined elevation prompt is reported by the hosting shell in prose rather than
    /// by a reliable exit code, and it is the one failure the panel answers with "press
    /// Install again and choose Yes".
    #[test]
    fn a_closed_prompt_reads_as_a_refusal() {
        assert!(elevation_was_declined(
            "Start-Process : The operation was canceled by the user"
        ));
        assert!(elevation_was_declined(
            "This operation requires elevation. Run as administrator."
        ));
        assert!(
            !elevation_was_declined("Internet connection failed. Check your network."),
            "a dead network is not a user saying no"
        );
    }

    /// Only a verdict about the package manager may abandon the rest of a batch. Per-package
    /// outcomes - a declined prompt, one installer that refused - must leave the other rows
    /// free to install, because "一键安装" is asking for all of them.
    #[test]
    fn only_a_broken_package_manager_ends_the_batch() {
        for code in [
            "homebrew-missing",
            "winget-missing",
            "winget-unavailable",
            "winget-install-timeout",
            "winget-launch-failed",
        ] {
            assert!(ends_the_batch(code), "{code} should stop the batch");
        }
        for code in [
            "winget-elevation-declined",
            "winget-install-failed",
            "winget-install-missing-binary",
            "homebrew-install-missing-binary",
        ] {
            assert!(!ends_the_batch(code), "{code} should not stop the batch");
        }
    }
}
