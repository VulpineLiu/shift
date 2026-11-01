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
    name: &'static str,
    package_id: &'static str,
    executable: &'static str,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_install_target(id: &str) -> Option<InstallTarget> {
    match id {
        "pandoc" => Some(InstallTarget {
            name: "Pandoc",
            package_id: "pandoc",
            executable: "pandoc",
        }),
        "media" => Some(InstallTarget {
            name: "FFmpeg",
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
            name: "Pandoc",
            package_id: "JohnMacFarlane.Pandoc",
            executable: "pandoc",
        }),
        "media" => Some(InstallTarget {
            name: "FFmpeg",
            package_id: "Gyan.FFmpeg",
            executable: "ffmpeg",
        }),
        "pdf-render" => Some(InstallTarget {
            name: "Poppler",
            package_id: "oschwartz10612.Poppler",
            executable: "pdftoppm",
        }),
        // PDF-to-Word uses the same page renderer as PDF-to-image on Windows.
        "pdf-to-word" => Some(InstallTarget {
            name: "Poppler",
            package_id: "oschwartz10612.Poppler",
            executable: "pdftoppm",
        }),
        "image-backend" => Some(InstallTarget {
            name: "ImageMagick",
            package_id: "ImageMagick.ImageMagick",
            executable: "magick",
        }),
        "pdf-engine" => Some(InstallTarget {
            name: "Typst PDF engine",
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
pub async fn install_runtime_component(component_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || install_runtime_component_sync(&component_id))
        .await
        .map_err(|error| format!("Could not install runtime component: {error}"))?
}

fn install_runtime_component_sync(component_id: &str) -> Result<String, String> {
    let target =
        install_target(component_id).ok_or_else(|| "unsupported-runtime-component".to_string())?;
    let timeout = Duration::from_secs(15 * 60);

    #[cfg(target_os = "macos")]
    {
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
        Ok(format!("{} was installed.", target.name))
    }

    #[cfg(target_os = "windows")]
    {
        let winget = files::resolve_tool("winget").ok_or_else(|| "winget-missing".to_string())?;
        let status = crate::process::status_with_timeout(
            winget,
            &[
                "install",
                "--id",
                target.package_id,
                "--exact",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ],
            timeout,
        )
        .map_err(|error| format!("winget-install-failed: {error}"))?;
        if !status.success() {
            return Err(format!("winget-install-failed: {status}"));
        }
        if files::resolve_tool(target.executable).is_none() {
            return Err("winget-install-missing-binary".to_string());
        }
        Ok(format!("{} was installed.", target.name))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = target;
        Err("runtime-install-unsupported".to_string())
    }
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
}
