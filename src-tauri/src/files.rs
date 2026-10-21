use std::{
    fs::{self, File},
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::UNIX_EPOCH,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{codecs::jpeg::JpegEncoder, DynamicImage, GenericImageView, ImageFormat, RgbImage};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::{ActionOutcome, FileInfo},
    office, pdf, settings,
};

const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "tiff", "tif", "bmp", "gif", "heic", "heif", "avif",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "m4v", "avi", "mkv", "webm"];
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "m4a", "aiff", "aif", "caf", "flac", "ogg"];
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "docx", "doc", "rtf", "odt", "txt", "md", "html", "htm", "pages",
];
const PRESENTATION_EXTENSIONS: &[&str] = &["pptx", "ppt", "key"];
const SPREADSHEET_EXTENSIONS: &[&str] = &["xlsx", "xls", "xlsb", "ods", "csv", "tsv", "numbers"];

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn kind_of(ext: &str) -> &'static str {
    if IMAGE_EXTENSIONS.contains(&ext) {
        "image"
    } else if VIDEO_EXTENSIONS.contains(&ext) {
        "video"
    } else if AUDIO_EXTENSIONS.contains(&ext) {
        "audio"
    } else if ext == "pdf" {
        "pdf"
    } else if DOCUMENT_EXTENSIONS.contains(&ext) {
        "document"
    } else if PRESENTATION_EXTENSIONS.contains(&ext) {
        "presentation"
    } else if SPREADSHEET_EXTENSIONS.contains(&ext) {
        "spreadsheet"
    } else {
        "other"
    }
}

fn preview_for(path: &Path, ext: &str) -> Option<(String, Option<u32>, Option<u32>)> {
    if !IMAGE_EXTENSIONS.contains(&ext) {
        return None;
    }
    let image = image::open(path).ok()?;
    let (width, height) = image.dimensions();
    let preview = image.thumbnail(520, 520);
    let mut png = Vec::new();
    preview
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .ok()?;
    Some((
        format!("data:image/png;base64,{}", STANDARD.encode(png)),
        Some(width),
        Some(height),
    ))
}

pub fn inspect_paths(paths: &[String]) -> Result<Vec<FileInfo>, String> {
    let mut files = Vec::new();
    for raw_path in paths {
        let path = PathBuf::from(raw_path);
        if !path.is_file() {
            continue;
        }
        let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
        let ext = extension_of(&path);
        let preview = preview_for(&path, &ext);
        files.push(FileInfo {
            path: path.to_string_lossy().to_string(),
            name: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string(),
            extension: ext.clone(),
            kind: kind_of(&ext).to_string(),
            size: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_secs() as i64),
            preview: preview.as_ref().map(|value| value.0.clone()),
            width: preview.as_ref().and_then(|value| value.1),
            height: preview.as_ref().and_then(|value| value.2),
        });
    }
    Ok(files)
}

pub(crate) fn unique_output(input: &Path, suffix: &str, ext: &str) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    let base = format!("{stem}{suffix}");
    let mut candidate = parent.join(format!("{base}.{ext}"));
    let mut index = 2;
    while candidate.exists() {
        candidate = parent.join(format!("{base} {index}.{ext}"));
        index += 1;
    }
    candidate
}

pub(crate) fn unique_output_in(
    input: &Path,
    output_dir: Option<&Path>,
    suffix: &str,
    ext: &str,
) -> Result<PathBuf, String> {
    let Some(directory) = output_dir else {
        return Ok(unique_output(input, suffix, ext));
    };
    fs::create_dir_all(directory).map_err(|error| {
        format!(
            "Could not create {}. Choose another output folder in Settings. {error}",
            directory.display()
        )
    })?;
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    let base = format!("{stem}{suffix}");
    let mut candidate = directory.join(format!("{base}.{ext}"));
    let mut index = 2;
    while candidate.exists() {
        candidate = directory.join(format!("{base} {index}.{ext}"));
        index += 1;
    }
    Ok(candidate)
}

fn normalized_target(target: &str) -> (&'static str, ImageFormat) {
    match target.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => ("jpg", ImageFormat::Jpeg),
        "png" => ("png", ImageFormat::Png),
        "webp" => ("webp", ImageFormat::WebP),
        "tiff" | "tif" => ("tiff", ImageFormat::Tiff),
        "bmp" => ("bmp", ImageFormat::Bmp),
        "gif" => ("gif", ImageFormat::Gif),
        _ => ("png", ImageFormat::Png),
    }
}

fn save_image(
    image: &DynamicImage,
    output: &Path,
    format: ImageFormat,
    quality: u8,
) -> Result<(), String> {
    match format {
        ImageFormat::Jpeg => {
            let rgb = flatten_to_rgb(image);
            let mut file =
                File::create(output).map_err(|error| describe_write_error(output, error))?;
            JpegEncoder::new_with_quality(&mut file, quality.clamp(30, 95))
                .encode_image(&DynamicImage::ImageRgb8(rgb))
                .map_err(|error| format!("Could not write {}: {error}", output.display()))
        }
        _ => image
            .save_with_format(output, format)
            .map_err(|error| format!("Could not write {}: {error}", output.display())),
    }
}

fn describe_write_error(output: &Path, error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        return format!(
            "Shift cannot write to {}. Allow access in System Settings > Privacy & Security > Files and Folders, or choose another output folder in Settings.",
            output
                .parent()
                .unwrap_or(output)
                .display()
        );
    }
    format!("Could not write {}: {error}", output.display())
}

fn flatten_to_rgb(image: &DynamicImage) -> RgbImage {
    let rgba = image.to_rgba8();
    let mut rgb = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = pixel[3] as f32 / 255.0;
        let blend = |channel: u8| -> u8 {
            (channel as f32 * alpha + 255.0 * (1.0 - alpha))
                .round()
                .clamp(0.0, 255.0) as u8
        };
        rgb.put_pixel(
            x,
            y,
            image::Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]),
        );
    }
    rgb
}

#[cfg(target_os = "macos")]
fn macos_convert(input: &Path, output: &Path, target: &str) -> Result<(), String> {
    let format = match target {
        "jpg" | "jpeg" => "jpeg",
        "heic" | "heif" => "heic",
        "avif" => "avif",
        "tif" => "tiff",
        "pdf" => "pdf",
        other => other,
    };
    let status = Command::new("/usr/bin/sips")
        .args([
            "-s",
            "format",
            format,
            input.to_string_lossy().as_ref(),
            "--out",
            output.to_string_lossy().as_ref(),
        ])
        .status()
        .map_err(|error| format!("System converter unavailable: {error}"))?;
    if status.success() && output.exists() {
        Ok(())
    } else {
        Err(format!(
            "This Mac cannot write {} files. Try PNG or WEBP.",
            target.to_ascii_uppercase()
        ))
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn system_convert(input: &Path, output: &Path, target: &str) -> Result<(), String> {
    macos_convert(input, output, target)
}

#[cfg(target_os = "windows")]
pub(crate) fn system_convert(input: &Path, output: &Path, target: &str) -> Result<(), String> {
    let Some(magick) = resolve_tool("magick") else {
        return Err("HEIC/AVIF export on Windows requires ImageMagick (magick).".into());
    };
    let status = Command::new(magick)
        .arg(input)
        .arg(output)
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() && output.exists() {
        Ok(())
    } else {
        Err(format!(
            "Could not write {} with ImageMagick.",
            target.to_ascii_uppercase()
        ))
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn system_convert(input: &Path, output: &Path, target: &str) -> Result<(), String> {
    let Some(magick) = resolve_tool("magick") else {
        return Err("HEIC/AVIF export on Linux requires ImageMagick (magick).".into());
    };
    let status = Command::new(magick)
        .arg(input)
        .arg(output)
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() && output.exists() {
        Ok(())
    } else {
        Err(format!(
            "Could not write {} with ImageMagick.",
            target.to_ascii_uppercase()
        ))
    }
}

fn convert_file(input: &Path, target: &str, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let (ext, format) = normalized_target(target);
    let output = unique_output_in(input, output_dir, "", ext)?;
    let direct_formats = ["png", "jpg", "webp", "tiff", "bmp", "gif"];

    if direct_formats.contains(&ext) {
        if let Ok(image) = image::open(input) {
            save_image(&image, &output, format, 92)?;
            return Ok(output);
        }
    }
    system_convert(input, &output, ext)?;
    Ok(output)
}

pub(crate) fn resolve_tool(name: &str) -> Option<PathBuf> {
    if let Ok(path) = which::which(name) {
        if path.is_file() {
            return Some(path);
        }
    }
    #[cfg(target_os = "macos")]
    let directories = [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ];
    #[cfg(not(target_os = "macos"))]
    let directories = ["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"];
    directories
        .into_iter()
        .map(|directory| Path::new(directory).join(name))
        .find(|candidate| candidate.is_file())
}

fn ffmpeg_path() -> Result<PathBuf, String> {
    resolve_tool("ffmpeg").ok_or_else(|| {
        "This conversion needs ffmpeg. Install it with Homebrew or add it to your PATH.".to_string()
    })
}

fn ffmpeg_has_encoder(name: &str) -> bool {
    let Ok(ffmpeg) = ffmpeg_path() else {
        return false;
    };
    let Ok(output) = Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
    else {
        return false;
    };
    String::from_utf8_lossy(&output.stdout).contains(name)
}

fn video_encoder() -> &'static str {
    static ENCODER: OnceLock<&'static str> = OnceLock::new();
    ENCODER.get_or_init(|| {
        if ffmpeg_has_encoder("libx264") {
            "libx264"
        } else if ffmpeg_has_encoder("h264_videotoolbox") {
            "h264_videotoolbox"
        } else {
            "mpeg4"
        }
    })
}

fn video_encode_args(crf: &'static str, bitrate: &'static str) -> Vec<&'static str> {
    match video_encoder() {
        "libx264" => vec![
            "-c:v", "libx264", "-preset", "medium", "-crf", crf, "-pix_fmt", "yuv420p",
        ],
        "h264_videotoolbox" => vec![
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            bitrate,
            "-pix_fmt",
            "yuv420p",
        ],
        _ => vec!["-c:v", "mpeg4", "-q:v", "5", "-pix_fmt", "yuv420p"],
    }
}

/// Video -> GIF stays small and predictable: first 15 seconds, 12 fps, 480 px wide.
const GIF_MAX_SECONDS: &str = "15";
const GIF_FILTER: &str = "fps=12,scale=480:-1:flags=lanczos";

fn convert_media(input: &Path, target: &str, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let ffmpeg = ffmpeg_path()?;
    let (ext, target) = match target.to_ascii_lowercase().as_str() {
        "mp4" => ("mp4", "mp4"),
        "mov" => ("mov", "mov"),
        "gif" => ("gif", "gif"),
        "m4a" => ("m4a", "m4a"),
        "wav" => ("wav", "wav"),
        "mp3" => ("mp3", "mp3"),
        other => return Err(format!("Unsupported media format: {other}")),
    };
    let output = unique_output_in(input, output_dir, "", ext)?;
    let mut command = Command::new(ffmpeg);
    command.args(["-y", "-i", input.to_string_lossy().as_ref()]);
    match target {
        "gif" => {
            // Cap duration and width: without these a minute of screen recording
            // turns into a hundreds-of-megabytes animated file.
            command.args(["-t", GIF_MAX_SECONDS]);
            command.args(["-vf", GIF_FILTER, "-loop", "0"]);
        }
        "m4a" => {
            command.args(["-vn", "-c:a", "aac", "-b:a", "192k"]);
        }
        "wav" => {
            command.args(["-vn", "-c:a", "pcm_s16le"]);
        }
        "mp3" => {
            if ffmpeg_has_encoder("libmp3lame") {
                command.args(["-vn", "-c:a", "libmp3lame", "-b:a", "192k"]);
            } else {
                return convert_mp3_with_lame(input, &output);
            }
        }
        "mp4" | "mov" => {
            command.args(video_encode_args("20", "6M"));
            command.args(["-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart"]);
        }
        _ => {}
    }
    let status = command
        .arg(output.to_string_lossy().as_ref())
        .status()
        .map_err(|error| format!("Could not run ffmpeg: {error}"))?;
    if !status.success() || !output.exists() {
        return Err("Media conversion failed.".into());
    }
    Ok(output)
}

fn convert_mp3_with_lame(input: &Path, output: &Path) -> Result<PathBuf, String> {
    let lame = resolve_tool("lame").ok_or_else(|| {
        "MP3 export needs an FFmpeg build with libmp3lame, or LAME on your PATH.".to_string()
    })?;
    let ffmpeg = ffmpeg_path()?;
    let wav = std::env::temp_dir().join(format!("shift-{}.wav", uuid::Uuid::new_v4()));
    let status = Command::new(ffmpeg)
        .args(["-y", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "2", "-ar", "44100"])
        .arg(&wav)
        .status()
        .map_err(|error| format!("Could not extract audio: {error}"))?;
    if !status.success() || !wav.exists() {
        let _ = fs::remove_file(&wav);
        return Err("Could not extract audio for MP3 export.".into());
    }
    let status = Command::new(lame)
        .args(["--silent", "-b", "192"])
        .arg(&wav)
        .arg(output)
        .status()
        .map_err(|error| format!("Could not run LAME: {error}"))?;
    let _ = fs::remove_file(&wav);
    if !status.success() || !output.exists() {
        return Err("MP3 export failed.".into());
    }
    Ok(output.to_path_buf())
}

fn compress_media(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let ffmpeg = ffmpeg_path()?;
    let ext = extension_of(input);
    let (extension, audio_only) = if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        ("m4a", true)
    } else {
        ("mp4", false)
    };
    let output = unique_output_in(input, output_dir, " (compressed)", extension)?;
    let mut command = Command::new(ffmpeg);
    command.args(["-y", "-i", input.to_string_lossy().as_ref()]);
    if audio_only {
        command.args(["-vn", "-c:a", "aac", "-b:a", "128k"]);
    } else {
        command.args(video_encode_args("28", "2M"));
        command.args(["-c:a", "aac", "-b:a", "96k", "-movflags", "+faststart"]);
    }
    let status = command
        .arg(output.to_string_lossy().as_ref())
        .status()
        .map_err(|error| format!("Could not run ffmpeg: {error}"))?;
    if !status.success() || !output.exists() {
        return Err("Media compression failed.".into());
    }
    Ok(output)
}

#[cfg(target_os = "macos")]
fn doc_render_bridge() -> Option<PathBuf> {
    let triple = match std::env::consts::ARCH {
        "aarch64" => "aarch64-apple-darwin",
        "x86_64" => "x86_64-apple-darwin",
        _ => return None,
    };
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("doc-render-{triple}"));
    if dev.exists() {
        return Some(dev);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            let sidecar = parent.join("doc-render");
            if sidecar.exists() {
                return Some(sidecar);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn native_document_pdf(input: &Path, output_dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let Some(renderer) = doc_render_bridge() else {
        return Ok(None);
    };
    let extension = extension_of(input);
    let mut source = input.to_path_buf();
    let mut temporary = None;
    if matches!(extension.as_str(), "md" | "markdown") {
        if let Some(pandoc) = resolve_tool("pandoc") {
            let path =
                std::env::temp_dir().join(format!("shift-doc-{}.html", uuid::Uuid::new_v4()));
            let status = Command::new(pandoc)
                .arg(input)
                .args(["-s", "-t", "html", "-o"])
                .arg(&path)
                .status()
                .map_err(|error| format!("Could not run pandoc: {error}"))?;
            if status.success() && path.exists() {
                source = path.clone();
                temporary = Some(path);
            }
        }
    }
    let output = unique_output_in(input, output_dir, "", "pdf")?;
    let status = Command::new(renderer)
        .arg(&source)
        .arg(&output)
        .status()
        .map_err(|error| format!("Could not start the document renderer: {error}"))?;
    if let Some(path) = temporary {
        let _ = fs::remove_file(path);
    }
    if !status.success() || !output.exists() {
        let _ = fs::remove_file(&output);
        return Err("Could not render this document to PDF.".into());
    }
    Ok(Some(output))
}

/// Renames a generated file from a throwaway intermediate name to one derived
/// from the original input, keeping whatever extension the writer produced.
fn rename_office_output(
    generated: PathBuf,
    input: &Path,
    target_dir: &Path,
) -> Result<PathBuf, String> {
    let extension = generated
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("pdf")
        .to_ascii_lowercase();
    let desired = unique_output_in(input, Some(target_dir), "", &extension)?;
    if generated != desired {
        fs::rename(&generated, &desired)
            .map_err(|error| format!("Could not name the output file: {error}"))?;
    }
    Ok(desired)
}

fn convert_document(
    input: &Path,
    target: &str,
    output_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    let extension = extension_of(input);
    if extension == "pages" {
        return Err("Pages files are not supported directly. Export as DOCX first.".into());
    }
    if target.eq_ignore_ascii_case("pdf") {
        #[cfg(target_os = "macos")]
        if let Some(output) = native_document_pdf(input, output_dir)? {
            return Ok(output);
        }
    }
    let pandoc = resolve_tool("pandoc").ok_or_else(|| {
        "Document conversion needs pandoc. Install it with Homebrew or add it to your PATH."
            .to_string()
    })?;
    let (extension, writer) = match target.to_ascii_lowercase().as_str() {
        "md" | "markdown" => ("md", Some("gfm")),
        "html" => ("html", Some("html")),
        "txt" => ("txt", Some("plain")),
        "docx" => ("docx", None),
        "rtf" => ("rtf", None),
        "pdf" => ("pdf", None),
        "odt" => ("odt", None),
        other => return Err(format!("Unsupported document format: {other}")),
    };
    let output = unique_output_in(input, output_dir, "", extension)?;
    let mut command = Command::new(pandoc);
    command.arg(input);
    if let Some(writer) = writer {
        command.args(["-t", writer]);
    }
    let status = command
        .args(["-o", output.to_string_lossy().as_ref()])
        .status()
        .map_err(|error| format!("Could not run pandoc: {error}"))?;
    if !status.success() || !output.exists() {
        return Err(format!(
            "Could not convert {} to {}. Check that pandoc and the required PDF engine are installed.",
            input.display(),
            extension.to_ascii_uppercase()
        ));
    }
    Ok(output)
}

fn compress_file(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let image = image::open(input).map_err(|error| format!("Could not read image: {error}"))?;
    let has_alpha = image.color().has_alpha();
    let (ext, format) = if has_alpha {
        ("png", ImageFormat::Png)
    } else {
        ("jpg", ImageFormat::Jpeg)
    };
    let output = unique_output_in(input, output_dir, " (compressed)", ext)?;
    save_image(&image, &output, format, 68)?;
    Ok(output)
}

fn strip_metadata(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let image = image::open(input).map_err(|error| format!("Could not read image: {error}"))?;
    let ext = extension_of(input);
    let (ext, format) = normalized_target(&ext);
    let output = unique_output_in(input, output_dir, " (clean)", ext)?;
    save_image(&image, &output, format, 92)?;
    Ok(output)
}

fn resize_file(input: &Path, scale: f64, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let image = image::open(input).map_err(|error| format!("Could not read image: {error}"))?;
    let (width, height) = image.dimensions();
    let target_width = ((width as f64 * scale).round() as u32).max(1);
    let target_height = ((height as f64 * scale).round() as u32).max(1);
    let resized = image.resize_exact(
        target_width,
        target_height,
        image::imageops::FilterType::Lanczos3,
    );
    let ext = extension_of(input);
    let (ext, format) = normalized_target(&ext);
    let output = unique_output_in(
        input,
        output_dir,
        &format!(" ({}%)", (scale * 100.0).round() as u32),
        ext,
    )?;
    save_image(&resized, &output, format, 92)?;
    Ok(output)
}

fn rotate_file(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let image = image::open(input).map_err(|error| format!("Could not read image: {error}"))?;
    let rotated = image.rotate90();
    let ext = extension_of(input);
    let (ext, format) = normalized_target(&ext);
    let output = unique_output_in(input, output_dir, " (rotated)", ext)?;
    save_image(&rotated, &output, format, 92)?;
    Ok(output)
}

fn flip_file(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let image = image::open(input).map_err(|error| format!("Could not read image: {error}"))?;
    let flipped = image.fliph();
    let ext = extension_of(input);
    let (ext, format) = normalized_target(&ext);
    let output = unique_output_in(input, output_dir, " (flipped)", ext)?;
    save_image(&flipped, &output, format, 92)?;
    Ok(output)
}

fn grayscale_file(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let image = image::open(input).map_err(|error| format!("Could not read image: {error}"))?;
    let grayscale = image.grayscale();
    let ext = extension_of(input);
    let (ext, format) = normalized_target(&ext);
    let output = unique_output_in(input, output_dir, " (bw)", ext)?;
    save_image(&grayscale, &output, format, 92)?;
    Ok(output)
}

fn mute_video(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let ffmpeg = ffmpeg_path()?;
    let ext = extension_of(input);
    let output = unique_output_in(input, output_dir, " (muted)", &ext)?;
    let status = Command::new(ffmpeg)
        .args(["-y", "-i"])
        .arg(input)
        .args(["-c:v", "copy", "-an"])
        .arg(&output)
        .status()
        .map_err(|error| format!("Could not run ffmpeg: {error}"))?;
    if !status.success() || !output.exists() {
        return Err("Could not mute this video.".into());
    }
    Ok(output)
}

fn resize_video_720(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let ffmpeg = ffmpeg_path()?;
    let output = unique_output_in(input, output_dir, " (720p)", "mp4")?;
    let status = Command::new(ffmpeg)
        .args(["-y", "-i"])
        .arg(input)
        .args(["-vf", "scale=-2:720"])
        .args(video_encode_args("26", "3M"))
        .args(["-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"])
        .arg(&output)
        .status()
        .map_err(|error| format!("Could not run ffmpeg: {error}"))?;
    if !status.success() || !output.exists() {
        return Err("Could not resize this video.".into());
    }
    Ok(output)
}

fn frame_video(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let ffmpeg = ffmpeg_path()?;
    let output = unique_output_in(input, output_dir, " (frame)", "png")?;
    let status = Command::new(ffmpeg)
        .args(["-y", "-ss", "0", "-i"])
        .arg(input)
        .args(["-frames:v", "1"])
        .arg(&output)
        .status()
        .map_err(|error| format!("Could not run ffmpeg: {error}"))?;
    if !status.success() || !output.exists() {
        return Err("Could not extract a frame from this video.".into());
    }
    Ok(output)
}

fn mono_audio(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let ffmpeg = ffmpeg_path()?;
    let output = unique_output_in(input, output_dir, " (mono)", "m4a")?;
    let status = Command::new(ffmpeg)
        .args(["-y", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "1", "-c:a", "aac", "-b:a", "128k"])
        .arg(&output)
        .status()
        .map_err(|error| format!("Could not run ffmpeg: {error}"))?;
    if !status.success() || !output.exists() {
        return Err("Could not convert this audio to mono.".into());
    }
    Ok(output)
}

fn directory_of(input: &Path) -> PathBuf {
    input.parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
}

fn crop_file(
    input: &Path,
    rect: &crate::models::NormalizedRect,
    output_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    let image = image::open(input).map_err(|error| format!("Could not read image: {error}"))?;
    let (width, height) = image.dimensions();
    let x = (rect.x.clamp(0.0, 1.0) * width as f64).round() as u32;
    let y = (rect.y.clamp(0.0, 1.0) * height as f64).round() as u32;
    let crop_width = (rect.width.clamp(0.0, 1.0) * width as f64).round() as u32;
    let crop_height = (rect.height.clamp(0.0, 1.0) * height as f64).round() as u32;
    if crop_width < 1 || crop_height < 1 {
        return Err("The selected area is too small.".into());
    }
    let cropped = image.crop_imm(x, y, crop_width.min(width - x), crop_height.min(height - y));
    let output = unique_output_in(input, output_dir, " (cropped)", "png")?;
    cropped
        .save_with_format(&output, ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(output)
}

pub fn open_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("/usr/bin/open");
        command.arg(path);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", path.to_string_lossy().as_ref()]);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };
    command.status().map_err(|error| error.to_string())?;
    Ok(())
}

pub fn reveal_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let status = Command::new("/usr/bin/open")
        .args(["-R", path.to_string_lossy().as_ref()])
        .status();
    #[cfg(target_os = "windows")]
    let status = Command::new("explorer")
        .arg(format!("/select,{}", path.to_string_lossy()))
        .status();
    #[cfg(target_os = "linux")]
    let status = Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .status();
    status.map_err(|error| error.to_string())?;
    Ok(())
}

pub fn perform_action(
    action: &str,
    paths: &[String],
    target: Option<&str>,
    output_dir: Option<&Path>,
) -> Result<ActionOutcome, String> {
    if paths.is_empty() {
        return Err("No files selected.".into());
    }
    if action == "reveal" {
        reveal_path(Path::new(&paths[0]))?;
        return Ok(ActionOutcome {
            outputs: vec![],
            message: "Revealed in file manager.".into(),
        });
    }
    if action == "open" {
        open_path(Path::new(&paths[0]))?;
        return Ok(ActionOutcome {
            outputs: vec![],
            message: "Opened.".into(),
        });
    }

    let pdf_paths: Vec<PathBuf> = paths
        .iter()
        .map(PathBuf::from)
        .filter(|path| extension_of(path) == "pdf")
        .collect();
    if action == "merge" {
        if pdf_paths.len() == paths.len() && pdf_paths.len() > 1 {
            let output = pdf::merge(&pdf_paths, output_dir)?;
            return Ok(ActionOutcome {
                outputs: vec![output.to_string_lossy().to_string()],
                message: "Merged PDFs.".into(),
            });
        }
        let image_paths: Vec<PathBuf> = paths
            .iter()
            .map(PathBuf::from)
            .filter(|path| IMAGE_EXTENSIONS.contains(&extension_of(path).as_str()))
            .collect();
        if image_paths.len() == paths.len() && image_paths.len() > 1 {
            let output = pdf::images_to_pdf(&image_paths, output_dir)?;
            return Ok(ActionOutcome {
                outputs: vec![output.to_string_lossy().to_string()],
                message: "Combined images into a PDF.".into(),
            });
        }
        return Err("Merge works with multiple PDFs or multiple images.".into());
    }
    if action == "split" {
        if pdf_paths.is_empty() {
            return Err("Split works with PDF files.".into());
        }
        let mut outputs = Vec::new();
        for path in &pdf_paths {
            outputs.extend(pdf::split(path, output_dir)?);
        }
        return Ok(ActionOutcome {
            message: format!("Saved {} PDF pages.", outputs.len()),
            outputs: outputs
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
        });
    }
    if action == "text" {
        if pdf_paths.is_empty() {
            return Err("Text extraction currently works with PDF files.".into());
        }
        let mut outputs = Vec::new();
        for path in &pdf_paths {
            outputs.push(pdf::extract_text(path, output_dir)?);
        }
        return Ok(ActionOutcome {
            message: "Extracted PDF text.".into(),
            outputs: outputs
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
        });
    }
    if action == "convert"
        && target
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "tif" | "tiff" | "heic" | "heif"
                )
            })
            .unwrap_or(false)
        && !pdf_paths.is_empty()
    {
        let extension = target.unwrap_or("png");
        let mut outputs = Vec::new();
        for path in &pdf_paths {
            outputs.extend(pdf::to_images(path, extension, output_dir)?);
        }
        return Ok(ActionOutcome {
            message: format!("Saved {} PDF pages.", outputs.len()),
            outputs: outputs
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
        });
    }

    let mut outputs = Vec::new();
    for raw_path in paths {
        let input = Path::new(raw_path);
        if !input.is_file() {
            continue;
        }
        let ext = extension_of(input);
        let media =
            VIDEO_EXTENSIONS.contains(&ext.as_str()) || AUDIO_EXTENSIONS.contains(&ext.as_str());
        let is_pdf = ext == "pdf";
        let is_image = IMAGE_EXTENSIONS.contains(&ext.as_str());
        let is_video = VIDEO_EXTENSIONS.contains(&ext.as_str());
        let is_audio = AUDIO_EXTENSIONS.contains(&ext.as_str());
        let is_document = DOCUMENT_EXTENSIONS.contains(&ext.as_str());
        let is_presentation = PRESENTATION_EXTENSIONS.contains(&ext.as_str());
        let is_spreadsheet = SPREADSHEET_EXTENSIONS.contains(&ext.as_str());
        let output = match action {
            "convert" if is_pdf && matches!(target, Some("txt" | "md")) => {
                pdf::extract_text_to(input, target.unwrap_or("txt"), output_dir)?
            }
            "convert" if is_pdf && matches!(target, Some("docx" | "rtf")) => {
                // The text layer becomes a Markdown intermediate, then the same
                // document writer produces the requested container.
                let text = pdf::text_of(input)?;
                let temporary = std::env::temp_dir()
                    .join(format!("shift-pdf-{}.md", uuid::Uuid::new_v4()));
                fs::write(&temporary, text).map_err(|error| error.to_string())?;
                let target = target.unwrap_or("docx");
                // Write straight into the resolved output folder, then rename away
                // from the throwaway intermediate name.
                let target_dir = output_dir
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| directory_of(input));
                let result = convert_document(&temporary, target, Some(&target_dir))
                    .and_then(|generated| rename_office_output(generated, input, &target_dir));
                let _ = fs::remove_file(&temporary);
                result?
            }
            "convert" if is_pdf => {
                return Err(
                    "Pick a supported target for this PDF: an image format, TXT, MD, DOCX or RTF."
                        .into(),
                )
            }
            "convert"
                if is_image
                    && target
                        .map(|value| value.eq_ignore_ascii_case("pdf"))
                        .unwrap_or(false) =>
            {
                pdf::images_to_pdf(&[input.to_path_buf()], output_dir)?
            }
            "convert" if is_document => {
                convert_document(input, target.unwrap_or("md"), output_dir)?
            }
            "convert" if is_presentation => match target.unwrap_or("md") {
                "md" => office::presentation_to_markdown(input, output_dir)?,
                "pdf" => {
                    let temporary = std::env::temp_dir()
                        .join(format!("shift-presentation-{}", uuid::Uuid::new_v4()));
                    fs::create_dir_all(&temporary).map_err(|error| error.to_string())?;
                    let markdown = office::presentation_to_markdown(input, Some(&temporary))?;
                    let target_dir = output_dir.map(Path::to_path_buf).unwrap_or_else(|| {
                        input
                            .parent()
                            .unwrap_or_else(|| Path::new("."))
                            .to_path_buf()
                    });
                    let result = convert_document(&markdown, "pdf", Some(&target_dir))
                        .and_then(|generated| rename_office_output(generated, input, &target_dir));
                    let _ = fs::remove_dir_all(&temporary);
                    result?
                }
                other => {
                    return Err(format!(
                        "Presentations convert to Markdown or PDF, not {other}."
                    ))
                }
            },
            "convert" if is_spreadsheet => match target.unwrap_or("md") {
                "md" => office::spreadsheet_to_markdown(input, output_dir)?,
                "pdf" => {
                    let temporary = std::env::temp_dir()
                        .join(format!("shift-spreadsheet-{}", uuid::Uuid::new_v4()));
                    fs::create_dir_all(&temporary).map_err(|error| error.to_string())?;
                    let markdown = office::spreadsheet_to_markdown(input, Some(&temporary))?;
                    let target_dir = output_dir.map(Path::to_path_buf).unwrap_or_else(|| {
                        input
                            .parent()
                            .unwrap_or_else(|| Path::new("."))
                            .to_path_buf()
                    });
                    let result = convert_document(&markdown, "pdf", Some(&target_dir))
                        .and_then(|generated| rename_office_output(generated, input, &target_dir));
                    let _ = fs::remove_dir_all(&temporary);
                    result?
                }
                other => {
                    return Err(format!(
                        "Spreadsheets convert to Markdown or PDF, not {other}."
                    ))
                }
            },
            "convert" if media => convert_media(
                input,
                target.ok_or_else(|| "No target format selected.".to_string())?,
                output_dir,
            )?,
            "convert" => convert_file(
                input,
                target.ok_or_else(|| "No target format selected.".to_string())?,
                output_dir,
            )?,
            "compress" if media => compress_media(input, output_dir)?,
            "compress" if is_pdf => pdf::compress(input, output_dir)?,
            "compress" => compress_file(input, output_dir)?,
            "stripMetadata" if is_pdf => pdf::strip_metadata(input, output_dir)?,
            "stripMetadata" => strip_metadata(input, output_dir)?,
            "resizeHalf" => resize_file(input, 0.5, output_dir)?,
            "rotate" if is_pdf => pdf::rotate(input, 90, output_dir)?,
            "rotate" => rotate_file(input, output_dir)?,
            "flip" => flip_file(input, output_dir)?,
            "grayscale" => grayscale_file(input, output_dir)?,
            "mute" if is_video => mute_video(input, output_dir)?,
            "resize720" if is_video => resize_video_720(input, output_dir)?,
            "frame" if is_video => frame_video(input, output_dir)?,
            "mono" if is_audio => mono_audio(input, output_dir)?,
            "crop" => {
                let rect = crate::models::NormalizedRect {
                    x: 0.08,
                    y: 0.08,
                    width: 0.84,
                    height: 0.84,
                };
                crop_file(input, &rect, output_dir)?
            }
            other => return Err(format!("Unsupported action: {other}")),
        };
        outputs.push(output.to_string_lossy().to_string());
    }
    Ok(ActionOutcome {
        message: if outputs.len() == 1 {
            "Saved 1 file.".into()
        } else {
            format!("Saved {} files.", outputs.len())
        },
        outputs,
    })
}

pub fn editor_kind(path: &Path) -> &'static str {
    let ext = extension_of(path);
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        "image"
    } else if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        "video"
    } else if matches!(ext.as_str(), "txt" | "md" | "csv" | "tsv" | "html" | "htm") {
        "text"
    } else {
        "file"
    }
}

#[tauri::command]
pub fn inspect_files(paths: Vec<String>) -> Result<Vec<FileInfo>, String> {
    inspect_paths(&paths)
}

#[tauri::command]
pub fn perform_file_action(
    app: AppHandle,
    action: String,
    paths: Vec<String>,
    target: Option<String>,
) -> Result<ActionOutcome, String> {
    let output_dir = settings::files_output_dir(&app)?;
    let outcome = perform_action(&action, &paths, target.as_deref(), output_dir.as_deref())?;
    if settings::auto_reveal(&app) {
        if let Some(first) = outcome.outputs.first() {
            let path = Path::new(first);
            if path.exists() {
                let _ = reveal_path(path);
            }
        }
    }
    let _ = app.emit("app://file-result", outcome.clone());
    Ok(outcome)
}

#[tauri::command]
pub fn reveal_file(path: String) -> Result<(), String> {
    reveal_path(Path::new(&path))
}

#[tauri::command]
pub fn existing_files(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .filter(|path| Path::new(path).is_file())
        .collect()
}

#[tauri::command]
pub fn open_file(path: String) -> Result<(), String> {
    open_path(Path::new(&path))
}

#[tauri::command]
pub fn read_file_data_url(path: String) -> Result<String, String> {
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let ext = extension_of(Path::new(&path));
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    };
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

#[tauri::command]
pub fn read_text_file(path: String) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_image_data(
    app: AppHandle,
    source: String,
    data_url: String,
    suffix: Option<String>,
) -> Result<String, String> {
    let output_dir = settings::files_output_dir(&app)?;
    save_image_data_in(&source, &data_url, suffix.as_deref(), output_dir.as_deref())
}

fn save_image_data_in(
    source: &str,
    data_url: &str,
    suffix: Option<&str>,
    output_dir: Option<&Path>,
) -> Result<String, String> {
    let encoded = data_url
        .split_once(',')
        .map(|(_, value)| value)
        .ok_or_else(|| "Invalid image data.".to_string())?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|error| format!("Invalid image data: {error}"))?;
    let image = image::load_from_memory(&bytes)
        .map_err(|error| format!("Could not decode edited image: {error}"))?;
    let input = Path::new(source);
    let output = unique_output_in(input, output_dir, suffix.unwrap_or(" (annotated)"), "png")?;
    image
        .save_with_format(&output, ImageFormat::Png)
        .map_err(|error| format!("Could not write {}: {error}", output.display()))?;
    Ok(output.to_string_lossy().to_string())
}

#[tauri::command]
pub fn render_pdf_pages(app: AppHandle, path: String) -> Result<Vec<String>, String> {
    let folder = app_cache_dir(&app)?.join(format!(
        "ocr-{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")
    ));
    pdf::to_images_in(Path::new(&path), "png", &folder).map(|pages| {
        pages
            .into_iter()
            .map(|page| page.to_string_lossy().to_string())
            .collect()
    })
}

#[tauri::command]
pub fn save_text_near(
    app: AppHandle,
    source: String,
    text: String,
    suffix: Option<String>,
) -> Result<String, String> {
    let output_dir = settings::files_output_dir(&app)?;
    save_text_near_in(&source, &text, suffix.as_deref(), output_dir.as_deref())
}

fn save_text_near_in(
    source: &str,
    text: &str,
    suffix: Option<&str>,
    output_dir: Option<&Path>,
) -> Result<String, String> {
    let input = Path::new(source);
    let output = unique_output_in(input, output_dir, suffix.unwrap_or(" (OCR)"), "txt")?;
    fs::write(&output, text).map_err(|error| describe_write_error(&output, error))?;
    Ok(output.to_string_lossy().to_string())
}

#[allow(dead_code)]
pub fn app_cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join("captures");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn scratch_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("shift-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn sample_image(path: &Path) {
        let image = RgbaImage::from_pixel(32, 20, Rgba([240, 100, 40, 255]));
        image.save_with_format(path, ImageFormat::Png).unwrap();
    }

    #[test]
    fn converts_and_avoids_overwriting() {
        let dir = scratch_dir();
        let input = dir.join("sample.png");
        sample_image(&input);

        let first = convert_file(&input, "jpg", None).unwrap();
        let second = convert_file(&input, "jpg", None).unwrap();
        assert!(first.exists());
        assert!(second.exists());
        assert_ne!(first, second);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn custom_output_directory_collects_results() {
        let dir = scratch_dir();
        let output_dir = dir.join("Shift");
        let input = dir.join("sample.png");
        sample_image(&input);

        let outcome = perform_action(
            "convert",
            &[input.to_string_lossy().to_string()],
            Some("jpg"),
            Some(&output_dir),
        )
        .unwrap();

        assert_eq!(outcome.outputs.len(), 1);
        assert!(Path::new(&outcome.outputs[0]).exists());
        assert_eq!(
            Path::new(&outcome.outputs[0]).parent(),
            Some(output_dir.as_path())
        );
        assert!(input.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn text_results_follow_the_custom_directory() {
        let dir = scratch_dir();
        let output_dir = dir.join("Shift");
        let input = dir.join("sample.png");
        sample_image(&input);

        let output = save_text_near_in(
            input.to_string_lossy().as_ref(),
            "hello",
            Some(" (OCR)"),
            Some(&output_dir),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&output).unwrap(), "hello");
        assert_eq!(Path::new(&output).parent(), Some(output_dir.as_path()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saves_edited_image_data() {
        let dir = scratch_dir();
        let input = dir.join("shot.png");
        sample_image(&input);
        let data = fs::read(&input).unwrap();
        let data_url = format!("data:image/png;base64,{}", STANDARD.encode(data));
        let output = save_image_data_in(
            input.to_string_lossy().as_ref(),
            &data_url,
            Some(" (annotated)"),
            None,
        )
        .unwrap();
        assert!(Path::new(&output).exists());
        assert!(output.contains("(annotated)"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn edits_keep_the_source_untouched() {
        let dir = scratch_dir();
        let input = dir.join("sample.png");
        sample_image(&input);
        let original = fs::read(&input).unwrap();

        let compressed = compress_file(&input, None).unwrap();
        let resized = resize_file(&input, 0.5, None).unwrap();
        let rotated = rotate_file(&input, None).unwrap();
        let flipped = flip_file(&input, None).unwrap();
        let grayscale = grayscale_file(&input, None).unwrap();
        let cropped = crop_file(
            &input,
            &crate::models::NormalizedRect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            None,
        )
        .unwrap();

        assert!(compressed.exists());
        assert_eq!(image::image_dimensions(resized).unwrap(), (16, 10));
        assert_eq!(image::image_dimensions(rotated).unwrap(), (20, 32));
        assert!(flipped.exists());
        assert!(grayscale.exists());
        assert_eq!(image::image_dimensions(cropped).unwrap(), (16, 10));
        assert_eq!(fs::read(&input).unwrap(), original);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn converts_media_when_ffmpeg_is_available() {
        if which::which("ffmpeg").is_err() {
            return;
        }
        let dir = scratch_dir();
        let input = dir.join("sample.mp4");
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=10",
                "-t",
                "1",
                "-pix_fmt",
                "yuv420p",
                input.to_string_lossy().as_ref(),
            ])
            .status()
            .unwrap();
        assert!(status.success());

        let gif = convert_media(&input, "gif", None).unwrap();
        let compressed = compress_media(&input, None).unwrap();
        let muted = mute_video(&input, None).unwrap();
        let hd = resize_video_720(&input, None).unwrap();
        let frame = frame_video(&input, None).unwrap();
        assert!(gif.exists());
        assert!(compressed.exists());
        assert!(muted.exists());
        assert!(hd.exists());
        assert!(frame.exists());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn converts_audio_to_mp3_when_an_encoder_is_available() {
        if which::which("ffmpeg").is_err() {
            return;
        }
        if !ffmpeg_has_encoder("libmp3lame") && which::which("lame").is_err() {
            return;
        }
        let dir = scratch_dir();
        let input = dir.join("sample.m4a");
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:a",
                "aac",
                input.to_string_lossy().as_ref(),
            ])
            .status()
            .unwrap();
        assert!(status.success());

        let output = convert_media(&input, "mp3", None).unwrap();
        let mono = mono_audio(&input, None).unwrap();
        assert!(output.exists());
        assert!(mono.exists());
        assert_eq!(
            output.extension().and_then(|value| value.to_str()),
            Some("mp3")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn converts_documents_when_pandoc_is_available() {
        if which::which("pandoc").is_err() {
            return;
        }
        let dir = scratch_dir();
        let input = dir.join("sample.html");
        fs::write(
            &input,
            "<h1>Shift</h1><p>Local <strong>documents</strong>.</p>",
        )
        .unwrap();
        let output = convert_document(&input, "md", None).unwrap();
        let text = fs::read_to_string(output).unwrap();
        assert!(text.contains("# Shift"));
        assert!(text.contains("**documents**"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renders_documents_to_pdf_with_native_helper() {
        let dir = scratch_dir();
        let input = dir.join("sample.html");
        fs::write(&input, "<h1>Shift</h1><p>Native PDF.</p>").unwrap();
        let output = convert_document(&input, "pdf", None).unwrap();
        assert!(output.exists());
        assert_eq!(
            output.extension().and_then(|value| value.to_str()),
            Some("pdf")
        );
        assert!(fs::read(&output).unwrap().starts_with(b"%PDF"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renders_presentations_to_pdf() {
        use std::io::Write as _;
        use zip::{write::SimpleFileOptions, ZipWriter};

        let dir = scratch_dir();
        let input = dir.join("slides.pptx");
        let file = fs::File::create(&input).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("ppt/slides/slide1.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>Hello</a:t></a:r></a:p></p:sld>"#,
        )
        .unwrap();
        zip.finish().unwrap();

        let outcome = perform_action(
            "convert",
            &[input.to_string_lossy().to_string()],
            Some("pdf"),
            Some(&dir),
        )
        .unwrap();
        assert_eq!(outcome.outputs.len(), 1);
        assert!(outcome.outputs[0].ends_with("slides.pdf"));
        assert!(fs::read(&outcome.outputs[0]).unwrap().starts_with(b"%PDF"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renders_spreadsheets_to_pdf() {
        let dir = scratch_dir();
        let input = dir.join("sheet.csv");
        fs::write(&input, "name,value\nAlpha,1\n").unwrap();

        let outcome = perform_action(
            "convert",
            &[input.to_string_lossy().to_string()],
            Some("pdf"),
            Some(&dir),
        )
        .unwrap();
        assert_eq!(outcome.outputs.len(), 1);
        assert!(outcome.outputs[0].ends_with("sheet.pdf"));
        assert!(fs::read(&outcome.outputs[0]).unwrap().starts_with(b"%PDF"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn converts_pdf_text_layer_to_docx_when_pandoc_is_available() {
        if which::which("pandoc").is_err() {
            return;
        }
        let dir = scratch_dir();
        let input = dir.join("report.pdf");
        write_text_pdf(&input, "Quarterly numbers");

        let outcome = perform_action(
            "convert",
            &[input.to_string_lossy().to_string()],
            Some("docx"),
            None,
        )
        .expect("a PDF text layer should convert to DOCX");
        assert_eq!(outcome.outputs.len(), 1);
        assert!(
            outcome.outputs[0].ends_with("report.docx"),
            "output must follow the requested format, not .pdf: {:?}",
            outcome.outputs[0]
        );
        assert!(Path::new(&outcome.outputs[0]).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pdf_docx_conversion_honours_a_custom_output_folder() {
        if which::which("pandoc").is_err() {
            return;
        }
        let dir = scratch_dir();
        let output_dir = dir.join("Shift");
        let input = dir.join("nested").join("report.pdf");
        fs::create_dir_all(input.parent().unwrap()).unwrap();
        write_text_pdf(&input, "Budget details");

        let outcome = perform_action(
            "convert",
            &[input.to_string_lossy().to_string()],
            Some("docx"),
            Some(&output_dir),
        )
        .unwrap();
        let produced = PathBuf::from(&outcome.outputs[0]);
        assert_eq!(
            produced.parent(),
            Some(output_dir.as_path()),
            "a custom output folder must win over the source folder: {produced:?}"
        );
        assert!(produced.to_string_lossy().contains("report"), "{produced:?}");
        assert_eq!(produced.extension().and_then(|v| v.to_str()), Some("docx"));
        // Nothing should leak next to the original.
        assert!(!dir.join("nested").join("report.docx").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    /// A one-page PDF with a real text layer, built with the same crate the
    /// production code reads through.
    fn write_text_pdf(path: &Path, text: &str) {
        use lopdf::{dictionary, Document, Object, Stream};
        let mut document = Document::new();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let content = format!("BT /F1 18 Tf 72 720 Td ({text}) Tj ET");
        let content_id = document.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let pages_id = document.add_object(dictionary! { "Type" => "Pages", "Kids" => Vec::<Object>::new(), "Count" => 0_i64 });
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        });
        document.objects.insert(
            pages_id,
            dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1_i64 }
                .into(),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog);
        document.save(path).unwrap();
    }

    #[test]
    fn converts_a_docx_back_to_markdown_when_pandoc_is_available() {
        if which::which("pandoc").is_err() {
            return;
        }
        let dir = scratch_dir();
        let source = dir.join("sample.md");
        let docx = dir.join("sample.docx");
        fs::write(&source, "# Shift\n\nDocument round trip.\n").unwrap();
        let status = Command::new("pandoc")
            .arg(&source)
            .args(["-o"])
            .arg(&docx)
            .status()
            .unwrap();
        assert!(status.success());

        let output = convert_document(&docx, "md", None).unwrap();
        let text = fs::read_to_string(output).unwrap();
        assert!(text.contains("Shift"));
        assert!(text.contains("Document round trip"));
        fs::remove_dir_all(dir).unwrap();
    }

}
