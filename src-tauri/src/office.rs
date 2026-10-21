use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use calamine::{open_workbook_auto, Reader};
use quick_xml::{events::Event, Reader as XmlReader};
use zip::ZipArchive;

use crate::files::unique_output_in;

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

/// Extracts slide content in document order. Text is grouped per `<a:p>` as
/// before; tables are additionally walked row by row so cells do not collapse
/// into a single paragraph of prose.
fn slide_blocks(xml: &str) -> Result<Vec<SlideBlock>, String> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut blocks: Vec<SlideBlock> = Vec::new();
    let mut paragraph = String::new();
    // Table state. Nested tables are not a real PowerPoint layout, so a single
    // level of row/cell buffering is enough.
    let mut in_table = false;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) => match local_name(element.name().as_ref()) {
                "tbl" => {
                    push_paragraph(&mut paragraph, &mut blocks);
                    in_table = true;
                    rows.clear();
                    row.clear();
                }
                "tr" if in_table => row.clear(),
                "tc" if in_table => cell.clear(),
                "p" if !in_table => push_paragraph(&mut paragraph, &mut blocks),
                "p" if in_table && !cell.trim().is_empty() => cell.push(' '),
                _ => {}
            },
            Ok(Event::End(element)) => {
                if !in_table {
                    continue;
                }
                match local_name(element.name().as_ref()) {
                    "tc" => row.push(std::mem::take(&mut cell)),
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
                let text = unescaped.trim();
                if text.is_empty() {
                    continue;
                }
                if in_table {
                    if !cell.is_empty() {
                        cell.push(' ');
                    }
                    cell.push_str(text);
                } else {
                    if !paragraph.ends_with('\n') && !paragraph.is_empty() {
                        paragraph.push(' ');
                    }
                    paragraph.push_str(text);
                }
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
    use std::io::Write as _;
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
        let directory =
            std::env::temp_dir().join(format!("shift-pptx-{}", uuid::Uuid::new_v4()));
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
        let directory = std::env::temp_dir().join(format!("shift-pptx-tbl-{}", uuid::Uuid::new_v4()));
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
}
