use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{BufReader, Cursor, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::UNIX_EPOCH,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{
    codecs::{gif::GifDecoder, jpeg::JpegEncoder},
    AnimationDecoder, Delay, DynamicImage, Frame, GenericImageView, ImageDecoder, ImageFormat,
    RgbImage, RgbaImage,
};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::{ActionOutcome, ActionResultEvent, FileActionProgress, FileInfo},
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

fn image_mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "tiff" | "tif" => "image/tiff",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "heic" | "heif" => "image/heic",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
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
    let status = crate::process::command("/usr/bin/sips")
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
            "This Mac cannot write {} files. Try PNG or JPG.",
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
    let status = crate::process::command(magick)
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
    let status = crate::process::command(magick)
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

/// Open an image for the pixel tools.
///
/// HEIC, HEIF and AVIF are readable only through the system codec, so every pixel
/// tool used to reject a phone screenshot that the convert wedge could read without
/// trouble. Decoding through the same path the convert wedge already uses keeps the
/// rejection for files no decoder can read at all.
fn open_for_pixels(input: &Path) -> Result<DynamicImage, String> {
    if let Ok(image) = image::open(input) {
        return Ok(image);
    }
    let intermediate =
        std::env::temp_dir().join(format!("shift-decode-{}.png", uuid::Uuid::new_v4()));
    let decoded = system_convert(input, &intermediate, "png")
        .ok()
        .and_then(|()| image::open(&intermediate).ok());
    let _ = fs::remove_file(&intermediate);
    decoded.ok_or_else(|| {
        format!(
            "Could not read {}: an unsupported or corrupted image.",
            input.display()
        )
    })
}

fn convert_file(input: &Path, target: &str, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let (ext, format) = normalized_target(target);
    let output = unique_output_in(input, output_dir, "", ext)?;
    let direct_formats = ["png", "jpg", "webp", "tiff", "bmp", "gif"];

    if direct_formats.contains(&ext) {
        if let Ok(image) = image::open(input) {
            if ext == "gif" {
                save_gif_capped(input, &output, GIF_MAX_BYTES)?;
            } else {
                save_image(&image, &output, format, 92)?;
            }
            return Ok(output);
        }
        if ext == "webp" {
            // HEIC/HEIF/AVIF are readable only through the system codec, while
            // macOS sips cannot encode WEBP. Decode to PNG, then use the native
            // WEBP encoder.
            let intermediate =
                std::env::temp_dir().join(format!("shift-image-{}.png", uuid::Uuid::new_v4()));
            if system_convert(input, &intermediate, "png").is_ok() {
                let converted = image::open(&intermediate).ok();
                let _ = fs::remove_file(intermediate);
                if let Some(image) = converted {
                    save_image(&image, &output, format, 92)?;
                    return Ok(output);
                }
            } else {
                let _ = fs::remove_file(intermediate);
            }
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
    {
        let directories = [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/opt/local/bin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
        ];
        directories
            .into_iter()
            .map(|directory| Path::new(directory).join(name))
            .find(|candidate| candidate.is_file())
    }
    #[cfg(target_os = "windows")]
    {
        find_windows_tool(name)
    }
    #[cfg(target_os = "linux")]
    {
        let directories = ["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"];
        directories
            .into_iter()
            .map(|directory| Path::new(directory).join(name))
            .find(|candidate| candidate.is_file())
    }
}

pub(crate) fn document_pdf_engine() -> Option<PathBuf> {
    [
        "xelatex",
        "lualatex",
        "pdflatex",
        "tectonic",
        "typst",
        "weasyprint",
        "wkhtmltopdf",
    ]
    .into_iter()
    .find_map(resolve_tool)
}

#[cfg(target_os = "windows")]
fn windows_tool_file_names(name: &str) -> Vec<String> {
    let mut names = vec![name.to_string()];
    if Path::new(name).extension().is_none() {
        names.push(format!("{name}.exe"));
    }
    names
}

#[cfg(target_os = "windows")]
fn push_windows_environment_directory(
    directories: &mut Vec<PathBuf>,
    variable: &str,
    suffix: &str,
) {
    if let Some(root) = std::env::var_os(variable) {
        directories.push(PathBuf::from(root).join(suffix));
    }
}

#[cfg(target_os = "windows")]
fn windows_tool_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for (variable, suffix) in [
        ("LOCALAPPDATA", r"Microsoft\WinGet\Links"),
        ("LOCALAPPDATA", r"Microsoft\WindowsApps"),
        ("LOCALAPPDATA", "Pandoc"),
        ("LOCALAPPDATA", r"Programs\Pandoc"),
        ("ProgramFiles", "Pandoc"),
        ("ProgramW6432", "Pandoc"),
        ("ProgramFiles", r"ffmpeg\bin"),
        ("ProgramW6432", r"ffmpeg\bin"),
        ("ProgramFiles", r"Gyan\FFmpeg\bin"),
        ("ProgramW6432", r"Gyan\FFmpeg\bin"),
        ("ProgramFiles", r"FFmpeg\bin"),
        ("ProgramW6432", r"FFmpeg\bin"),
        ("ProgramData", r"chocolatey\bin"),
        ("USERPROFILE", r"scoop\shims"),
    ] {
        push_windows_environment_directory(&mut directories, variable, suffix);
    }

    // ImageMagick installs into a versioned folder under Program Files.
    for variable in ["ProgramFiles", "ProgramW6432"] {
        let Some(root) = std::env::var_os(variable) else {
            continue;
        };
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("imagemagick-")
            {
                directories.push(path);
            }
        }
    }
    directories
}

#[cfg(target_os = "windows")]
fn windows_winget_prefixes(name: &str) -> &'static [&'static str] {
    match name {
        "pandoc" => &["johnmacfarlane.pandoc", "chocolatey.pandoc"],
        "ffmpeg" => &["gyan.ffmpeg"],
        "pdftoppm" | "pdfimages" | "pdfinfo" => &["oschwartz10612.poppler"],
        "magick" => &["imagemagick.imagemagick"],
        "typst" => &["typst.typst"],
        "winget" => &[],
        _ => &[],
    }
}

#[cfg(target_os = "windows")]
fn find_file_below(
    directory: &Path,
    file_names: &[String],
    remaining_depth: usize,
) -> Option<PathBuf> {
    if remaining_depth == 0 {
        return None;
    }
    let entries = fs::read_dir(directory).ok()?;
    let mut directories = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            directories.push(path);
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_names
            .iter()
            .any(|candidate| file_name.eq_ignore_ascii_case(candidate))
        {
            return Some(path);
        }
    }
    directories.into_iter().find_map(|directory| {
        find_file_below(&directory, file_names, remaining_depth.saturating_sub(1))
    })
}

#[cfg(target_os = "windows")]
fn find_winget_package_tool(name: &str, file_names: &[String]) -> Option<PathBuf> {
    let prefixes = windows_winget_prefixes(name);
    if prefixes.is_empty() {
        return None;
    }
    let packages =
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join(r"Microsoft\WinGet\Packages");
    let entries = fs::read_dir(packages).ok()?;
    for entry in entries.flatten() {
        let package_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !entry.path().is_dir()
            || !prefixes
                .iter()
                .any(|prefix| package_name.starts_with(prefix))
        {
            continue;
        }
        if let Some(found) = find_file_below(&entry.path(), file_names, 6) {
            return Some(found);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn find_windows_tool(name: &str) -> Option<PathBuf> {
    static FOUND: OnceLock<std::sync::Mutex<std::collections::HashMap<String, PathBuf>>> =
        OnceLock::new();
    let cache = FOUND.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Some(path) = cache.lock().ok().and_then(|tools| tools.get(name).cloned()) {
        return Some(path);
    }
    let found = find_windows_tool_uncached(name);
    if let Some(path) = found.as_ref() {
        if let Ok(mut tools) = cache.lock() {
            tools.insert(name.to_string(), path.clone());
        }
    }
    found
}

#[cfg(target_os = "windows")]
fn find_windows_tool_uncached(name: &str) -> Option<PathBuf> {
    let file_names = windows_tool_file_names(name);
    for directory in windows_tool_directories() {
        for file_name in &file_names {
            let candidate = directory.join(file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    find_winget_package_tool(name, &file_names)
}

/// Runs long media and document work below the UI's scheduling priority on
/// macOS, so a CPU-heavy encoder cannot starve the always-on-top radial window.
pub(crate) fn background_command(program: impl AsRef<OsStr>) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = crate::process::command("/usr/bin/nice");
        command.args(["-n", "10"]).arg(program);
        command
    }
    #[cfg(target_os = "windows")]
    {
        let mut command = crate::process::command(program);
        crate::process::background(&mut command);
        command
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        crate::process::command(program)
    }
}

fn ffmpeg_path() -> Result<PathBuf, String> {
    resolve_tool("ffmpeg").ok_or_else(|| {
        "This conversion needs ffmpeg. Install it from Settings > Runtime Components or add it to your PATH."
            .to_string()
    })
}

fn ffmpeg_has_encoder(name: &str) -> bool {
    let Ok(ffmpeg) = ffmpeg_path() else {
        return false;
    };
    let Ok(output) = crate::process::output(ffmpeg, &["-hide_banner", "-encoders"]) else {
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
/// Hard ceiling on the byte size of any GIF Shift writes.
const GIF_MAX_BYTES: u64 = 8 * 1024 * 1024;
/// The same ceiling for the ffmpeg path, passed as `-fs`.
const GIF_MAX_BYTES_FFMPEG: &str = "8388608";
/// Longest animation kept when re-encoding an existing animated GIF.
const GIF_MAX_MILLIS: u32 = 15_000;
/// Most frames kept when re-encoding an existing animated GIF.
const GIF_MAX_FRAMES: usize = 180;
/// Widest dimension kept when re-encoding an existing animated GIF.
const GIF_MAX_SIDE: u32 = 480;

/// Writes a GIF that respects the size ceiling. An animated source keeps its
/// frames; the running duration, the frame count and the resulting byte size
/// are all capped so a long recording cannot become a huge file.
fn save_gif_capped(input: &Path, output: &Path, max_bytes: u64) -> Result<(), String> {
    let (width, height, frames) = match gif_frames(input) {
        Ok(frames) => frames,
        // Not an animated GIF: a still image becomes a single-frame GIF.
        Err(_) => {
            let image =
                image::open(input).map_err(|error| format!("Could not read image: {error}"))?;
            let rgba = image.to_rgba8();
            let (width, height) = scaled_size(rgba.width(), rgba.height());
            (
                width,
                height,
                vec![Frame::from_parts(
                    rgba,
                    0,
                    0,
                    Delay::from_numer_denom_ms(0, 1),
                )],
            )
        }
    };
    let buffer = encode_gif_frames(&frames, width, height, max_bytes)?;
    let mut file = File::create(output).map_err(|error| describe_write_error(output, error))?;
    file.write_all(&buffer)
        .map_err(|error| describe_write_error(output, error))?;
    Ok(())
}

/// Encodes as many frames as fit into the byte ceiling. Frame count is found
/// by bisection because a frame's compressed size is only known once written.
fn encode_gif_frames(
    frames: &[Frame],
    width: u32,
    height: u32,
    max_bytes: u64,
) -> Result<Vec<u8>, String> {
    let encode = |count: usize| -> Result<Vec<u8>, String> {
        let mut buffer: Vec<u8> = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut buffer);
            for frame in frames.iter().take(count) {
                let resized = resize_rgba(frame.buffer(), width, height);
                encoder
                    .encode_frame(Frame::from_parts(resized, 0, 0, frame.delay()))
                    .map_err(|error| format!("Could not write the GIF: {error}"))?;
            }
        }
        Ok(buffer)
    };

    let full = encode(frames.len())?;
    if full.len() as u64 <= max_bytes {
        return Ok(full);
    }
    let (mut low, mut high) = (1_usize, frames.len() - 1);
    let mut best = encode(1)?;
    while low <= high {
        let middle = (low + high) / 2;
        let candidate = encode(middle)?;
        if candidate.len() as u64 <= max_bytes {
            best = candidate;
            low = middle + 1;
        } else {
            high = middle.saturating_sub(1);
        }
    }
    Ok(best)
}

/// Decodes a GIF into frames, capped by duration and frame count. The size of
/// the source decides the output size: still images keep their dimensions,
/// animations are scaled down to the GIF width ceiling.
fn gif_frames(input: &Path) -> Result<(u32, u32, Vec<Frame>), String> {
    let file = File::open(input).map_err(|error| format!("Could not read image: {error}"))?;
    let decoder = GifDecoder::new(BufReader::new(file))
        .map_err(|error| format!("Could not read image: {error}"))?;
    let (width, height) = decoder.dimensions();
    let mut frames = Vec::new();
    let mut elapsed = 0_u32;
    for frame in decoder.into_frames() {
        let frame = frame.map_err(|error| format!("Could not read image: {error}"))?;
        let delay = frame.delay().numer_denom_ms().0.min(GIF_MAX_MILLIS);
        elapsed = elapsed.saturating_add(delay);
        if elapsed > GIF_MAX_MILLIS || frames.len() >= GIF_MAX_FRAMES {
            break;
        }
        frames.push(Frame::from_parts(
            frame.into_buffer(),
            0,
            0,
            Delay::from_numer_denom_ms(delay, 1),
        ));
    }
    if frames.is_empty() {
        return Err("Could not read the image frames.".into());
    }
    let (width, height) = scaled_size(width, height);
    Ok((width, height, frames))
}

/// Scales a frame pair down to the GIF width ceiling.
fn scaled_size(width: u32, height: u32) -> (u32, u32) {
    if width <= GIF_MAX_SIDE {
        return (width.max(1), height.max(1));
    }
    let height = (height as f64 * GIF_MAX_SIDE as f64 / width as f64)
        .round()
        .max(1.0) as u32;
    (GIF_MAX_SIDE, height)
}

/// Resizes a frame to the given size with a smooth filter.
fn resize_rgba(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if image.width() == width && image.height() == height {
        return image.clone();
    }
    image::imageops::resize(image, width, height, image::imageops::FilterType::Lanczos3)
}

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
    let mut command = background_command(&ffmpeg);
    command.args(["-y", "-i", input.to_string_lossy().as_ref()]);
    match target {
        "gif" => {
            // Cap duration and width: without these a minute of screen recording
            // turns into a hundreds-of-megabytes animated file.
            command.args(["-t", GIF_MAX_SECONDS]);
            command.args(["-vf", GIF_FILTER, "-loop", "0"]);
            // Cap the byte size too: ffmpeg stops writing once the file is
            // this big, so a busy screen cannot produce a runaway GIF.
            command.args(["-fs", GIF_MAX_BYTES_FFMPEG]);
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
    let status = background_command(&ffmpeg)
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
    let status = background_command(&lame)
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
    let mut command = background_command(&ffmpeg);
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
pub(crate) fn doc_render_bridge() -> Option<PathBuf> {
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
fn quicklook_preview_html(work: &Path) -> Option<PathBuf> {
    fs::read_dir(work)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("Preview.html"))
        .find(|candidate| candidate.is_file())
}

/// Uses the same Quick Look preview Word documents show in Finder as the print
/// source. AppKit's OOXML reader keeps only the body, so headers, footers and
/// other page furniture disappear; the Quick Look HTML includes them.
#[cfg(target_os = "macos")]
fn docx_pdf_via_quicklook(
    input: &Path,
    renderer: &Path,
    output_dir: Option<&Path>,
) -> Option<PathBuf> {
    let work = std::env::temp_dir().join(format!("shift-docx-quicklook-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&work).ok()?;
    let result = (|| {
        let input_arg = input.to_string_lossy();
        let work_arg = work.to_string_lossy();
        let status = crate::process::status_with_timeout(
            "/usr/bin/qlmanage",
            &["-p", input_arg.as_ref(), "-o", work_arg.as_ref()],
            std::time::Duration::from_secs(30),
        )
        .ok()?;
        if !status.success() {
            return None;
        }
        let preview = quicklook_preview_html(&work)?;
        let generated = work.join("preview.pdf");
        let status = background_command(renderer)
            .arg(&preview)
            .arg(&generated)
            .arg(preview.parent()?)
            .arg("--webkit")
            .status()
            .ok()?;
        if !status.success() || !generated.exists() {
            return None;
        }
        let target_dir = output_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| directory_of(input));
        rename_office_output(generated, input, &target_dir).ok()
    })();
    let _ = fs::remove_dir_all(work);
    result
}

#[cfg(target_os = "macos")]
fn native_document_pdf(
    input: &Path,
    output_dir: Option<&Path>,
    on_progress: Option<&(dyn Fn(u32, u32, bool) + Sync)>,
) -> Result<Option<PathBuf>, String> {
    if extension_of(input) == "docx" {
        let work = std::env::temp_dir().join(format!("shift-docx-pdf-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&work).map_err(|error| error.to_string())?;
        let target_dir = output_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| directory_of(input));
        let result = (|| {
            let Some(images) = office::document_page_images(input, &work)? else {
                return Ok(None);
            };
            let generated =
                pdf::images_to_pdf_with_progress(&images, Some(&work), |done, total| {
                    if let Some(report) = on_progress {
                        report(done, total, false);
                    }
                })?;
            rename_office_output(generated, input, &target_dir).map(Some)
        })();
        let _ = fs::remove_dir_all(work);
        if let Some(output) = result? {
            return Ok(Some(output));
        }
    }
    let Some(renderer) = doc_render_bridge() else {
        return Ok(None);
    };
    let extension = extension_of(input);
    if extension == "docx" {
        if let Some(output) = docx_pdf_via_quicklook(input, &renderer, output_dir) {
            return Ok(Some(output));
        }
    }
    let mut source = input.to_path_buf();
    let mut temporary = None;
    // A DOCX with pictures cannot reach the AppKit reader: it silently drops
    // every inline drawing, so the PDF keeps the text and loses the figures.
    // Pandoc reads the same file as HTML with the media extracted next to it.
    let mut work_dir = None;
    if extension == "docx" && office::contains_inline_images(input).unwrap_or(false) {
        let work = std::env::temp_dir().join(format!("shift-docx-html-{}", uuid::Uuid::new_v4()));
        if fs::create_dir_all(&work).is_ok() {
            if let Some(html) = docx_html_via_pandoc(input, &work) {
                source = html;
                work_dir = Some(work);
            } else {
                let _ = fs::remove_dir_all(work);
            }
        }
    }
    let mut base_dir = input
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    if let Some(work) = work_dir.as_ref() {
        base_dir = work.clone();
    }
    if matches!(extension.as_str(), "md" | "markdown") {
        // Markdown must never reach the renderer as plain text: without that
        // step the PDF is a printout of the markup. Pandoc is nicer when it is
        // installed, and the built-in writer covers every other machine.
        let path = std::env::temp_dir().join(format!("shift-doc-{}.html", uuid::Uuid::new_v4()));
        let pandoc_rendered = resolve_tool("pandoc").is_some_and(|pandoc| {
            background_command(&pandoc)
                .arg(input)
                .args(["-s", "-t", "html", "-o"])
                .arg(&path)
                .status()
                .map(|status| status.success() && path.exists())
                .unwrap_or(false)
        });
        let rendered = pandoc_rendered
            || match fs::read_to_string(input) {
                Ok(text) => fs::write(&path, crate::markdown::to_html(&text)).is_ok(),
                Err(_) => false,
            };
        if !rendered {
            let _ = fs::remove_file(&path);
            return Err(format!(
                "Could not render {} as HTML before creating the PDF.",
                input.display()
            ));
        }
        source = path.clone();
        temporary = Some(path);
        // Keep the original folder as the HTML base URL so relative images
        // resolve even though the generated HTML lives in a temp directory.
        base_dir = input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
    }
    let output = unique_output_in(input, output_dir, "", "pdf")?;
    let rendered = background_command(&renderer)
        .arg(&source)
        .arg(&output)
        .arg(&base_dir)
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|error| format!("Could not start the document renderer: {error}"))?;
    let status = rendered.status;
    let renderer_error = String::from_utf8_lossy(&rendered.stderr);
    let renderer_error = renderer_error.trim();
    if let Some(path) = temporary {
        let _ = fs::remove_file(path);
    }
    if let Some(directory) = work_dir {
        let _ = fs::remove_dir_all(directory);
    }
    if !status.success() || !output.exists() {
        let _ = fs::remove_file(&output);
        if !renderer_error.is_empty() {
            return Err(format!(
                "Could not render this document to PDF: {renderer_error}"
            ));
        }
        return Err(format!(
            "Could not render this document to PDF. Renderer exited with {status}."
        ));
    }
    Ok(Some(output))
}

/// Renders a DOCX into the HTML document the macOS printer prints from.
///
/// Pandoc's reader keeps the pictures, tables and heading levels; the body is
/// wrapped in the stylesheet the Markdown path already uses, and the media
/// stays as files in `work` rather than base64 inside the HTML, which would
/// bloat a document whose figures are photographs.
#[cfg(target_os = "macos")]
fn docx_html_via_pandoc(input: &Path, work: &Path) -> Option<PathBuf> {
    let pandoc = resolve_tool("pandoc")?;
    let html = work.join("document.html");
    let status = background_command(&pandoc)
        .arg(input)
        .args(["-f", "docx", "-t", "html5"])
        .arg("--extract-media")
        .arg(work.join("media"))
        .args(["-o", html.to_string_lossy().as_ref()])
        .status()
        .ok()?;
    if !status.success() || !html.exists() {
        return None;
    }
    let body = fs::read_to_string(&html).ok()?;
    if fs::write(&html, crate::markdown::page(&body)).is_err() {
        return None;
    }
    Some(html)
}

/// Converts through the built-in macOS text converter.
///
/// Pandoc is an optional tool, and machines without it used to fail every
/// Word/RTF request. `textutil` ships with macOS and writes the same
/// formats, so Markdown is rendered to HTML first and handed over.
#[cfg(target_os = "macos")]
fn textutil_convert(
    input: &Path,
    target: &str,
    output_dir: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    let writer = match target.to_ascii_lowercase().as_str() {
        "docx" => "docx",
        "rtf" => "rtf",
        "html" | "htm" => "html",
        "txt" => "txt",
        _ => return Ok(None),
    };
    let extension = extension_of(input);
    let mut source = input.to_path_buf();
    let mut temporary = None;
    // textutil reads Markdown as plain text, so give it rendered HTML instead.
    if matches!(extension.as_str(), "md" | "markdown") {
        let path = std::env::temp_dir().join(format!("shift-doc-{}.html", uuid::Uuid::new_v4()));
        let text = fs::read_to_string(input)
            .map_err(|error| format!("Could not read {}: {error}", input.display()))?;
        fs::write(&path, crate::markdown::to_html(&text))
            .map_err(|error| format!("Could not render Markdown: {error}"))?;
        source = path.clone();
        temporary = Some(path);
    }
    let output = unique_output_in(input, output_dir, "", writer)?;
    let status = background_command("/usr/bin/textutil")
        .args(["-convert", writer])
        .arg("-output")
        .arg(output.to_string_lossy().as_ref())
        .arg(source.to_string_lossy().as_ref())
        .status();
    if let Some(path) = temporary {
        let _ = fs::remove_file(path);
    }
    match status {
        Ok(status) if status.success() && output.exists() => Ok(Some(output)),
        Ok(_) => {
            let _ = fs::remove_file(&output);
            Ok(None)
        }
        Err(error) => Err(format!("Could not run the macOS text converter: {error}")),
    }
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
    on_progress: Option<&(dyn Fn(u32, u32, bool) + Sync)>,
) -> Result<PathBuf, String> {
    let extension = extension_of(input);
    if extension == "pages" {
        return Err("Pages files are not supported directly. Export as DOCX first.".into());
    }
    if target.eq_ignore_ascii_case("pdf") {
        #[cfg(target_os = "macos")]
        if let Some(output) = native_document_pdf(input, output_dir, on_progress)? {
            return Ok(output);
        }
    }
    // Image-bearing PDFs go straight to page rendering. A text-only PDF keeps
    // the text-layer path so its Word output stays editable.
    if extension == "pdf" && target.eq_ignore_ascii_case("docx") {
        if !pdf::has_embedded_images(input)? {
            let text = pdf::text_of(input)?;
            let temporary =
                std::env::temp_dir().join(format!("shift-pdf-{}.md", uuid::Uuid::new_v4()));
            fs::write(&temporary, text).map_err(|error| error.to_string())?;
            let target_dir = output_dir
                .map(Path::to_path_buf)
                .unwrap_or_else(|| directory_of(input));
            let result = convert_document(&temporary, "docx", Some(&target_dir), None)
                .and_then(|generated| rename_office_output(generated, input, &target_dir));
            let _ = fs::remove_file(&temporary);
            return result;
        }
        if pdf::renderer_path().is_none() {
            return Err(
                "PDF to Word needs Poppler (pdftoppm). Install it from Settings > Runtime Components."
                    .into(),
            );
        }
        let output = unique_output_in(input, output_dir, "", "docx")?;
        match on_progress {
            Some(report) => {
                pdf::to_docx_with_progress(input, &output, |done, total, packaging| {
                    report(done, total, packaging)
                })?
            }
            None => pdf::to_docx(input, &output)?,
        }
        return Ok(output);
    }
    let target = target.to_ascii_lowercase();
    let pdf_engine = (target == "pdf").then(document_pdf_engine).flatten();
    if target == "pdf" && pdf_engine.is_none() {
        // Only Windows and Linux drive a PDF engine through Pandoc, and only they list
        // one in Settings > Runtime components. Reaching this on macOS means the
        // built-in renderer is unavailable too, so pointing at a Typst row there would
        // send the user to a row that panel never shows.
        #[cfg(target_os = "macos")]
        return Err(
            "PDF output needs the built-in document renderer or a Pandoc PDF engine (Typst, WeasyPrint, or a LaTeX engine on your PATH)."
                .into(),
        );
        #[cfg(not(target_os = "macos"))]
        return Err(
            "PDF output needs a Pandoc PDF engine. Install Typst from Settings > Runtime Components or add a supported engine to your PATH."
                .into(),
        );
    }
    let (extension, writer) = match target.as_str() {
        "md" | "markdown" => ("md", Some("gfm")),
        "html" => ("html", Some("html")),
        "txt" => ("txt", Some("plain")),
        "docx" => ("docx", None),
        "rtf" => ("rtf", None),
        "pdf" => ("pdf", None),
        other => return Err(format!("Unsupported document format: {other}")),
    };
    let pandoc = match resolve_tool("pandoc") {
        Some(pandoc) => pandoc,
        None => {
            #[cfg(target_os = "macos")]
            if let Some(output) = textutil_convert(input, extension, output_dir)? {
                return Ok(output);
            }
            return Err(
                "Document conversion needs pandoc. Install it from Settings > Runtime Components or add it to your PATH."
                    .into(),
            );
        }
    };
    let output = unique_output_in(input, output_dir, "", extension)?;
    let mut command = background_command(&pandoc);
    command.arg(input);
    // Pandoc resolves an image path relative to its own working directory,
    // which for a running Shift is nowhere near the document. Without this a
    // Markdown image is reported as unfetchable and replaced by its alt text,
    // so the DOCX/RTF/HTML output silently loses every figure.
    if let Some(directory) = input.parent().filter(|value| !value.as_os_str().is_empty()) {
        command.arg("--resource-path").arg(directory);
    }
    if let Some(writer) = writer {
        command.args(["-t", writer]);
    }
    if matches!(target.as_str(), "html" | "rtf") {
        command.arg("--standalone");
    }
    if target == "html" {
        // DOCX images otherwise become references to a media folder that
        // Pandoc never writes. Inline them so the single HTML file is portable.
        command.arg("--self-contained");
    }
    if let Some(engine) = pdf_engine {
        command.arg("--pdf-engine").arg(engine);
    }
    let status = command
        .args(["-o", output.to_string_lossy().as_ref()])
        .status()
        .map_err(|error| format!("Could not run pandoc: {error}"))?;
    if !status.success() || !output.exists() {
        let _ = fs::remove_file(&output);
        #[cfg(target_os = "macos")]
        if let Some(output) = textutil_convert(input, extension, output_dir)? {
            return Ok(output);
        }
        return Err(format!(
            "Could not convert {} to {}. The source may be damaged or contain unsupported content.",
            input.display(),
            extension.to_ascii_uppercase()
        ));
    }
    Ok(output)
}

fn compress_file(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let image = open_for_pixels(input)?;
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
    let image = open_for_pixels(input)?;
    let ext = extension_of(input);
    let (ext, format) = normalized_target(&ext);
    let output = unique_output_in(input, output_dir, " (clean)", ext)?;
    save_image(&image, &output, format, 92)?;
    Ok(output)
}

fn resize_file(input: &Path, scale: f64, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let image = open_for_pixels(input)?;
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
    let image = open_for_pixels(input)?;
    let rotated = image.rotate90();
    let ext = extension_of(input);
    let (ext, format) = normalized_target(&ext);
    let output = unique_output_in(input, output_dir, " (rotated)", ext)?;
    save_image(&rotated, &output, format, 92)?;
    Ok(output)
}

fn flip_file(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let image = open_for_pixels(input)?;
    let flipped = image.fliph();
    let ext = extension_of(input);
    let (ext, format) = normalized_target(&ext);
    let output = unique_output_in(input, output_dir, " (flipped)", ext)?;
    save_image(&flipped, &output, format, 92)?;
    Ok(output)
}

fn grayscale_file(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let image = open_for_pixels(input)?;
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
    let status = background_command(&ffmpeg)
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
    let status = background_command(&ffmpeg)
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
    let status = background_command(&ffmpeg)
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
    let status = background_command(&ffmpeg)
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
    input
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn crop_file(
    input: &Path,
    rect: &crate::models::NormalizedRect,
    output_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    let image = open_for_pixels(input)?;
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
        let mut command = crate::process::command("/usr/bin/open");
        command.arg(path);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = crate::process::command("xdg-open");
        command.arg(path);
        command
    };
    #[cfg(target_os = "windows")]
    return windows_open(path);

    #[cfg(not(target_os = "windows"))]
    {
        command.status().map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn windows_open(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result <= 32 {
        Err(format!(
            "Windows could not open {}. The default application may be missing.",
            path.display()
        ))
    } else {
        Ok(())
    }
}

pub fn reveal_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return crate::process::command("/usr/bin/open")
        .args(["-R", path.to_string_lossy().as_ref()])
        .status()
        .map(|_| ())
        .map_err(|error| error.to_string());

    #[cfg(target_os = "windows")]
    return windows_reveal(path);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return crate::process::command("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .status()
        .map(|_| ())
        .map_err(|error| error.to_string());
}

/// Selects a file in Explorer the way `open -R` does in Finder: a window already
/// showing that folder is reused and brought forward. `explorer /select,…` cannot
/// do this, it spawns a fresh window for every conversion that finishes.
#[cfg(target_os = "windows")]
fn windows_reveal(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows_sys::Win32::UI::Shell::{
        ILFindLastID, ILFree, ILRemoveLastID, SHOpenFolderAndSelectItems, SHParseDisplayName,
    };

    // The shell namespace APIs need COM on this thread. A thread that already
    // picked another apartment model returns RPC_E_CHANGED_MODE and keeps it, so
    // only uninitialise an apartment this call actually created.
    const RPC_E_CHANGED_MODE: i32 = -2_147_418_113; // 0x80010105
    let initialised = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut item = std::ptr::null_mut();
    let parsed = unsafe {
        SHParseDisplayName(
            wide.as_ptr(),
            std::ptr::null_mut(),
            &mut item,
            0,
            std::ptr::null_mut(),
        )
    };
    let result = if parsed < 0 || item.is_null() {
        Err(format!(
            "Windows could not find {} in the shell namespace.",
            path.display()
        ))
    } else {
        // Read the target's own identifier before truncating the list: removing
        // the last entry first would leave the parent folder's identifier.
        let child = unsafe { ILFindLastID(item) };
        if child.is_null() || unsafe { ILRemoveLastID(item) } == 0 {
            Err(format!("Windows could not open {}.", path.display()))
        } else {
            let children = [child as *const _];
            let opened = unsafe { SHOpenFolderAndSelectItems(item, 1, children.as_ptr(), 0) };
            if opened < 0 {
                Err(format!("Explorer refused to reveal {}.", path.display()))
            } else {
                Ok(())
            }
        }
    };

    unsafe { ILFree(item) };
    if initialised != RPC_E_CHANGED_MODE {
        unsafe { CoUninitialize() };
    }
    result
}

pub fn perform_action(
    action: &str,
    paths: &[String],
    target: Option<&str>,
    output_dir: Option<&Path>,
    on_progress: Option<&(dyn Fn(u32, u32, bool) + Sync)>,
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
            "convert"
                if is_pdf && target.is_some_and(|value| value.eq_ignore_ascii_case("docx")) =>
            {
                let target_dir = output_dir
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| directory_of(input));
                convert_document(input, "docx", Some(&target_dir), on_progress)?
            }
            "convert" if is_pdf && matches!(target, Some("rtf")) => {
                // The text layer becomes a Markdown intermediate, then the same
                // document writer produces the requested container.
                let text = pdf::text_of(input)?;
                let temporary =
                    std::env::temp_dir().join(format!("shift-pdf-{}.md", uuid::Uuid::new_v4()));
                fs::write(&temporary, text).map_err(|error| error.to_string())?;
                let target = target.unwrap_or("docx");
                // Write straight into the resolved output folder, then rename away
                // from the throwaway intermediate name.
                let target_dir = output_dir
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| directory_of(input));
                let result = convert_document(&temporary, target, Some(&target_dir), None)
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
                convert_document(input, target.unwrap_or("md"), output_dir, on_progress)?
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
                    let result = convert_document(&markdown, "pdf", Some(&target_dir), None)
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
                    let result = convert_document(&markdown, "pdf", Some(&target_dir), None)
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

/// Every command in this file reads, decodes or spawns, so each one runs on a
/// blocking thread: the webview stays responsive while previews are decoded and
/// while a conversion writes its output.
#[tauri::command]
pub async fn inspect_files(paths: Vec<String>) -> Result<Vec<FileInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || inspect_paths(&paths))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn perform_file_action(
    app: AppHandle,
    job_id: String,
    action: String,
    paths: Vec<String>,
    target: Option<String>,
) -> Result<(), String> {
    // Return immediately and report through events. The webview never waits on
    // a slow encoder or document renderer before it can paint the busy state.
    let worker = app.clone();
    std::thread::Builder::new()
        .name("shift-file-action".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || -> Result<ActionOutcome, String> {
                    let output_dir = settings::files_output_dir(&worker)?;
                    let auto_reveal = settings::auto_reveal(&worker);
                    let progress_worker = worker.clone();
                    let progress_job = job_id.clone();
                    let outcome = perform_action(
                        &action,
                        &paths,
                        target.as_deref(),
                        output_dir.as_deref(),
                        Some(&|done, total, packaging| {
                            let _ = progress_worker.emit(
                                "app://file-action-progress",
                                FileActionProgress {
                                    job_id: progress_job.clone(),
                                    done,
                                    total,
                                    packaging,
                                },
                            );
                        }),
                    )?;
                    if auto_reveal {
                        if let Some(first) = outcome.outputs.first() {
                            let path = Path::new(first);
                            if path.exists() {
                                let _ = reveal_path(path);
                            }
                        }
                    }
                    Ok(outcome)
                },
            ))
            .unwrap_or_else(|_| Err("File action stopped unexpectedly.".into()));

            match result {
                Ok(outcome) => {
                    crate::history::publish_result(&worker, outcome.clone());
                    let _ = worker.emit(
                        "app://file-action-finished",
                        ActionResultEvent {
                            job_id,
                            outcome: Some(outcome),
                            error: None,
                        },
                    );
                }
                Err(error) => {
                    let _ = worker.emit(
                        "app://file-action-finished",
                        ActionResultEvent {
                            job_id,
                            outcome: None,
                            error: Some(error),
                        },
                    );
                }
            }
        })
        .map_err(|error| format!("Could not start the file action: {error}"))?;
    Ok(())
}

#[tauri::command]
pub async fn reveal_file(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || reveal_path(Path::new(&path)))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn open_file(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || open_path(Path::new(&path)))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn read_text_file(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        fs::read_to_string(path).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Return image bytes as a data URL. The editor draws this into a canvas and
/// exports it later; loading through `asset://` would taint the canvas in
/// WKWebView and make `toDataURL` fail with SecurityError.
#[tauri::command]
pub async fn read_image_data(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || image_data_url(Path::new(&path)))
        .await
        .map_err(|error| error.to_string())?
}

fn image_data_url(path: &Path) -> Result<String, String> {
    let ext = extension_of(path);
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err("This file is not a supported image.".to_string());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    Ok(format!(
        "data:{};base64,{}",
        image_mime(&ext),
        STANDARD.encode(bytes)
    ))
}

#[tauri::command]
pub async fn save_image_data(
    app: AppHandle,
    source: String,
    data_url: String,
    suffix: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let output_dir = settings::files_output_dir(&app)?;
        let output =
            save_image_data_in(&source, &data_url, suffix.as_deref(), output_dir.as_deref())?;
        // Annotations saved from the editor are a result like any other, and this
        // path does not run through the action pipeline, so it records itself.
        crate::history::record(&app, std::slice::from_ref(&output));
        Ok(output)
    })
    .await
    .map_err(|error| error.to_string())?
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
pub async fn render_pdf_pages(app: AppHandle, path: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
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
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn cleanup_pdf_pages(app: AppHandle, paths: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cache = app_cache_dir(&app)?;
        let canonical_cache = fs::canonicalize(&cache).map_err(|error| error.to_string())?;
        for path in paths {
            let path = PathBuf::from(path);
            if let Some(parent) = path.parent() {
                let is_pdf_page_dir = parent
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("ocr-"))
                    && fs::canonicalize(parent)
                        .ok()
                        .and_then(|directory| directory.parent().map(Path::to_path_buf))
                        .is_some_and(|directory| directory == canonical_cache);
                if is_pdf_page_dir {
                    let _ = fs::remove_dir_all(parent);
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn save_text_near(
    app: AppHandle,
    source: String,
    text: String,
    suffix: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let output_dir = settings::files_output_dir(&app)?;
        let output = save_text_near_in(&source, &text, suffix.as_deref(), output_dir.as_deref())?;
        crate::history::record(&app, std::slice::from_ref(&output));
        Ok(output)
    })
    .await
    .map_err(|error| error.to_string())?
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

    #[cfg(target_os = "macos")]
    fn docx_with_header_and_footer(path: &Path) {
        use std::io::Write as _;
        use zip::{write::SimpleFileOptions, ZipWriter};

        let entries = [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
<Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>
</Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
            ),
            (
                "word/document.xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>Body content that must survive.</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rId1"/><w:footerReference w:type="default" r:id="rId2"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
            ),
            (
                "word/_rels/document.xml.rels",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/>
</Relationships>"#,
            ),
            (
                "word/header1.xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Shift Header Regression</w:t></w:r></w:p></w:hdr>"#,
            ),
            (
                "word/footer1.xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Shift Footer Regression</w:t></w:r></w:p></w:ftr>"#,
            ),
        ];

        let file = fs::File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        for (name, content) in entries {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
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

    #[cfg(target_os = "macos")]
    #[test]
    fn converts_system_only_image_formats_to_native_webp() {
        let dir = scratch_dir();
        let input = dir.join("sample.png");
        sample_image(&input);
        let heic = dir.join("sample.heic");
        if system_convert(&input, &heic, "heic").is_err() {
            fs::remove_dir_all(dir).unwrap();
            return;
        }

        let output = convert_file(&heic, "webp", None).unwrap();
        assert_eq!(
            output.extension().and_then(|value| value.to_str()),
            Some("webp")
        );
        assert!(image::open(&output).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pixel_tools_read_system_only_image_formats() {
        let dir = scratch_dir();
        let input = dir.join("sample.png");
        sample_image(&input);
        let heic = dir.join("phone.heic");
        if system_convert(&input, &heic, "heic").is_err() {
            fs::remove_dir_all(dir).unwrap();
            return;
        }

        // The pixel tools used to fail here because the Rust decoder cannot read
        // HEIC. The result is a PNG, which is what its name now says it is.
        let rotated = rotate_file(&heic, None).unwrap();
        assert_eq!(
            rotated.extension().and_then(|value| value.to_str()),
            Some("png")
        );
        assert!(image::open(&rotated).is_ok());
        let half = resize_file(&heic, 0.5, None).unwrap();
        assert!(image::open(&half).is_ok());
        assert!(image::open(&half).unwrap().width() <= 16);
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
            None,
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
        let data_url = image_data_url(&input).unwrap();
        assert!(data_url.starts_with("data:image/png;base64,"));
        let data = fs::read(&input).unwrap();
        assert_eq!(
            data_url,
            format!("data:image/png;base64,{}", STANDARD.encode(data))
        );
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
        let output = convert_document(&input, "md", None, None).unwrap();
        let text = fs::read_to_string(output).unwrap();
        assert!(text.contains("# Shift"));
        assert!(text.contains("**documents**"));
        let html = convert_document(&input, "html", None, None).unwrap();
        assert!(fs::read_to_string(html)
            .unwrap()
            .contains("<!DOCTYPE html>"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn textutil_fallback_converts_markdown_to_word_formats() {
        for target in ["docx", "rtf"] {
            let dir = scratch_dir();
            let input = dir.join("note.md");
            fs::write(&input, "# Shift\n\nLocal **documents**.\n").unwrap();

            let output = textutil_convert(&input, target, None)
                .unwrap()
                .unwrap_or_else(|| panic!("textutil did not write {target}"));
            assert_eq!(
                output.extension().and_then(|value| value.to_str()),
                Some(target)
            );
            assert!(output.exists(), "{output:?}");
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renders_documents_to_pdf_with_native_helper() {
        let dir = scratch_dir();
        let input = dir.join("sample.html");
        fs::write(&input, "<h1>Shift</h1><p>Native PDF.</p>").unwrap();
        let output = convert_document(&input, "pdf", None, None).unwrap();
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
    fn renders_image_only_docx_pages_to_pdf() {
        use std::io::Write as _;
        use zip::{write::SimpleFileOptions, ZipWriter};

        let dir = scratch_dir();
        let first = dir.join("first.png");
        let second = dir.join("second.png");
        sample_image(&first);
        sample_image(&second);
        let input = dir.join("scan.docx");
        let file = fs::File::create(&input).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("word/_rels/document.xml.rels", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/first.png"/>
              <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/second.png"/>
            </Relationships>"#,
        )
        .unwrap();
        zip.start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<w:document xmlns:w="w" xmlns:a="a" xmlns:r="r"><w:body>
              <w:p><w:r><w:drawing><a:blip r:embed="rId1"/></w:drawing></w:r></w:p>
              <w:p><w:r><w:drawing><a:blip r:embed="rId2"/></w:drawing></w:r></w:p>
            </w:body></w:document>"#,
        )
        .unwrap();
        for (name, path) in [("first.png", &first), ("second.png", &second)] {
            zip.start_file(format!("word/media/{name}"), SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&fs::read(path).unwrap()).unwrap();
        }
        zip.finish().unwrap();

        let progress = std::sync::Mutex::new(Vec::new());
        let report = |done, total, packaging| {
            progress.lock().unwrap().push((done, total, packaging));
        };
        let output = convert_document(&input, "pdf", None, Some(&report)).unwrap();
        let pages = lopdf::Document::load(&output).unwrap().get_pages().len();
        assert_eq!(pages, 2);
        assert_eq!(
            *progress.lock().unwrap(),
            vec![(1, 2, false), (2, 2, false)]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn keeps_docx_headers_and_footers_when_converting_to_pdf() {
        let dir = scratch_dir();
        let input = dir.join("header.docx");
        docx_with_header_and_footer(&input);

        let output = convert_document(&input, "pdf", None, None).unwrap();
        let text = pdf::text_of(&output).unwrap_or_default();
        assert!(text.contains("Shift Header Regression"), "{text}");
        assert!(text.contains("Body content that must survive"), "{text}");
        assert!(text.contains("Shift Footer Regression"), "{text}");
        fs::remove_dir_all(dir).unwrap();
    }

    /// The bug this pins: AppKit's OOXML reader keeps the text of a DOCX and
    /// silently drops its pictures, so a report with figures printed as a text
    /// document with no figures at all. The PDF must carry the image.
    #[cfg(target_os = "macos")]
    #[test]
    fn keeps_pictures_when_rendering_a_text_docx_to_pdf() {
        if which::which("pandoc").is_err() {
            return;
        }
        let dir = scratch_dir();
        let picture = dir.join("figure.png");
        sample_image(&picture);
        // Pandoc writes the fixture so the DOCX is a file a real reader accepts,
        // with text and a picture in the same body.
        let markdown = dir.join("report.md");
        fs::write(
            &markdown,
            format!(
                "# Report\n\nArchitecture overview.\n\n![Figure]({})\n",
                picture.display()
            ),
        )
        .unwrap();
        let input = dir.join("report.docx");
        let status = background_command("pandoc")
            .arg(&markdown)
            .args(["-o", input.to_string_lossy().as_ref()])
            .status()
            .unwrap();
        assert!(status.success() && input.exists());
        assert!(office::contains_inline_images(&input).unwrap());

        let output = convert_document(&input, "pdf", None, None).unwrap();
        assert!(
            pdf::has_embedded_images(&output).unwrap(),
            "the figure was dropped from {}",
            output.display()
        );
        let text = pdf::text_of(&output).unwrap_or_default();
        assert!(text.contains("Architecture overview"), "{text}");
        fs::remove_dir_all(dir).unwrap();
    }

    /// The second half of the same bug. The layout sizes a picture from the
    /// density header inside the file, so a screenshot saved at 96 dpi asks for
    /// 1200 points and prints as a cropped fragment of itself. The renderer has
    /// to fit it to the page; a Form XObject's `/BBox` reports that width
    /// without needing the content stream decoded.
    #[cfg(target_os = "macos")]
    #[test]
    fn fits_a_wide_picture_onto_the_page() {
        if which::which("pandoc").is_err() {
            return;
        }
        let dir = scratch_dir();
        let picture = dir.join("screenshot.png");
        // 1200 px, and 72 dpi is what the layout assumes for a file with none.
        RgbaImage::from_pixel(1200, 400, Rgba([30, 120, 200, 255]))
            .save_with_format(&picture, ImageFormat::Png)
            .unwrap();
        let markdown = dir.join("shot.md");
        fs::write(
            &markdown,
            format!("# Shot\n\nbefore\n\n![S]({})\n\nafter\n", picture.display()),
        )
        .unwrap();
        let input = dir.join("shot.docx");
        assert!(background_command("pandoc")
            .arg(&markdown)
            .args(["-o", input.to_string_lossy().as_ref()])
            .status()
            .unwrap()
            .success());

        let output = convert_document(&input, "pdf", None, None).unwrap();
        let rendered = pdf::to_images_in(&output, "png", &dir.join("rendered")).unwrap();
        assert_eq!(rendered.len(), 1, "the short report grew an extra page");
        let page = image::open(&rendered[0]).unwrap().to_rgb8();
        let mut blue_columns = Vec::new();
        for (x, _y, pixel) in page.enumerate_pixels() {
            if pixel[2] > 150 && pixel[1] > 70 && pixel[0] < 100 {
                blue_columns.push(x);
            }
        }
        let left = *blue_columns.iter().min().expect("the picture disappeared");
        let right = *blue_columns.iter().max().expect("the picture disappeared");
        assert!(
            left > 1 && right + 2 < page.width(),
            "picture runs off the page: columns {left}..={right} of {}",
            page.width()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    /// A Markdown image is stored as a path, so Pandoc has to fetch the file to
    /// embed it. It resolves that path against its own working directory, which
    /// for a running Shift is nowhere near the document, and a figure it cannot
    /// read is quietly replaced by its alt text.
    #[test]
    fn keeps_markdown_images_when_converting_to_word() {
        if which::which("pandoc").is_err() {
            return;
        }
        let dir = scratch_dir();
        let nested = dir.join("images");
        fs::create_dir_all(&nested).unwrap();
        let picture = nested.join("architecture.png");
        sample_image(&picture);
        let input = dir.join("report.md");
        fs::write(
            &input,
            "# Report\n\n![Architecture](images/architecture.png)\n",
        )
        .unwrap();

        for target in ["docx", "rtf"] {
            let output = convert_document(&input, target, None, None).unwrap();
            let bytes = fs::read(&output).unwrap();
            let embedded = if target == "docx" {
                let archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
                let names: Vec<String> = archive.file_names().map(str::to_string).collect();
                names.iter().any(|name| name.starts_with("word/media/"))
            } else {
                bytes.windows(7).any(|window| window == b"pngblip")
            };
            assert!(
                embedded,
                "{target} written from {} lost the picture",
                input.display()
            );
        }
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
            None,
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
            None,
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
    fn converts_pdf_text_layer_to_rtf_when_pandoc_is_available() {
        if which::which("pandoc").is_err() {
            return;
        }
        let dir = scratch_dir();
        let input = dir.join("report.pdf");
        write_text_pdf(&input, "Quarterly numbers");

        let outcome = perform_action(
            "convert",
            &[input.to_string_lossy().to_string()],
            Some("rtf"),
            None,
            None,
        )
        .expect("PDF to RTF failed");
        assert_eq!(outcome.outputs.len(), 1);
        assert!(
            outcome.outputs[0].ends_with("report.rtf"),
            "output must follow the requested format: {:?}",
            outcome.outputs[0]
        );
        assert!(fs::read_to_string(&outcome.outputs[0])
            .unwrap()
            .starts_with("{\\rtf1"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pdf_to_odt_is_rejected() {
        let dir = scratch_dir();
        let input = dir.join("report.pdf");
        write_text_pdf(&input, "Quarterly numbers");

        let error = perform_action(
            "convert",
            &[input.to_string_lossy().to_string()],
            Some("odt"),
            None,
            None,
        )
        .unwrap_err();
        assert!(error.contains("DOCX or RTF"), "{error}");
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
            None,
        )
        .unwrap();
        let produced = PathBuf::from(&outcome.outputs[0]);
        assert_eq!(
            produced.parent(),
            Some(output_dir.as_path()),
            "a custom output folder must win over the source folder: {produced:?}"
        );
        assert!(
            produced.to_string_lossy().contains("report"),
            "{produced:?}"
        );
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
        let pages_id = document.add_object(
            dictionary! { "Type" => "Pages", "Kids" => Vec::<Object>::new(), "Count" => 0_i64 },
        );
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

        let output = convert_document(&docx, "md", None, None).unwrap();
        let text = fs::read_to_string(output).unwrap();
        assert!(text.contains("Shift"));
        assert!(text.contains("Document round trip"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn image_bearing_docx_to_html_embeds_images() {
        if which::which("pandoc").is_err() {
            return;
        }
        let dir = scratch_dir();
        let image = dir.join("sample.png");
        sample_image(&image);
        let source = dir.join("sample.md");
        fs::write(&source, "![Sample](sample.png)\n").unwrap();
        let docx = dir.join("sample.docx");
        let status = Command::new("pandoc")
            .current_dir(&dir)
            .arg("sample.md")
            .args(["-o", "sample.docx"])
            .status()
            .unwrap();
        assert!(status.success());

        let output = convert_document(&docx, "html", None, None).unwrap();
        let html = fs::read_to_string(output).unwrap();
        assert!(html.contains("<img"), "{html}");
        assert!(html.contains("src=\"data:image/"), "{html}");
        assert!(!html.contains("src=\"media/"), "{html}");
        fs::remove_dir_all(dir).unwrap();
    }

    /// Writes an animated GIF with the given frame count and delay.
    fn sample_animated_gif(path: &Path, frames: usize, delay_ms: u32) {
        let mut buffer: Vec<u8> = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut buffer);
            for index in 0..frames {
                let mut frame = RgbaImage::from_pixel(64, 48, Rgba([index as u8, 40, 90, 255]));
                frame.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
                encoder
                    .encode_frame(Frame::from_parts(
                        frame,
                        0,
                        0,
                        Delay::from_numer_denom_ms(delay_ms, 1),
                    ))
                    .unwrap();
            }
        }
        fs::write(path, buffer).unwrap();
    }

    #[test]
    fn gif_conversion_keeps_animation() {
        let dir = scratch_dir();
        let input = dir.join("animated.gif");
        sample_animated_gif(&input, 8, 50);

        let output = convert_file(&input, "gif", None).unwrap();
        let decoder = GifDecoder::new(BufReader::new(File::open(&output).unwrap())).unwrap();
        let frames = decoder
            .into_frames()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(frames.len(), 8, "animation must survive the conversion");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gif_conversion_caps_duration_and_size() {
        let dir = scratch_dir();
        let input = dir.join("long.gif");
        // 240 frames of 100 ms is 24 seconds, well past the 15 second cap.
        sample_animated_gif(&input, 240, 100);

        let output = convert_file(&input, "gif", None).unwrap();
        let bytes = fs::metadata(&output).unwrap().len();
        assert!(
            bytes <= GIF_MAX_BYTES,
            "GIF grew past the ceiling: {bytes} bytes"
        );

        let decoder = GifDecoder::new(BufReader::new(File::open(&output).unwrap())).unwrap();
        let mut elapsed = 0_u32;
        let mut count = 0_usize;
        for frame in decoder.into_frames() {
            let frame = frame.unwrap();
            elapsed = elapsed.saturating_add(frame.delay().numer_denom_ms().0);
            count += 1;
        }
        assert!(count <= GIF_MAX_FRAMES, "too many frames kept: {count}");
        assert!(
            elapsed <= GIF_MAX_MILLIS,
            "animation is longer than the cap: {elapsed} ms"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gif_conversion_respects_a_tight_byte_ceiling() {
        let dir = scratch_dir();
        let input = dir.join("noisy.gif");
        sample_animated_gif(&input, 40, 40);

        let output = dir.join("small.gif");
        save_gif_capped(&input, &output, 20_000).unwrap();
        let bytes = fs::metadata(&output).unwrap().len();
        assert!(bytes <= 20_000, "GIF grew past the ceiling: {bytes} bytes");

        // The result must still be a readable GIF, not a truncated file.
        let decoder = GifDecoder::new(BufReader::new(File::open(&output).unwrap())).unwrap();
        assert!(decoder.into_frames().next().unwrap().is_ok());
        fs::remove_dir_all(dir).unwrap();
    }
}
