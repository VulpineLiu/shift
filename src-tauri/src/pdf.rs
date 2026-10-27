use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
#[cfg(not(target_os = "macos"))]
use std::{
    process::Stdio,
    time::{Duration, Instant},
};

use image::GenericImageView;
use lopdf::{Document, Object, ObjectId};
use printpdf::{Mm, Op, PdfDocument, PdfPage, RawImage, XObjectTransform};
use zip::ZipArchive;

use crate::files::unique_output_in;

fn load(path: &Path) -> Result<Document, String> {
    Document::load(path).map_err(|error| format!("Could not read {}: {error}", path.display()))
}

fn page_numbers(doc: &Document) -> Vec<u32> {
    doc.get_pages().keys().copied().collect()
}

/// The JPEG pages are written by the renderer, so a failure here must be
/// reported instead of leaving a body-less or zero-page DOCX behind.
fn remove_on_error<T>(path: &Path, result: Result<T, String>) -> Result<T, String> {
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

/// Page content is normally only a few KiB even for an image-heavy page: the
/// pixels live in XObjects, not in the content stream. A much larger stream is
/// either malformed or a decompression bomb, and must not be inflated without
/// a bound.
const MAX_PAGE_CONTENT: usize = 16 * 1024 * 1024;
/// Poppler opens the document once for a whole batch. The timeout therefore
/// applies to a lack of new rendered pages, not to the entire document: a
/// hundred-page scan is slow but healthy, while a renderer stuck on one page
/// still fails without leaving the UI processing forever.
#[cfg(not(target_os = "macos"))]
const PDF_RENDER_STALL_TIMEOUT: Duration = Duration::from_secs(90);
/// Longest rendered page edge in pixels. Poppler's `-scale-to` is an edge
/// length, not a total-pixel budget: passing a pixel budget here would allow a
/// single scan page to become tens of thousands of pixels wide and exhaust
/// memory. 1800 px keeps Letter-sized pages at roughly 125-220 DPI while
/// bounding pathological pages before they reach GDI.
#[cfg(not(target_os = "macos"))]
const PDF_RENDER_MAX_EDGE: &str = "1800";

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
    let mut text = String::new();
    for page in pages {
        match document.extract_text_with_limit(&[page], MAX_PAGE_CONTENT) {
            Ok(page_text) => {
                text.push_str(&page_text);
                if !page_text.ends_with('\n') {
                    text.push('\n');
                }
            }
            Err(lopdf::Error::Decompress(
                lopdf::DecompressError::MemoryLimitExceeded { .. },
            )) => {
                return Err(
                    "A page in this PDF is too large to extract safely. Convert it to Word to keep the page images."
                        .into(),
                )
            }
            Err(error) => {
                return Err(format!("Could not extract PDF text: {error}"));
            }
        }
    }
    if text.trim().is_empty() {
        return Err("This PDF has no text layer. Use OCR instead for scanned PDFs.".into());
    }
    Ok(text)
}

/// PDF-to-Word keeps text selectable unless the document actually contains
/// raster content. Looking at the object graph first avoids running a text
/// extraction over the large image streams that make scanned PDFs slow.
pub fn has_embedded_images(input: &Path) -> Result<bool, String> {
    #[cfg(not(target_os = "macos"))]
    if let Some(pdfimages) = crate::files::resolve_tool("pdfimages") {
        if let Ok(has_images) = pdfimages_has_images(&pdfimages, input) {
            return Ok(has_images);
        }
    }

    let document = load(input)?;
    Ok(document.objects.values().any(|object| {
        matches!(
            object,
            Object::Stream(stream)
                if is_name(stream.dict.get(b"Subtype"), b"Image")
        )
    }))
}

#[cfg(not(target_os = "macos"))]
fn pdfimages_has_images(pdfimages: &Path, input: &Path) -> Result<bool, String> {
    let output = crate::process::output_with_timeout(
        pdfimages,
        &["-list", input.to_string_lossy().as_ref()],
        Duration::from_secs(15),
    )?;
    if !output.status.success() {
        return Err("The PDF image listing command failed.".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|page| page.parse::<u32>().is_ok())
    }))
}

/// Converts a PDF to DOCX by rendering complete pages. Keeping the page bitmap
/// preserves diagrams, screenshots and vector artwork without first walking
/// every image stream in the PDF, which is prohibitively slow for scan-heavy
/// files on Windows. Pages stream into the package one at a time so a long
/// document does not need a second full-size copy in memory.
pub fn to_docx(input: &Path, output: &Path) -> Result<(), String> {
    to_docx_with_progress(input, output, |_, _, _| {})
}

/// Page-level progress lets the ring say which page is running, so a long scan
/// does not look frozen even though the work is already off the UI thread.
pub fn to_docx_with_progress(
    input: &Path,
    output: &Path,
    mut on_progress: impl FnMut(u32, u32, bool),
) -> Result<(), String> {
    let _ = fs::remove_file(output);
    let work = std::env::temp_dir().join(format!("shift-pdf-docx-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&work).map_err(|error| error.to_string())?;
    let result = (|| {
        let pages = render_pdf_to_docx_pages(input, &work, &mut on_progress)?;
        on_progress(pages.len() as u32, pages.len() as u32, true);
        write_image_docx(input, &pages, output)?;
        let file = fs::File::open(output)
            .map_err(|error| format!("The converter did not create a document: {error}"))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|error| format!("The Word document is invalid: {error}"))?;
        archive
            .by_name("word/document.xml")
            .map_err(|_| "The Word document has no document body.".to_string())?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&work);
    remove_on_error(output, result)
}

fn render_pdf_to_docx_pages(
    input: &Path,
    folder: &Path,
    on_page: &mut impl FnMut(u32, u32, bool),
) -> Result<Vec<PathBuf>, String> {
    let pages = to_docx_page_images(input, folder, on_page)?;
    if pages.is_empty() {
        return Err("No PDF pages were rendered.".into());
    }
    let mut stream = fs::File::open(&pages[0]).map_err(|error| error.to_string())?;
    let mut signature = [0_u8; 2];
    stream
        .read_exact(&mut signature)
        .map_err(|error| format!("The PDF renderer returned an unreadable page: {error}"))?;
    if signature != [0xff, 0xd8] {
        return Err("The PDF renderer did not return JPEG pages.".into());
    }
    Ok(pages)
}

/// Render the whole document with one renderer process. Poppler writes each
/// page as it finishes, so progress can be read from the output folder without
/// rasterising the entire document in memory. Restarting `pdftoppm` once per
/// page reparses the same image-heavy PDF dozens of times and was the reason a
/// scan-heavy PDF appeared to hang on Windows.
fn to_docx_page_images(
    input: &Path,
    folder: &Path,
    on_page: &mut impl FnMut(u32, u32, bool),
) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let page_count = pdf_page_count(input)?;
    if page_count == 0 {
        return Err("No PDF pages were rendered.".into());
    }

    #[cfg(target_os = "macos")]
    {
        let pages = to_images_in(input, "jpg", folder)?;
        for page in 1..=pages.len() as u32 {
            on_page(page, pages.len() as u32, false);
        }
        Ok(pages)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let pdftoppm = renderer_path().ok_or_else(|| {
            "PDF page rendering needs Poppler (pdftoppm) on Windows and Linux.".to_string()
        })?;
        let prefix = folder.join("Page");
        let mut command = crate::files::background_command(&pdftoppm);
        command
            .args([
                "-jpeg",
                "-r",
                "120",
                "-scale-to",
                PDF_RENDER_MAX_EDGE,
                input.to_string_lossy().as_ref(),
                prefix.to_string_lossy().as_ref(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|error| format!("Could not start the PDF renderer: {error}"))?;
        let mut reported = 0_u32;
        let mut last_change = Instant::now();
        // Keep trying to collect the renderer's exit status for a short while
        // after killing it. On Windows a busy Poppler process can take a moment
        // to disappear, and blocking forever in `wait` would freeze the worker.
        let stop = |child: &mut std::process::Child| {
            let _ = child.kill();
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        };
        loop {
            let rendered = rendered_jpegs(folder);
            if rendered.len() as u32 > reported {
                reported = rendered.len() as u32;
                last_change = Instant::now();
                on_page(reported.min(page_count), page_count, false);
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => {
                    stop(&mut child);
                    return Err(format!(
                        "PDF page rendering stopped after {reported} of {page_count} pages ({status})."
                    ));
                }
                Ok(None)
                    if last_change.elapsed() >= Duration::from_secs(15) && rendered.is_empty() =>
                {
                    stop(&mut child);
                    return Err("PDF rendering did not start within 15 seconds.".into());
                }
                Ok(None) if last_change.elapsed() >= PDF_RENDER_STALL_TIMEOUT => {
                    stop(&mut child);
                    return Err(format!(
                        "PDF rendering stopped responding after {reported} of {page_count} pages."
                    ));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(error) => {
                    stop(&mut child);
                    return Err(format!("Could not monitor the PDF renderer: {error}"));
                }
            }
        }
        let mut pages = rendered_jpegs(folder);
        if pages.len() != page_count as usize {
            return Err(format!(
                "The PDF renderer returned {} of {page_count} pages.",
                pages.len()
            ));
        }
        pages.sort_by_key(|path| page_number_in_name(path));
        if reported < page_count {
            on_page(page_count, page_count, false);
        }
        Ok(pages)
    }
}

fn pdf_page_count(input: &Path) -> Result<u32, String> {
    #[cfg(not(target_os = "macos"))]
    if let Some(pdfinfo) = crate::files::resolve_tool("pdfinfo") {
        if let Ok(output) = crate::process::output_with_timeout(
            pdfinfo,
            &[input.to_string_lossy().as_ref()],
            Duration::from_secs(15),
        ) {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                if let Some(pages) = parse_pdfinfo_pages(&text) {
                    return Ok(pages);
                }
            }
        }
    }
    Ok(load(input)?.get_pages().len() as u32)
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn parse_pdfinfo_pages(output: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Pages:")
            .and_then(|pages| pages.trim().parse().ok())
            .filter(|pages| *pages > 0)
    })
}

#[cfg(not(target_os = "macos"))]
fn rendered_jpegs(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("jpg"))
        })
        .collect()
}

fn write_image_docx(input: &Path, pages: &[PathBuf], output: &Path) -> Result<(), String> {
    use std::io::Write as _;
    use zip::{write::SimpleFileOptions, ZipWriter};

    let dimensions: Vec<(u32, u32)> = pages
        .iter()
        .map(|path| {
            let reader = image::ImageReader::open(path)
                .map_err(|error| error.to_string())?
                .with_guessed_format()
                .map_err(|error| error.to_string())?;
            let (width, height) = reader
                .into_dimensions()
                .or_else(|_| {
                    let mut file = fs::File::open(path)?;
                    let mut bytes = Vec::new();
                    file.read_to_end(&mut bytes)?;
                    let image = image::load_from_memory(&bytes)?;
                    Ok::<_, image::ImageError>(image.dimensions())
                })
                .map_err(|error| error.to_string())?;
            Ok((width, height))
        })
        .collect::<Result<_, String>>()?;
    let first = *dimensions
        .first()
        .ok_or_else(|| "No PDF pages were rendered.".to_string())?;
    let short_side = 11_906.0;
    let ratio = first.1 as f64 / first.0 as f64;
    let (page_width, page_height) = if ratio >= 1.0 {
        let width = short_side as u32;
        let height = (short_side * ratio).round() as u32;
        (width, height)
    } else {
        let height = short_side as u32;
        let width = (short_side / ratio).round() as u32;
        (width, height)
    };

    let title = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("PDF");
    let mut document = String::with_capacity(pages.len() * 2600);
    document.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
         xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
         xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><w:body>",
    );
    let orientation = if page_width > page_height {
        "landscape"
    } else {
        "portrait"
    };
    let section = format!(
        "<w:sectPr><w:type w:val=\"nextPage\"/>\
         <w:pgSz w:w=\"{page_width}\" w:h=\"{page_height}\" w:orient=\"{orientation}\"/>\
         <w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" \
         w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
         <w:cols w:space=\"0\"/></w:sectPr>"
    );
    for (index, (width, height)) in dimensions.into_iter().enumerate() {
        // Leave a fraction of a point for Word's inline-image baseline so a
        // full-page image cannot spill into and collapse the next section.
        let usable_height = page_height.saturating_sub(4);
        let scale = (page_width as f64 / width as f64).min(usable_height as f64 / height as f64);
        let image_width = (width as f64 * scale).floor().max(1.0) as u64;
        let image_height = (height as f64 * scale).floor().max(1.0) as u64;
        let extent_x = image_width * 635;
        let extent_y = image_height * 635;
        let relationship = index + 1;
        let page_number = index + 1;
        let page_section = if page_number < pages.len() {
            section.as_str()
        } else {
            ""
        };
        document.push_str(&format!(
            "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" \
             w:lineRule=\"auto\"/>{page_section}</w:pPr><w:r><w:drawing>\
             <wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
             <wp:extent cx=\"{extent_x}\" cy=\"{extent_y}\"/>\
             <wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>\
             <wp:docPr id=\"{page_number}\" name=\"Page {page_number}\" \
             descr=\"PDF page {page_number}\"/>\
             <wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/>\
             </wp:cNvGraphicFramePr><a:graphic>\
             <a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
             <pic:pic><pic:nvPicPr><pic:cNvPr id=\"{page_number}\" name=\"Page {page_number}\"/>\
             <pic:cNvPicPr/></pic:nvPicPr><pic:blipFill>\
             <a:blip r:embed=\"rId{relationship}\"/><a:stretch><a:fillRect/></a:stretch>\
             </pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"{extent_x}\" cy=\"{extent_y}\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
             </a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        ));
    }
    document.push_str(&format!(
        "<w:sectPr><w:pgSz w:w=\"{page_width}\" w:h=\"{page_height}\" w:orient=\"{}\"/>\
         <w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" \
         w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>",
        orientation
    ));

    let mut relationships = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for index in 0..pages.len() {
        relationships.push_str(&format!(
            "<Relationship Id=\"rId{}\" \
             Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" \
             Target=\"media/page-{:03}.jpg\"/>",
            index + 1,
            index + 1
        ));
    }
    relationships.push_str("</Relationships>");

    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
        <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
        <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
        <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
        <Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>\
        <Override PartName=\"/word/document.xml\" \
        ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
        <Override PartName=\"/docProps/core.xml\" \
        ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
        <Override PartName=\"/docProps/app.xml\" \
        ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>\
        </Types>";
    let root_relationships = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
        <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
        <Relationship Id=\"rId1\" \
        Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" \
        Target=\"word/document.xml\"/>\
        <Relationship Id=\"rId2\" \
        Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" \
        Target=\"docProps/core.xml\"/>\
        <Relationship Id=\"rId3\" \
        Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" \
        Target=\"docProps/app.xml\"/></Relationships>";
    let escaped_title = xml_escape(title);
    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
         <dc:title>{escaped_title}</dc:title><dc:creator>Shift</dc:creator>\
         <cp:lastModifiedBy>Shift</cp:lastModifiedBy></cp:coreProperties>"
    );
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
        <Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\">\
        <Application>Shift</Application></Properties>";

    let file = fs::File::create(output).map_err(|error| error.to_string())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for (name, value) in [
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_relationships.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", relationships.as_bytes()),
        ("docProps/core.xml", core.as_bytes()),
        ("docProps/app.xml", app.as_bytes()),
    ] {
        zip.start_file(name, options)
            .map_err(|error| error.to_string())?;
        zip.write_all(value).map_err(|error| error.to_string())?;
    }
    for (index, path) in pages.iter().enumerate() {
        zip.start_file(format!("word/media/page-{:03}.jpg", index + 1), options)
            .map_err(|error| error.to_string())?;
        // Copy each rendered page straight into the ZIP. Reading every page
        // into a Vec first doubles memory use on scans and can stall Windows.
        let mut page = fs::File::open(path).map_err(|error| error.to_string())?;
        std::io::copy(&mut page, &mut zip).map_err(|error| error.to_string())?;
    }
    zip.finish().map_err(|error| error.to_string())?;
    Ok(())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
    dict.set(
        "ColorSpace",
        if is_gray { "DeviceGray" } else { "DeviceRGB" },
    );
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
    for key in [
        b"SMask".as_slice(),
        b"Mask".as_slice(),
        b"Alternates".as_slice(),
    ] {
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
        [b"DCTDecode"] => image::load_from_memory(&stream.content)
            .ok()
            .map(|image| (image, is_gray)),
        // Raw or deflate-packed samples: rebuild from the pixel array.
        [] | [b"FlateDecode"] => {
            let bytes_per_pixel = if is_gray { 1 } else { 3 };
            let decoded = stream.get_plain_content().ok()?;
            if decoded.len() != (width as usize) * (height as usize) * bytes_per_pixel {
                return None;
            }
            let image = if is_gray {
                image::GrayImage::from_raw(width, height, decoded)
                    .map(image::DynamicImage::ImageLuma8)
            } else {
                image::RgbImage::from_raw(width, height, decoded)
                    .map(image::DynamicImage::ImageRgb8)
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
pub(crate) fn pdf_render_bridge() -> Option<PathBuf> {
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

/// The executable that provides PDF page rendering for the current platform.
pub(crate) fn renderer_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        pdf_render_bridge()
    }
    #[cfg(not(target_os = "macos"))]
    {
        crate::files::resolve_tool("pdftoppm")
    }
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
        "heic" | "heif" => (
            "png",
            Some(if requested == "heif" { "heif" } else { "heic" }),
        ),
        other => return Err(format!("Unsupported PDF image format: {other}")),
    };
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;

    #[cfg(target_os = "macos")]
    {
        let bridge = renderer_path().ok_or_else(|| {
            "The PDF renderer is missing. Run pnpm mac:drag-bridge and rebuild.".to_string()
        })?;
        let status = crate::files::background_command(&bridge)
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
        let pdftoppm = renderer_path().ok_or_else(|| {
            "PDF page rendering needs Poppler (pdftoppm) on Windows and Linux.".to_string()
        })?;
        let flag = match extension {
            "jpg" => "-jpeg",
            "tif" => "-tiff",
            _ => "-png",
        };
        let prefix = folder.join("Page");
        let status = crate::process::status_with_timeout(
            pdftoppm,
            &[
                flag,
                "-r",
                "144",
                "-scale-to",
                PDF_RENDER_MAX_EDGE,
                input.to_string_lossy().as_ref(),
                prefix.to_string_lossy().as_ref(),
            ],
            const { std::time::Duration::from_secs(60 * 8) },
        )?;
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
        images.sort_by_key(|path| page_number_in_name(path));
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
    images.sort_by_key(|path| page_number_in_name(path));
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

fn page_number_in_name(path: &Path) -> u32 {
    path.file_stem()
        .and_then(|value| value.to_str())
        .and_then(|value| {
            value
                .rsplit(|character: char| !character.is_ascii_digit())
                .next()
        })
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(u32::MAX)
}

pub fn images_to_pdf(paths: &[PathBuf], output_dir: Option<&Path>) -> Result<PathBuf, String> {
    images_to_pdf_with_progress(paths, output_dir, |_, _| {})
}

fn image_for_pdf(path: &Path) -> Result<RawImage, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    match RawImage::decode_from_bytes(&bytes, &mut Vec::new()) {
        Ok(image) => Ok(image),
        Err(decode_error) => {
            let converted =
                std::env::temp_dir().join(format!("shift-pdf-image-{}.png", uuid::Uuid::new_v4()));
            if let Err(convert_error) = crate::files::system_convert(path, &converted, "png") {
                return Err(format!(
                    "Could not decode {} ({decode_error}). {convert_error}",
                    path.display()
                ));
            }
            let result = fs::read(&converted)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    RawImage::decode_from_bytes(&bytes, &mut Vec::new())
                        .map_err(|error| format!("Could not decode {}: {error}", path.display()))
                });
            let _ = fs::remove_file(converted);
            result
        }
    }
}

/// Builds a PDF page by page so every document path that already has page
/// images can report the same progress event as PDF-to-Word.
pub fn images_to_pdf_with_progress(
    paths: &[PathBuf],
    output_dir: Option<&Path>,
    mut on_page: impl FnMut(u32, u32),
) -> Result<PathBuf, String> {
    if paths.is_empty() {
        return Err("Select at least one image.".into());
    }
    let mut document = PdfDocument::new("Shift images");
    let mut pages = Vec::new();
    let total = paths.len() as u32;
    for (index, path) in paths.iter().enumerate() {
        let image = image_for_pdf(path)?;
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
        on_page(index as u32 + 1, total);
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

    /// A PDF embedding an uncompressed RGB image on each page.
    fn make_pdf_with_image(path: &Path, width: u32, height: u32, pages: usize) {
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
        let pages_id = document.new_object_id();
        let mut kids = Vec::new();
        for _ in 0..pages {
            let content = "q 612 0 0 792 0 0 cm /Im0 Do Q".to_string();
            let content_id =
                document.add_object(lopdf::Stream::new(dictionary! {}, content.into_bytes()));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Contents" => content_id,
                "Resources" => dictionary! {
                    "XObject" => dictionary! { "Im0" => image_id },
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

    fn embedded_image(path: &Path) -> (i64, i64, Option<String>) {
        let document = load(path).unwrap();
        for object in document.objects.values() {
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
        let directory = std::env::temp_dir().join(format!("shift-heic-{}", uuid::Uuid::new_v4()));
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
                        matches!(
                            heic_brand.as_str(),
                            "heic" | "heix" | "heim" | "heis" | "mif1" | "msf1"
                        ),
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
        make_pdf_with_image(&input, 1800, 1800, 1);
        assert_eq!(embedded_image(&input), (1800, 1800, None));

        let output = compress(&input, None).unwrap();
        let (width, height, filter) = embedded_image(&output);
        assert_eq!(
            filter.as_deref(),
            Some("DCTDecode"),
            "rewritten image must be JPEG"
        );
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
        make_pdf_with_image(&input, 64, 48, 1);

        // Below the cap: compress may still shrink the file structurally, but it
        // must not resample or re-encode an image that is already small.
        let output =
            compress(&input, None).expect("structural compression should shrink raw pixels");
        let (width, height, filter) = embedded_image(&output);
        assert_eq!(
            (width, height),
            (64, 48),
            "small images must keep their dimensions"
        );
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
        let directory = std::env::temp_dir().join(format!("shift-images-{}", uuid::Uuid::new_v4()));
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
        let directory = std::env::temp_dir().join(format!("shift-render-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("render.pdf");
        make_pdf(&input, 2);
        let pages = to_images(&input, "jpg", None).unwrap();
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(|page| page.exists()));
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn converts_heic_images_to_pdf() {
        let directory =
            std::env::temp_dir().join(format!("shift-heic-pdf-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let png = directory.join("source.png");
        let image = image::RgbImage::from_pixel(32, 24, image::Rgb([30, 120, 210]));
        image.save(&png).unwrap();
        let heic = directory.join("source.heic");
        if crate::files::system_convert(&png, &heic, "heic").is_err() {
            fs::remove_dir_all(directory).unwrap();
            return;
        }

        let output = images_to_pdf(&[heic], None).unwrap();
        assert!(fs::read(&output).unwrap().starts_with(b"%PDF"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn converts_pdf_to_docx_with_rendered_page_images() {
        use std::{fs::File, io::Read as _};

        if renderer_path().is_none() {
            return;
        }
        let directory = std::env::temp_dir().join(format!("shift-docx-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("report.pdf");
        let output = directory.join("report.docx");
        make_pdf(&input, 2);
        let mut progress = Vec::new();
        to_docx_with_progress(&input, &output, |done, total, packaging| {
            progress.push((done, total, packaging))
        })
        .unwrap();
        assert_eq!(progress, vec![(1, 2, false), (2, 2, false), (2, 2, true)]);
        let mut archive = ZipArchive::new(File::open(&output).unwrap()).unwrap();
        let media_pages = (0..archive.len())
            .filter(|index| {
                archive
                    .by_index(*index)
                    .map(|entry| entry.name().starts_with("word/media/page-"))
                    .unwrap_or(false)
            })
            .count();
        let mut body = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut body)
            .unwrap();
        assert!(body.contains("Page"), "{body}");
        assert!(body.contains("<wp:inline"), "{body}");
        assert!(
            !body.contains("w:lineRule=\"exact\""),
            "full-page images need an automatic line height or Word clips them"
        );
        assert!(body.contains("w:lineRule=\"auto\""), "{body}");
        assert_eq!(body.matches("<w:type w:val=\"nextPage\"/>").count(), 1);
        assert_eq!(media_pages, 2);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn detects_whether_a_pdf_needs_page_rendering() {
        let directory =
            std::env::temp_dir().join(format!("shift-pdf-images-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();

        let text = directory.join("text.pdf");
        make_pdf(&text, 1);
        assert!(!has_embedded_images(&text).unwrap());

        let scan = directory.join("scan.pdf");
        make_pdf_with_image(&scan, 40, 40, 1);
        assert!(has_embedded_images(&scan).unwrap());

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn poppler_page_count_is_read_from_pdfinfo() {
        let listing = "Title: report\nPages:          12\nEncrypted: no\n";
        assert_eq!(parse_pdfinfo_pages(listing), Some(12));
        assert_eq!(parse_pdfinfo_pages("Pages: 0\n"), None);
        assert_eq!(parse_pdfinfo_pages("Title: report\n"), None);
    }

    #[test]
    fn image_heavy_pdf_to_docx_completes_without_text_extraction() {
        use std::time::{Duration, Instant};

        if renderer_path().is_none() {
            return;
        }
        let directory =
            std::env::temp_dir().join(format!("shift-docx-image-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("scan.pdf");
        let output = directory.join("scan.docx");
        // The page contains no text layer and a large embedded image. A text
        // extractor would either return nothing or spend its time inflating the
        // image stream; the page-rendering path must not depend on it.
        make_pdf_with_image(&input, 900, 1200, 3);
        let started = Instant::now();
        let mut progress = Vec::new();
        to_docx_with_progress(&input, &output, |done, total, packaging| {
            progress.push((done, total, packaging))
        })
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "conversion took too long"
        );
        assert_eq!(
            progress,
            vec![(1, 3, false), (2, 3, false), (3, 3, false), (3, 3, true)]
        );
        assert!(output.exists());
        let archive = ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
        assert!(archive
            .file_names()
            .any(|name| name.starts_with("word/media/page-")));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn text_extraction_rejects_an_oversized_page_without_inflating_it() {
        use std::io::Write as _;

        let directory =
            std::env::temp_dir().join(format!("shift-pdf-bomb-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("bomb.pdf");
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder
            .write_all(&vec![0_u8; MAX_PAGE_CONTENT + 1])
            .unwrap();
        let compressed = encoder.finish().unwrap();

        let mut document = Document::new();
        let pages_id = document.new_object_id();
        let content_id = document.add_object(lopdf::Stream::new(
            dictionary! { "Filter" => "FlateDecode" },
            compressed,
        ));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1_i64,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document.save(&input).unwrap();

        let error = text_of(&input).unwrap_err();
        assert!(error.contains("too large"), "{error}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rendered_page_names_sort_numerically_not_lexically() {
        let mut pages = [
            PathBuf::from("Page-1.jpg"),
            PathBuf::from("Page-10.jpg"),
            PathBuf::from("Page-2.jpg"),
        ];
        pages.sort_by_key(|path| page_number_in_name(path));
        assert_eq!(pages[0], PathBuf::from("Page-1.jpg"));
        assert_eq!(pages[1], PathBuf::from("Page-2.jpg"));
        assert_eq!(pages[2], PathBuf::from("Page-10.jpg"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn renderer_limit_is_an_edge_length_not_a_pixel_count() {
        let edge: u32 = PDF_RENDER_MAX_EDGE.parse().unwrap();
        // Passing a total-pixel budget to `pdftoppm -scale-to` would allow one
        // page edge to grow into the tens of thousands and exhaust memory.
        assert!(
            (1000..=4096).contains(&edge),
            "unexpected renderer edge: {edge}"
        );
    }
}
