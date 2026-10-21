use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use lopdf::{Document, Object, ObjectId};
use printpdf::{Mm, Op, PdfDocument, PdfPage, RawImage, XObjectTransform};

use crate::files::unique_output_in;

fn load(path: &Path) -> Result<Document, String> {
    Document::load(path).map_err(|error| format!("Could not read {}: {error}", path.display()))
}

fn page_numbers(doc: &Document) -> Vec<u32> {
    doc.get_pages().keys().copied().collect()
}

pub fn merge(paths: &[PathBuf], output_dir: Option<&Path>) -> Result<PathBuf, String> {
    if paths.len() < 2 {
        return Err("Select at least two PDF files to merge.".into());
    }
    let mut max_id = 1;
    let mut page_objects = BTreeMap::new();
    let mut objects = BTreeMap::new();
    let mut merged = Document::with_version("1.5");

    for path in paths {
        let mut document = load(path)?;
        document.renumber_objects_with(max_id);
        max_id = document.max_id + 1;
        for page_id in document.get_pages().into_values() {
            if let Ok(object) = document.get_object(page_id) {
                page_objects.insert(page_id, object.clone());
            }
        }
        objects.extend(document.objects);
    }

    let mut catalog: Option<(ObjectId, Object)> = None;
    let mut pages_root: Option<(ObjectId, Object)> = None;
    for (object_id, object) in objects {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                if catalog.is_none() {
                    catalog = Some((object_id, object));
                }
            }
            b"Pages" => {
                let mut dict = object.as_dict().map_err(|error| error.to_string())?.clone();
                if let Some((_, Object::Dictionary(previous))) = pages_root.as_ref() {
                    dict.extend(previous);
                }
                pages_root = Some((
                    pages_root
                        .as_ref()
                        .map(|value| value.0)
                        .unwrap_or(object_id),
                    Object::Dictionary(dict),
                ));
            }
            b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                merged.objects.insert(object_id, object);
            }
        }
    }

    let (pages_id, pages_object) = pages_root.ok_or("No PDF pages found.")?;
    let (catalog_id, catalog_object) = catalog.ok_or("No PDF catalog found.")?;

    for (object_id, object) in page_objects {
        let mut dict = object.as_dict().map_err(|error| error.to_string())?.clone();
        dict.set("Parent", pages_id);
        merged.objects.insert(object_id, Object::Dictionary(dict));
    }

    let mut pages_dict = pages_object
        .as_dict()
        .map_err(|error| error.to_string())?
        .clone();
    let kids: Vec<Object> = merged
        .objects
        .iter()
        .filter_map(|(id, object)| {
            if object.type_name().ok() == Some(b"Page") {
                Some(Object::Reference(*id))
            } else {
                None
            }
        })
        .collect();
    pages_dict.set("Kids", kids);
    pages_dict.set(
        "Count",
        merged
            .objects
            .values()
            .filter(|object| object.type_name().ok() == Some(b"Page"))
            .count() as i64,
    );
    merged
        .objects
        .insert(pages_id, Object::Dictionary(pages_dict));

    let mut catalog_dict = catalog_object
        .as_dict()
        .map_err(|error| error.to_string())?
        .clone();
    catalog_dict.set("Pages", pages_id);
    catalog_dict.remove(b"Outlines");
    merged
        .objects
        .insert(catalog_id, Object::Dictionary(catalog_dict));
    merged.trailer.set("Root", catalog_id);
    merged.renumber_objects();
    let output = unique_output_in(&paths[0], output_dir, " (merged)", "pdf")?;
    merged
        .save(&output)
        .map_err(|error| format!("Could not save merged PDF: {error}"))?;
    Ok(output)
}

pub fn split(input: &Path, output_dir: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    let page_count = load(input)?.get_pages().len();
    let parent = output_dir.map(PathBuf::from).unwrap_or_else(|| {
        input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    });
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("PDF");
    let mut folder = parent.join(format!("{stem} pages"));
    let mut counter = 2;
    while folder.exists() {
        folder = parent.join(format!("{stem} pages {counter}"));
        counter += 1;
    }
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let all_pages: Vec<u32> = (1..=page_count as u32).collect();
    let mut outputs = Vec::new();
    for page_number in 1..=page_count as u32 {
        let mut document = load(input)?;
        let remove: Vec<u32> = all_pages
            .iter()
            .copied()
            .filter(|number| *number != page_number)
            .collect();
        document.delete_pages(&remove);
        let output = folder.join(format!("Page {page_number:03}.pdf"));
        document
            .save(&output)
            .map_err(|error| format!("Could not save page {page_number}: {error}"))?;
        outputs.push(output);
    }
    Ok(outputs)
}

pub fn rotate(input: &Path, degrees: i64, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let mut document = load(input)?;
    let page_ids: Vec<ObjectId> = document.get_pages().into_values().collect();
    for page_id in page_ids {
        let object = document
            .get_object_mut(page_id)
            .map_err(|error| error.to_string())?;
        let dict = object.as_dict_mut().map_err(|error| error.to_string())?;
        let current = dict
            .get(b"Rotate")
            .ok()
            .and_then(|value| value.as_i64().ok())
            .unwrap_or(0);
        dict.set("Rotate", (current + degrees).rem_euclid(360));
    }
    let output = unique_output_in(input, output_dir, " (rotated)", "pdf")?;
    document
        .save(&output)
        .map_err(|error| format!("Could not save rotated PDF: {error}"))?;
    Ok(output)
}

/// The text layer of a PDF, or a clear error when it is a scan.
pub fn text_of(input: &Path) -> Result<String, String> {
    let document = load(input)?;
    let pages = page_numbers(&document);
    let text = document
        .extract_text(&pages)
        .map_err(|error| format!("Could not extract PDF text: {error}"))?;
    if text.trim().is_empty() {
        return Err(
            "This PDF has no text layer. Use OCR instead for scanned PDFs.".into(),
        );
    }
    Ok(text)
}

pub fn extract_text_to(
    input: &Path,
    extension: &str,
    output_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    let text = text_of(input)?;
    let output = unique_output_in(input, output_dir, " (text)", extension)?;
    fs::write(&output, text).map_err(|error| error.to_string())?;
    Ok(output)
}

pub fn extract_text(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    extract_text_to(input, "txt", output_dir)
}

pub fn strip_metadata(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let mut document = load(input)?;
    document.trailer.remove(b"Info");
    if let Ok(catalog) = document.catalog_mut() {
        catalog.remove(b"Metadata");
    }
    let output = unique_output_in(input, output_dir, " (clean)", "pdf")?;
    document
        .save(&output)
        .map_err(|error| format!("Could not save cleaned PDF: {error}"))?;
    Ok(output)
}

/// Longest side kept for embedded images, and the JPEG quality used when
/// rewriting them. Scanned pages are routinely stored at 2500-3500 px, which is
/// wasted on screen reading; pruning objects alone barely moves those files.
const IMAGE_MAX_SIDE: u32 = 1440;
const IMAGE_JPEG_QUALITY: u8 = 60;

pub fn compress(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let mut document = load(input)?;
    let resampled = resample_images(&mut document);
    document.compress();
    document.prune_objects();
    document.renumber_objects();
    let output = unique_output_in(input, output_dir, " (compressed)", "pdf")?;
    document
        .save(&output)
        .map_err(|error| format!("Could not save compressed PDF: {error}"))?;
    let input_size = fs::metadata(input)
        .map_err(|error| error.to_string())?
        .len();
    let output_size = fs::metadata(&output)
        .map_err(|error| error.to_string())?
        .len();
    if output_size >= input_size {
        let _ = fs::remove_file(&output);
        if resampled == 0 {
            return Err("This PDF is already optimized.".into());
        }
        return Err(format!(
            "Re-encoding {} embedded image(s) did not make this PDF smaller.",
            resampled
        ));
    }
    Ok(output)
}

/// Rewrites embedded raster images as smaller JPEGs, the way a "optimize images
/// for screen" export does. Anything the pipeline cannot prove it can round-trip
/// — masks, soft masks, indexed or CMYK colour, sub-8-bit samples, or codecs we
/// do not decode — is left byte-for-byte untouched, so a failed heuristic can
/// only miss a saving, never corrupt a page.
fn resample_images(document: &mut Document) -> usize {
    let ids: Vec<ObjectId> = document
        .objects
        .iter()
        .filter(|(_, object)| matches!(object, Object::Stream(_)))
        .map(|(id, _)| *id)
        .collect();
    let mut rewritten = 0;
    for id in ids {
        if rewrite_image(document, id) {
            rewritten += 1;
        }
    }
    rewritten
}

fn rewrite_image(document: &mut Document, id: ObjectId) -> bool {
    let Some((image, is_gray)) = decode_image(document, id) else {
        return false;
    };
    if image.width().max(image.height()) <= IMAGE_MAX_SIDE {
        // Small enough already; rewriting could only lose quality.
        return false;
    }
    let scaled = image.resize(
        IMAGE_MAX_SIDE,
        IMAGE_MAX_SIDE,
        image::imageops::FilterType::Lanczos3,
    );
    let width = scaled.width();
    let height = scaled.height();
    // Encode explicitly so the declared quality is the quality that is written.
    let mut encoded = Vec::new();
    let encoded_ok = if is_gray {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, IMAGE_JPEG_QUALITY)
            .encode_image(&image::DynamicImage::ImageLuma8(scaled.to_luma8()))
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, IMAGE_JPEG_QUALITY)
            .encode_image(&image::DynamicImage::ImageRgb8(scaled.to_rgb8()))
    };
    if encoded_ok.is_err() || encoded.is_empty() {
        return false;
    }
    let original_len = match document.get_object(id) {
        Ok(Object::Stream(stream)) => stream.content.len(),
        _ => return false,
    };
    // Never trade a smaller pixel budget for a bigger file.
    if encoded.len() >= original_len {
        return false;
    }

    let Ok(Object::Stream(stream)) = document.get_object_mut(id) else {
        return false;
    };
    stream.set_content(encoded);
    let dict = &mut stream.dict;
    dict.set("Filter", "DCTDecode");
    dict.remove(b"DecodeParms");
    dict.remove(b"Decode");
    dict.set("Width", width as i64);
    dict.set("Height", height as i64);
    dict.set("BitsPerComponent", 8_i64);
    // The JPEG is written with the same component count as the source, so the
    // declared colour space still matches the decoded samples.
    dict.set("ColorSpace", if is_gray { "DeviceGray" } else { "DeviceRGB" });
    true
}

/// Decodes an embedded image, or `None` when it is unsafe to rewrite.
fn decode_image(document: &Document, id: ObjectId) -> Option<(image::DynamicImage, bool)> {
    let Object::Stream(stream) = document.get_object(id).ok()? else {
        return None;
    };
    if !is_name(stream.dict.get(b"Subtype"), b"Image") {
        return None;
    }
    // Masks carry alignment and colour-key semantics we would destroy.
    for key in [b"SMask".as_slice(), b"Mask".as_slice(), b"Alternates".as_slice()] {
        if stream.dict.get(key).is_ok() {
            return None;
        }
    }
    if matches!(stream.dict.get(b"ImageMask"), Ok(mask) if matches!(mask.as_bool(), Ok(true))) {
        return None;
    }
    if !matches!(stream.dict.get(b"BitsPerComponent"), Ok(bits) if matches!(bits.as_i64(), Ok(8))) {
        return None;
    }
    let space = resolve_name(document, stream.dict.get(b"ColorSpace").ok()?.clone())?;
    let is_gray = space == b"DeviceGray";
    if !is_gray && space != b"DeviceRGB" {
        return None;
    }
    let width = stream.dict.get(b"Width").ok()?.as_i64().ok()? as u32;
    let height = stream.dict.get(b"Height").ok()?.as_i64().ok()? as u32;
    if width == 0 || height == 0 || width > 20_000 || height > 20_000 {
        return None;
    }

    match stream.filters().unwrap_or_default().as_slice() {
        // Already a JPEG: decode, shrink, re-encode.
        [b"DCTDecode"] => image::load_from_memory(&stream.content).ok().map(|image| (image, is_gray)),
        // Raw or deflate-packed samples: rebuild from the pixel array.
        [] | [b"FlateDecode"] => {
            let bytes_per_pixel = if is_gray { 1 } else { 3 };
            let decoded = stream.get_plain_content().ok()?;
            if decoded.len() != (width as usize) * (height as usize) * bytes_per_pixel {
                return None;
            }
            let image = if is_gray {
                image::GrayImage::from_raw(width, height, decoded).map(image::DynamicImage::ImageLuma8)
            } else {
                image::RgbImage::from_raw(width, height, decoded).map(image::DynamicImage::ImageRgb8)
            }?;
            Some((image, is_gray))
        }
        // JPX, JBIG2, CCITT and chains we cannot verify: leave untouched.
        _ => None,
    }
}

fn is_name(result: lopdf::Result<&Object>, expected: &[u8]) -> bool {
    matches!(result.and_then(Object::as_name), Ok(name) if name == expected)
}

/// Resolves a name that may be an indirect reference, for /ColorSpace lookups.
fn resolve_name(document: &Document, object: Object) -> Option<Vec<u8>> {
    let resolved = match object {
        Object::Name(name) => name.to_vec(),
        Object::Reference(reference) => match document.get_object(reference).ok()? {
            Object::Name(name) => name.to_vec(),
            _ => return None,
        },
        _ => return None,
    };
    Some(resolved)
}

#[cfg(target_os = "macos")]
fn pdf_render_bridge() -> Option<PathBuf> {
    let triple = match std::env::consts::ARCH {
        "aarch64" => "aarch64-apple-darwin",
        "x86_64" => "x86_64-apple-darwin",
        _ => return None,
    };
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("pdf-render-{triple}"));
    if dev.exists() {
        return Some(dev);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            let sidecar = parent.join("pdf-render");
            if sidecar.exists() {
                return Some(sidecar);
            }
        }
    }
    None
}

pub fn to_images(
    input: &Path,
    extension: &str,
    output_dir: Option<&Path>,
) -> Result<Vec<PathBuf>, String> {
    let parent = output_dir.map(PathBuf::from).unwrap_or_else(|| {
        input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    });
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("PDF");
    let mut folder = parent.join(format!("{stem} pages"));
    let mut counter = 2;
    while folder.exists() {
        folder = parent.join(format!("{stem} pages {counter}"));
        counter += 1;
    }
    to_images_in(input, extension, &folder)
}

pub fn to_images_in(input: &Path, extension: &str, folder: &Path) -> Result<Vec<PathBuf>, String> {
    let requested = extension.to_ascii_lowercase();
    // HEIC has no page renderer of its own: pages render as PNG and are then
    // re-encoded by the system codec, the same path image conversion uses.
    let (extension, transcode_to) = match requested.as_str() {
        "jpg" | "jpeg" => ("jpg", None),
        "png" => ("png", None),
        "tif" | "tiff" => ("tif", None),
        "heic" | "heif" => ("png", Some(if requested == "heif" { "heif" } else { "heic" })),
        other => return Err(format!("Unsupported PDF image format: {other}")),
    };
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;

    #[cfg(target_os = "macos")]
    {
        let bridge = pdf_render_bridge().ok_or_else(|| {
            "The PDF renderer is missing. Run pnpm mac:drag-bridge and rebuild.".to_string()
        })?;
        let status = Command::new(bridge)
            .args([
                input.to_string_lossy().as_ref(),
                folder.to_string_lossy().as_ref(),
                extension,
            ])
            .status()
            .map_err(|error| format!("Could not start PDF renderer: {error}"))?;
        if !status.success() {
            return Err("PDF page rendering failed.".into());
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let pdftoppm = crate::files::resolve_tool("pdftoppm").ok_or_else(|| {
            "PDF page rendering needs Poppler (pdftoppm) on Windows and Linux.".to_string()
        })?;
        let flag = match extension {
            "jpg" => "-jpeg",
            "tif" => "-tiff",
            _ => "-png",
        };
        let prefix = folder.join("Page");
        let status = Command::new(pdftoppm)
            .args([
                flag,
                "-r",
                "144",
                input.to_string_lossy().as_ref(),
                prefix.to_string_lossy().as_ref(),
            ])
            .status()
            .map_err(|error| format!("Could not run pdftoppm: {error}"))?;
        if !status.success() {
            return Err("PDF page rendering failed.".into());
        }
        let mut images: Vec<PathBuf> = fs::read_dir(folder)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|value| value.to_str())
                    .map(|value| value.eq_ignore_ascii_case(extension))
                    .unwrap_or(false)
            })
            .collect();
        images.sort();
        for (index, path) in images.iter().enumerate() {
            let target = folder.join(format!("Page {:03}.{extension}", index + 1));
            if path != &target {
                fs::rename(path, &target).map_err(|error| error.to_string())?;
            }
        }
    }

    let mut images: Vec<PathBuf> = fs::read_dir(folder)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case(extension))
                .unwrap_or(false)
        })
        .collect();
    images.sort();
    if images.is_empty() {
        return Err("No PDF pages were rendered.".into());
    }
    if let Some(target) = transcode_to {
        let mut converted = Vec::with_capacity(images.len());
        for page in &images {
            let desired = page.with_extension(target);
            crate::files::system_convert(page, &desired, target)?;
            let _ = fs::remove_file(page);
            converted.push(desired);
        }
        return Ok(converted);
    }
    Ok(images)
}

pub fn images_to_pdf(paths: &[PathBuf], output_dir: Option<&Path>) -> Result<PathBuf, String> {
    if paths.is_empty() {
        return Err("Select at least one image.".into());
    }
    let mut document = PdfDocument::new("Shift images");
    let mut pages = Vec::new();
    for path in paths {
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        let image = RawImage::decode_from_bytes(&bytes, &mut Vec::new())
            .map_err(|error| format!("Could not decode {}: {error}", path.display()))?;
        let width_mm = image.width as f32 * 25.4 / 300.0;
        let height_mm = image.height as f32 * 25.4 / 300.0;
        let image_id = document.add_image(&image);
        pages.push(PdfPage::new(
            Mm(width_mm.max(1.0)),
            Mm(height_mm.max(1.0)),
            vec![Op::UseXobject {
                id: image_id,
                transform: XObjectTransform::default(),
            }],
        ));
    }
    document.with_pages(pages);
    let output = unique_output_in(&paths[0], output_dir, " (images)", "pdf")?;
    let mut warnings = Vec::new();
    let bytes = document.save(&printpdf::PdfSaveOptions::default(), &mut warnings);
    fs::write(&output, bytes).map_err(|error| error.to_string())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn make_pdf(path: &Path, pages: usize) {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let mut kids = Vec::new();
        for index in 0..pages {
            let content = format!("BT /F1 18 Tf 72 720 Td (Page {}) Tj ET", index + 1);
            let content_id =
                document.add_object(lopdf::Stream::new(dictionary! {}, content.into_bytes()));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Contents" => content_id,
                "Resources" => dictionary! {
                    "Font" => dictionary! { "F1" => font_id },
                },
            });
            kids.push(Object::Reference(page_id));
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => pages as i64,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document.save(path).unwrap();
    }

    /// A one-page PDF embedding an uncompressed RGB image of the given size.
    fn make_pdf_with_image(path: &Path, width: u32, height: u32) {
        let mut document = Document::new();
        let pixels: Vec<u8> = (0..width * height)
            .flat_map(|index| {
                let value = ((index % 251) as u8).wrapping_mul(3);
                [value, value / 2, 255 - value]
            })
            .collect();
        let image_id = document.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => width as i64,
                "Height" => height as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8_i64,
            },
            pixels,
        ));
        let content = "q 612 0 0 792 0 0 cm /Im0 Do Q".to_string();
        let content_id =
            document.add_object(lopdf::Stream::new(dictionary! {}, content.into_bytes()));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
            "Resources" => dictionary! {
                "XObject" => dictionary! { "Im0" => image_id },
            },
        });
        let pages_id = document.add_object(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        });
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document.save(path).unwrap();
    }

    fn embedded_image(path: &Path) -> (i64, i64, Option<String>) {
        let document = load(path).unwrap();
        for (_, object) in &document.objects {
            if let Object::Stream(stream) = object {
                if is_name(stream.dict.get(b"Subtype"), b"Image") {
                    let filter = stream
                        .dict
                        .get(b"Filter")
                        .ok()
                        .and_then(|value| value.as_name().ok())
                        .map(|name| String::from_utf8_lossy(name).to_string());
                    return (
                        stream.dict.get(b"Width").unwrap().as_i64().unwrap(),
                        stream.dict.get(b"Height").unwrap().as_i64().unwrap(),
                        filter,
                    );
                }
            }
        }
        panic!("no embedded image found");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renders_pdf_pages_to_heic() {
        if pdf_render_bridge().is_none() {
            return;
        }
        let directory =
            std::env::temp_dir().join(format!("shift-heic-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("doc.pdf");
        make_pdf(&input, 2);
        match to_images(&input, "heic", None) {
            Ok(pages) => {
                assert_eq!(pages.len(), 2);
                for page in &pages {
                    assert_eq!(page.extension().and_then(|v| v.to_str()), Some("heic"));
                    let bytes = fs::read(page).unwrap();
                    // ISO BMFF: four-byte length, then "ftyp" and an HEIC brand.
                    let brand = String::from_utf8_lossy(&bytes[4..12]).to_string();
                    assert!(brand.starts_with("ftyp"), "not a BMFF container: {brand}");
                    let heic_brand = String::from_utf8_lossy(&bytes[8..12]).to_string();
                    assert!(
                        matches!(heic_brand.as_str(), "heic" | "heix" | "heim" | "heis" | "mif1" | "msf1"),
                        "unexpected HEIC brand: {heic_brand}"
                    );
                    assert_ne!(&bytes[0..4], b"\x89PNG", "output must not be a renamed PNG");
                }
            }
            // A Mac without HEIC support must fail with a message, not silently.
            Err(error) => assert!(error.contains("HEIC") || error.contains("heic"), "{error}"),
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn compress_resamples_large_embedded_images() {
        let directory =
            std::env::temp_dir().join(format!("shift-pdf-img-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("scan.pdf");
        // 1800 px raw RGB is ~9.7 MB, far above the 1440 px cap.
        make_pdf_with_image(&input, 1800, 1800);
        assert_eq!(embedded_image(&input), (1800, 1800, None));

        let output = compress(&input, None).unwrap();
        let (width, height, filter) = embedded_image(&output);
        assert_eq!(filter.as_deref(), Some("DCTDecode"), "rewritten image must be JPEG");
        assert_eq!(width, i64::from(IMAGE_MAX_SIDE));
        assert_eq!(height, i64::from(IMAGE_MAX_SIDE));
        assert!(
            fs::metadata(&output).unwrap().len() < fs::metadata(&input).unwrap().len() / 4,
            "a raw 9.7 MB embedded image must shrink dramatically"
        );

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn compress_leaves_small_images_untouched() {
        let directory =
            std::env::temp_dir().join(format!("shift-pdf-small-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("small.pdf");
        make_pdf_with_image(&input, 64, 48);

        // Below the cap: compress may still shrink the file structurally, but it
        // must not resample or re-encode an image that is already small.
        let output = compress(&input, None).expect("structural compression should shrink raw pixels");
        let (width, height, filter) = embedded_image(&output);
        assert_eq!((width, height), (64, 48), "small images must keep their dimensions");
        assert_ne!(
            filter.as_deref(),
            Some("DCTDecode"),
            "a small image must not be re-encoded as JPEG"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn merges_splits_rotates_and_extracts_text() {
        let directory = std::env::temp_dir().join(format!("shift-pdf-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.pdf");
        let second = directory.join("second.pdf");
        make_pdf(&first, 2);
        make_pdf(&second, 1);

        let merged = merge(&[first.clone(), second.clone()], None).unwrap();
        assert_eq!(load(&merged).unwrap().get_pages().len(), 3);
        let text = extract_text(&first, None).unwrap();
        assert!(fs::read_to_string(text).unwrap().contains("Page 1"));
        let rotated = rotate(&first, 90, None).unwrap();
        assert!(rotated.exists());
        let split = split(&first, None).unwrap();
        assert_eq!(split.len(), 2);

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn combines_images_into_pdf() {
        let directory =
            std::env::temp_dir().join(format!("shift-images-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let image_path = directory.join("image.png");
        image::RgbaImage::from_pixel(64, 48, image::Rgba([240, 90, 36, 255]))
            .save_with_format(&image_path, image::ImageFormat::Png)
            .unwrap();
        let output = images_to_pdf(&[image_path], None).unwrap();
        assert_eq!(load(&output).unwrap().get_pages().len(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn pdf_outputs_can_use_a_custom_folder() {
        let directory =
            std::env::temp_dir().join(format!("shift-pdf-out-{}", uuid::Uuid::new_v4()));
        let output_dir = directory.join("Shift");
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("input.pdf");
        make_pdf(&input, 2);

        let rotated = rotate(&input, 90, Some(&output_dir)).unwrap();
        assert_eq!(rotated.parent(), Some(output_dir.as_path()));
        let split = split(&input, Some(&output_dir)).unwrap();
        assert_eq!(split.len(), 2);
        assert!(split
            .iter()
            .all(|page| page.ancestors().any(|path| path == output_dir.as_path())));

        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renders_pdf_pages_with_native_helper() {
        if pdf_render_bridge().is_none() {
            return;
        }
        let directory =
            std::env::temp_dir().join(format!("shift-render-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("render.pdf");
        make_pdf(&input, 2);
        let pages = to_images(&input, "jpg", None).unwrap();
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(|page| page.exists()));
        fs::remove_dir_all(directory).unwrap();
    }
}
