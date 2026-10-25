use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use calamine::{open_workbook_auto, Reader};
use quick_xml::{events::Event, Reader as XmlReader};
use zip::ZipArchive;

use crate::files::unique_output_in;

/// Extracts a DOCX whose body consists only of images, in document order.
///
/// AppKit's OOXML reader drops inline drawings from page-scan documents, which
/// used to turn a 12-page file into one blank PDF page. Restricting this path
/// to image-only bodies keeps normal text documents on the native renderer.
pub fn document_page_images(
    input: &Path,
    output_dir: &Path,
) -> Result<Option<Vec<PathBuf>>, String> {
    let file = fs::File::open(input).map_err(|error| error.to_string())?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("Could not read document: {error}"))?;
    let relationships = {
        let mut xml = String::new();
        archive
            .by_name("word/_rels/document.xml.rels")
            .map_err(|error| format!("Could not read document links: {error}"))?
            .read_to_string(&mut xml)
            .map_err(|error| error.to_string())?;
        document_image_links(&xml)?
    };
    let document = {
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .map_err(|error| format!("Could not read document body: {error}"))?
            .read_to_string(&mut xml)
            .map_err(|error| error.to_string())?;
        xml
    };
    let (image_ids, has_text) = document_body_summary(&document)?;
    if has_text || image_ids.is_empty() {
        return Ok(None);
    }

    let mut images = Vec::with_capacity(image_ids.len());
    for (index, relationship) in image_ids.into_iter().enumerate() {
        let Some(target) = relationships.get(&relationship) else {
            return Ok(None);
        };
        let archive_path = if target.starts_with("word/") {
            target.clone()
        } else {
            format!("word/{}", target.trim_start_matches('/'))
        };
        if archive_path
            .split('/')
            .any(|part| part == ".." || part.is_empty())
        {
            return Ok(None);
        }
        let extension = Path::new(target)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("img")
            .to_ascii_lowercase();
        let destination = output_dir.join(format!("page-{:03}.{extension}", index + 1));
        let mut entry = match archive.by_name(&archive_path) {
            Ok(entry) => entry,
            Err(_) => return Ok(None),
        };
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        let mut output = fs::File::create(&destination).map_err(|error| error.to_string())?;
        output
            .write_all(&bytes)
            .map_err(|error| error.to_string())?;
        images.push(destination);
    }
    Ok(Some(images))
}

fn document_image_links(xml: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut reader = XmlReader::from_str(xml);
    let mut links = std::collections::BTreeMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element))
                if local_name(element.name().as_ref()) == "Relationship" =>
            {
                let kind = attribute_value(&element, "Type").unwrap_or_default();
                if let (Some(id), Some(target)) = (
                    attribute_value(&element, "Id"),
                    attribute_value(&element, "Target"),
                ) {
                    if kind.ends_with("/image") {
                        links.insert(id, target);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse document links: {error}")),
        }
    }
    Ok(links)
}

fn document_body_summary(xml: &str) -> Result<(Vec<String>, bool), String> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut images = Vec::new();
    let mut has_text = false;
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                match local_name(element.name().as_ref()) {
                    "blip" => {
                        if let Some(id) = attribute_value(&element, "embed") {
                            images.push(id);
                        }
                    }
                    "t" => in_text = true,
                    _ => {}
                }
            }
            Ok(Event::Text(text)) if in_text => {
                let text = quick_xml::escape::unescape(&text)
                    .map_err(|error| format!("Could not decode document text: {error}"))?;
                if !text.trim().is_empty() {
                    has_text = true;
                }
            }
            Ok(Event::CData(text)) if in_text => {
                if !text.trim().is_empty() {
                    has_text = true;
                }
            }
            Ok(Event::End(element)) if local_name(element.name().as_ref()) == "t" => {
                in_text = false;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse document body: {error}")),
        }
    }
    Ok((images, has_text))
}

fn clean_cell(value: &calamine::Data) -> String {
    value
        .to_string()
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string()
}

pub fn presentation_to_markdown(
    input: &Path,
    output_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    if extension_is(input, "ppt") || extension_is(input, "key") {
        return Err("Save the presentation as PPTX before converting it.".into());
    }
    let file = fs::File::open(input).map_err(|error| error.to_string())?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("Could not read presentation: {error}"))?;
    let mut slides = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Could not read presentation entry: {error}"))?;
        let name = entry.name().to_string();
        if !name.starts_with("ppt/slides/slide") || !name.ends_with(".xml") {
            continue;
        }
        let number = name
            .trim_start_matches("ppt/slides/slide")
            .trim_end_matches(".xml")
            .parse::<u32>()
            .unwrap_or(index as u32);
        let mut xml = String::new();
        entry
            .read_to_string(&mut xml)
            .map_err(|error| error.to_string())?;
        slides.push((number, slide_blocks(&xml)?));
    }
    slides.sort_by_key(|(number, _)| *number);
    if slides.is_empty() {
        return Err("No slides were found in this presentation.".into());
    }
    let mut markdown = String::new();
    for (number, blocks) in slides {
        markdown.push_str(&format!("## Slide {number}\n\n"));
        if blocks.is_empty() {
            markdown.push_str("_No text on this slide._\n\n");
            continue;
        }
        for block in blocks {
            match block {
                SlideBlock::Paragraph(text) => {
                    markdown.push_str(&text);
                    markdown.push_str("\n\n");
                }
                SlideBlock::Table(rows) => markdown.push_str(&markdown_table(rows)),
            }
        }
    }
    let output = unique_output_in(input, output_dir, " (markdown)", "md")?;
    fs::write(&output, markdown).map_err(|error| error.to_string())?;
    Ok(output)
}

fn extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

/// One renderable region of a slide: a run of text, or a table with its
/// row/column structure intact.
enum SlideBlock {
    Paragraph(String),
    Table(Vec<Vec<String>>),
}

fn local_name(raw: &str) -> &str {
    raw.rsplit(':').next().unwrap_or(raw)
}

/// Reads an attribute value from a start element, ignoring namespace prefixes.
fn attribute_value(element: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    element.attributes().flatten().find_map(|attribute| {
        if local_name(attribute.key.as_ref()) == name {
            attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.to_string())
        } else {
            None
        }
    })
}

/// Resolves an entity reference such as `amp`, `lt` or a numeric `&#8364;`.
fn resolve_entity(reference: &str) -> Option<String> {
    if let Some(character) = quick_xml::events::BytesRef::new(reference)
        .resolve_char_ref()
        .ok()
        .flatten()
    {
        return Some(character.to_string());
    }
    quick_xml::escape::resolve_predefined_entity(reference).map(str::to_string)
}

/// Moves the collected run text into the paragraph or the table cell. Runs
/// that follow each other are separated by a single space, the way the slide
/// reads when it is shown.
fn flush_run(pending: &mut String, target: &mut String) {
    let trimmed = pending.trim().to_string();
    pending.clear();
    if trimmed.is_empty() {
        return;
    }
    if !target.is_empty() && !target.ends_with(' ') {
        target.push(' ');
    }
    target.push_str(&trimmed);
}

/// Moves the pending run text into whichever buffer is currently being read.
fn flush_pending(pending: &mut String, in_table: bool, paragraph: &mut String, cell: &mut String) {
    if in_table {
        flush_run(pending, cell);
    } else {
        flush_run(pending, paragraph);
    }
}

/// Extracts slide content in document order. Text is grouped per `<a:p>` as
/// before; tables are additionally walked row by row so cells do not collapse
/// into a single paragraph of prose.
fn slide_blocks(xml: &str) -> Result<Vec<SlideBlock>, String> {
    let mut reader = XmlReader::from_str(xml);
    // Whitespace inside a run is meaningful: it separates an entity
    // reference from the words around it, so it must not be trimmed away.
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut blocks: Vec<SlideBlock> = Vec::new();
    let mut paragraph = String::new();
    // Text of the run that is currently being read. Entity references and
    // CDATA land in the same buffer, so the spacing around them survives.
    let mut pending = String::new();
    // Table state. Nested tables are not a real PowerPoint layout, so a single
    // level of row/cell buffering is enough.
    let mut in_table = false;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    // A merged cell spans several grid columns, and a horizontally or
    // vertically merged cell is written as a continuation cell. Both would
    // otherwise shift every following column.
    let mut cell_span = 1_usize;
    let mut skip_cell = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) => match local_name(element.name().as_ref()) {
                "tbl" => {
                    flush_pending(&mut pending, false, &mut paragraph, &mut cell);
                    push_paragraph(&mut paragraph, &mut blocks);
                    in_table = true;
                    rows.clear();
                    row.clear();
                }
                "tr" if in_table => row.clear(),
                "tc" if in_table => {
                    flush_pending(&mut pending, true, &mut paragraph, &mut cell);
                    cell.clear();
                    cell_span = attribute_value(&element, "gridSpan")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(1)
                        .max(1);
                    // A horizontally merged cell continues the cell to its
                    // left, so the grid span of that cell already covers it.
                    skip_cell = attribute_value(&element, "hMerge")
                        .map(|value| value.trim() == "1")
                        .unwrap_or(false);
                }
                "p" if !in_table => {
                    flush_pending(&mut pending, false, &mut paragraph, &mut cell);
                    push_paragraph(&mut paragraph, &mut blocks);
                }
                "p" if in_table && !cell.trim().is_empty() => cell.push(' '),
                _ => {}
            },
            Ok(Event::End(element)) => {
                if local_name(element.name().as_ref()) == "t" {
                    flush_pending(&mut pending, in_table, &mut paragraph, &mut cell);
                    continue;
                }
                if !in_table {
                    continue;
                }
                match local_name(element.name().as_ref()) {
                    "tc" => {
                        if !skip_cell {
                            flush_pending(&mut pending, true, &mut paragraph, &mut cell);
                            row.push(std::mem::take(&mut cell));
                            // A spanned cell occupies as many grid columns.
                            for _ in 1..cell_span {
                                row.push(String::new());
                            }
                        }
                        skip_cell = false;
                        cell_span = 1;
                    }
                    "tr" => {
                        if row.iter().any(|value| !value.trim().is_empty()) {
                            rows.push(std::mem::take(&mut row));
                        }
                        row.clear();
                    }
                    "tbl" => {
                        let captured = std::mem::take(&mut rows);
                        if !captured.is_empty() {
                            blocks.push(SlideBlock::Table(captured));
                        }
                        rows.clear();
                        row.clear();
                        in_table = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(value)) => {
                let unescaped = quick_xml::escape::unescape(&value)
                    .map_err(|error| format!("Could not decode slide text: {error}"))?;
                pending.push_str(unescaped.as_ref());
            }
            Ok(Event::CData(value)) => {
                pending.push_str(value.as_ref());
            }
            Ok(Event::GeneralRef(reference)) => {
                let Some(resolved) = resolve_entity(reference.as_ref()) else {
                    continue;
                };
                pending.push_str(&resolved);
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(format!("Could not parse slide XML: {error}")),
            _ => {}
        }
        buffer.clear();
    }
    push_paragraph(&mut paragraph, &mut blocks);
    Ok(blocks)
}

fn push_paragraph(paragraph: &mut String, blocks: &mut Vec<SlideBlock>) {
    let text = paragraph.trim().to_string();
    if !text.is_empty() {
        blocks.push(SlideBlock::Paragraph(escape_markdown_cell(&text)));
    }
    paragraph.clear();
}

/// Renders rows as a GitHub-flavoured Markdown table, padding ragged rows.
fn markdown_table(rows: Vec<Vec<String>>) -> String {
    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|row| row.iter().map(|cell| escape_markdown_cell(cell)).collect())
        .collect();
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return String::new();
    }
    let padded = |row: &Vec<String>| {
        let mut cells = row.clone();
        cells.resize(columns, String::new());
        cells
    };
    let mut output = String::from("|");
    for cell in padded(&rows[0]) {
        output.push_str(&format!(" {cell} |"));
    }
    output.push('\n');
    output.push('|');
    for _ in 0..columns {
        output.push_str(" --- |");
    }
    output.push_str("\n");
    for row in rows.iter().skip(1) {
        output.push('|');
        for cell in padded(row) {
            output.push_str(&format!(" {cell} |"));
        }
        output.push('\n');
    }
    output.push('\n');
    output
}

pub fn spreadsheet_to_markdown(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    if extension_is(input, "csv") || extension_is(input, "tsv") {
        return delimited_to_markdown(input, output_dir);
    }
    if extension_is(input, "numbers") {
        return Err(
            "Numbers files are not supported directly. Export as XLSX or CSV first.".into(),
        );
    }
    let mut workbook = open_workbook_auto(input)
        .map_err(|error| format!("Could not read spreadsheet: {error}"))?;
    let sheet_names = workbook.sheet_names().to_owned();
    if sheet_names.is_empty() {
        return Err("The spreadsheet has no worksheets.".into());
    }
    let mut markdown = String::new();
    for sheet_name in sheet_names {
        let range = match workbook.worksheet_range(&sheet_name) {
            Ok(range) => range,
            Err(error) => return Err(format!("Could not read sheet {sheet_name}: {error}")),
        };
        markdown.push_str(&format!("## {sheet_name}\n\n"));
        let rows: Vec<Vec<String>> = range
            .rows()
            .map(|row| row.iter().map(clean_cell).collect())
            .filter(|row: &Vec<String>| row.iter().any(|cell| !cell.is_empty()))
            .collect();
        if rows.is_empty() {
            markdown.push_str("_Empty sheet_\n\n");
            continue;
        }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let padded = |row: &Vec<String>| {
            let mut cells = row.clone();
            cells.resize(columns, String::new());
            cells
        };
        markdown.push('|');
        for cell in padded(&rows[0]) {
            markdown.push_str(&format!(" {cell} |"));
        }
        markdown.push('\n');
        markdown.push('|');
        for _ in 0..columns {
            markdown.push_str(" --- |");
        }
        markdown.push('\n');
        for row in rows.iter().skip(1) {
            markdown.push('|');
            for cell in padded(row) {
                markdown.push_str(&format!(" {cell} |"));
            }
            markdown.push('\n');
        }
        markdown.push('\n');
    }
    let output = unique_output_in(input, output_dir, " (markdown)", "md")?;
    fs::write(&output, markdown).map_err(|error| error.to_string())?;
    Ok(output)
}

fn delimited_to_markdown(input: &Path, output_dir: Option<&Path>) -> Result<PathBuf, String> {
    let delimiter = if extension_is(input, "tsv") {
        '\t'
    } else {
        ','
    };
    let text = fs::read_to_string(input).map_err(|error| error.to_string())?;
    let text = text.trim_start_matches('\u{feff}');
    let rows = parse_delimited(text, delimiter);
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return Err("The spreadsheet has no rows.".into());
    }
    let mut markdown = String::new();
    for (index, row) in rows.iter().enumerate() {
        let mut cells = row.clone();
        cells.resize(columns, String::new());
        markdown.push('|');
        for cell in cells {
            markdown.push(' ');
            markdown.push_str(&escape_markdown_cell(&cell));
            markdown.push_str(" |");
        }
        markdown.push('\n');
        if index == 0 {
            markdown.push('|');
            for _ in 0..columns {
                markdown.push_str(" --- |");
            }
            markdown.push('\n');
        }
    }
    let output = unique_output_in(input, output_dir, " (markdown)", "md")?;
    fs::write(&output, markdown).map_err(|error| error.to_string())?;
    Ok(output)
}

fn parse_delimited(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cell.push('"');
                let _ = chars.next();
            }
            '"' => quoted = !quoted,
            value if value == delimiter && !quoted => {
                row.push(std::mem::take(&mut cell));
            }
            '\n' if !quoted => {
                if !cell.is_empty() || !row.is_empty() {
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                }
            }
            '\r' if !quoted => {}
            value => cell.push(value),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

fn escape_markdown_cell(value: &str) -> String {
    value
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::{write::SimpleFileOptions, ZipWriter};

    #[test]
    fn escapes_markdown_table_cells() {
        assert_eq!(
            clean_cell(&calamine::Data::String("a|b\nc".into())),
            "a\\|b c"
        );
    }

    #[test]
    fn extracts_pptx_slide_text() {
        let directory = std::env::temp_dir().join(format!("shift-pptx-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("slides.pptx");
        let file = fs::File::create(&input).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("ppt/slides/slide1.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>Hello</a:t></a:r></a:p><a:p><a:r><a:t>World</a:t></a:r></a:p></p:sld>"#,
        )
        .unwrap();
        zip.finish().unwrap();

        let output = presentation_to_markdown(&input, None).unwrap();
        let markdown = fs::read_to_string(output).unwrap();
        assert!(markdown.contains("## Slide 1"));
        assert!(markdown.contains("Hello"));
        assert!(markdown.contains("World"));

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn extracts_pptx_tables_as_markdown() {
        let directory =
            std::env::temp_dir().join(format!("shift-pptx-tbl-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("table.pptx");
        let file = fs::File::create(&input).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("ppt/slides/slide1.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<p:sld xmlns:p="p" xmlns:a="a">
              <a:p><a:r><a:t>Intro</a:t></a:r></a:p>
              <a:tbl><a:tr>
                  <a:tc><a:p><a:r><a:t>Quarter</a:t></a:r></a:p></a:tc>
                  <a:tc><a:p><a:r><a:t>Revenue</a:t></a:r></a:p></a:tc>
              </a:tr><a:tr>
                  <a:tc><a:p><a:r><a:t>Q1</a:t></a:r></a:p></a:tc>
                  <a:tc><a:p><a:r><a:t>1|2</a:t></a:r><a:r><a:t>M</a:t></a:r></a:p></a:tc>
              </a:tr></a:tbl>
              <a:p><a:r><a:t>Outro</a:t></a:r></a:p>
            </p:sld>"#,
        )
        .unwrap();
        zip.finish().unwrap();

        let output = presentation_to_markdown(&input, None).unwrap();
        let markdown = fs::read_to_string(output).unwrap();
        assert!(markdown.contains("Intro"), "{markdown}");
        // Header and separator rows prove structure survived.
        assert!(markdown.contains("| Quarter | Revenue |"), "{markdown}");
        assert!(markdown.contains("| --- | --- |"), "{markdown}");
        assert!(markdown.contains("| Q1 | 1\\|2 M |"), "{markdown}");
        // Text after the table is still emitted.
        assert!(markdown.contains("Outro"), "{markdown}");
        assert!(
            markdown.find("Outro") > markdown.find("| Q1 |"),
            "document order must be preserved"
        );

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn converts_delimited_spreadsheets_to_markdown() {
        let directory =
            std::env::temp_dir().join(format!("shift-delimited-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();

        let csv = directory.join("sample.csv");
        fs::write(&csv, "name,note\nAlpha,\"Hello, world\"\nBeta,a|b\n").unwrap();
        let markdown = fs::read_to_string(spreadsheet_to_markdown(&csv, None).unwrap()).unwrap();
        assert!(markdown.contains("| name | note |"));
        assert!(markdown.contains("| Alpha | Hello, world |"));
        assert!(markdown.contains("| Beta | a\\|b |"));

        let tsv = directory.join("sample.tsv");
        fs::write(&tsv, "name\tvalue\nAlpha\t1\n").unwrap();
        let markdown = fs::read_to_string(spreadsheet_to_markdown(&tsv, None).unwrap()).unwrap();
        assert!(markdown.contains("| name | value |"));
        assert!(markdown.contains("| Alpha | 1 |"));

        fs::remove_dir_all(directory).unwrap();
    }

    /// Builds a one-slide presentation with the given slide XML.
    fn pptx_with_slide(directory: &Path, name: &str, slide_xml: &str) -> PathBuf {
        let input = directory.join(name);
        let file = fs::File::create(&input).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("ppt/slides/slide1.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(slide_xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        input
    }

    #[test]
    fn keeps_xml_entities_in_slide_text() {
        let directory =
            std::env::temp_dir().join(format!("shift-pptx-entity-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = pptx_with_slide(
            &directory,
            "entities.pptx",
            r#"<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>A &amp; B &lt; C &#8364; D</a:t></a:r></a:p></p:sld>"#,
        );

        let output = presentation_to_markdown(&input, None).unwrap();
        let markdown = fs::read_to_string(output).unwrap();
        assert!(markdown.contains("A & B < C € D"), "{markdown}");

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn keeps_cdata_in_slide_text() {
        let directory =
            std::env::temp_dir().join(format!("shift-pptx-cdata-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = pptx_with_slide(
            &directory,
            "cdata.pptx",
            r#"<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>before </a:t></a:r><a:r><a:t><![CDATA[raw & wild]]></a:t></a:r></a:p></p:sld>"#,
        );

        let output = presentation_to_markdown(&input, None).unwrap();
        let markdown = fs::read_to_string(output).unwrap();
        assert!(markdown.contains("raw & wild"), "{markdown}");

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn keeps_merged_cells_aligned_in_slide_tables() {
        let directory =
            std::env::temp_dir().join(format!("shift-pptx-merge-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        // The second row spans two columns and is followed by a continuation
        // cell; the third row is a vertical merge continuation.
        let input = pptx_with_slide(
            &directory,
            "merged.pptx",
            r#"<p:sld xmlns:p="p" xmlns:a="a"><a:tbl>
              <a:tr>
                <a:tc><a:p><a:r><a:t>A</a:t></a:r></a:p></a:tc>
                <a:tc><a:p><a:r><a:t>B</a:t></a:r></a:p></a:tc>
                <a:tc><a:p><a:r><a:t>C</a:t></a:r></a:p></a:tc>
              </a:tr><a:tr>
                <a:tc gridSpan="2"><a:p><a:r><a:t>Wide</a:t></a:r></a:p></a:tc>
                <a:tc hMerge="1"><a:p/></a:tc>
                <a:tc><a:p><a:r><a:t>Y</a:t></a:r></a:p></a:tc>
              </a:tr><a:tr>
                <a:tc vMerge="1"><a:p/></a:tc>
                <a:tc><a:p><a:r><a:t>E</a:t></a:r></a:p></a:tc>
                <a:tc><a:p><a:r><a:t>F</a:t></a:r></a:p></a:tc>
              </a:tr>
            </a:tbl></p:sld>"#,
        );

        let output = presentation_to_markdown(&input, None).unwrap();
        let markdown = fs::read_to_string(output).unwrap();
        // Three grid columns, so every row keeps three cells.
        assert!(markdown.contains("| A | B | C |"), "{markdown}");
        assert!(markdown.contains("| Wide |  | Y |"), "{markdown}");
        assert!(markdown.contains("|  | E | F |"), "{markdown}");
        assert!(markdown.contains("| --- | --- | --- |"), "{markdown}");

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn extracts_image_only_docx_pages_in_document_order() {
        let directory =
            std::env::temp_dir().join(format!("shift-docx-pages-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("pages.docx");
        let media = directory.join("media");
        fs::create_dir_all(&media).unwrap();
        let first = media.join("first.png");
        let second = media.join("second.png");
        image::RgbImage::from_pixel(4, 3, image::Rgb([255, 0, 0]))
            .save_with_format(&first, image::ImageFormat::Png)
            .unwrap();
        image::RgbImage::from_pixel(4, 3, image::Rgb([0, 0, 255]))
            .save_with_format(&second, image::ImageFormat::Png)
            .unwrap();

        let file = fs::File::create(&input).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("word/_rels/document.xml.rels", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/second.png"/>
              <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/first.png"/>
            </Relationships>"#,
        )
        .unwrap();
        zip.start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<w:document xmlns:w="w" xmlns:a="a" xmlns:r="r"><w:body>
              <w:p><w:r><w:drawing><a:blip r:embed="rId9"/></w:drawing></w:r></w:p>
              <w:p><w:r><w:drawing><a:blip r:embed="rId2"/></w:drawing></w:r></w:p>
            </w:body></w:document>"#,
        )
        .unwrap();
        zip.start_file("word/media/first.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&fs::read(&first).unwrap()).unwrap();
        zip.start_file("word/media/second.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&fs::read(&second).unwrap()).unwrap();
        zip.finish().unwrap();

        let output = directory.join("pages");
        fs::create_dir_all(&output).unwrap();
        let pages = document_page_images(&input, &output).unwrap().unwrap();
        assert_eq!(pages.len(), 2);
        let first_pixel = image::load_from_memory(&fs::read(&pages[0]).unwrap())
            .unwrap()
            .to_rgb8()
            .get_pixel(0, 0)
            .0;
        let second_pixel = image::load_from_memory(&fs::read(&pages[1]).unwrap())
            .unwrap()
            .to_rgb8()
            .get_pixel(0, 0)
            .0;
        assert_eq!(first_pixel, [0, 0, 255]);
        assert_eq!(second_pixel, [255, 0, 0]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn leaves_text_documents_on_the_native_renderer() {
        let directory =
            std::env::temp_dir().join(format!("shift-docx-text-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("text.docx");
        let file = fs::File::create(&input).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("word/_rels/document.xml.rels", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#,
        )
        .unwrap();
        zip.start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Readable text</w:t></w:r></w:p></w:body></w:document>"#,
        )
        .unwrap();
        zip.finish().unwrap();

        let output = directory.join("pages");
        fs::create_dir_all(&output).unwrap();
        assert!(document_page_images(&input, &output).unwrap().is_none());
        fs::remove_dir_all(directory).unwrap();
    }
}
