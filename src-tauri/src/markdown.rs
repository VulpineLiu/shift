//! A small Markdown to HTML writer.
//!
//! Pandoc renders documents beautifully but is an optional external tool, and
//! without it Markdown used to fall through to the plain-text path, which
//! printed the markup verbatim. This covers the common syntax so headings,
//! lists and code are rendered wherever Pandoc is missing.

fn escape_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            other => output.push(other),
        }
    }
    output
}

/// Rewrites inline spans: code, emphasis, strikethrough and links.
fn inline(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut output = String::new();
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        match character {
            '`' => {
                if let Some(end) = find(&characters, index + 1, '`') {
                    output.push_str("<code>");
                    output.push_str(&escape_html(&String::from_iter(
                        &characters[index + 1..end],
                    )));
                    output.push_str("</code>");
                    index = end + 1;
                } else {
                    output.push_str(&escape_html(&character.to_string()));
                    index += 1;
                }
            }
            '!' if index + 1 < characters.len() && characters[index + 1] == '[' => {
                if let Some(closing) = find(&characters, index + 2, ']') {
                    if closing + 1 < characters.len() && characters[closing + 1] == '(' {
                        if let Some(end) = find(&characters, closing + 2, ')') {
                            let alt = String::from_iter(&characters[index + 2..closing]);
                            let url = String::from_iter(&characters[closing + 2..end]);
                            output.push_str("<img src=\"");
                            output.push_str(&escape_html(&url));
                            output.push_str("\" alt=\"");
                            output.push_str(&escape_html(&alt));
                            output.push_str("\" />");
                            index = end + 1;
                            continue;
                        }
                    }
                }
                output.push('!');
                index += 1;
            }
            '[' => {
                // A link needs `[text](` before the closing bracket is trusted.
                if let Some(closing) = find(&characters, index + 1, ']') {
                    let label = &characters[index + 1..closing];
                    if closing + 1 < characters.len() && characters[closing + 1] == '(' {
                        if let Some(end) = find(&characters, closing + 2, ')') {
                            let url = String::from_iter(&characters[closing + 2..end]);
                            output.push_str("<a href=\"");
                            output.push_str(&escape_html(&url));
                            output.push_str("\">");
                            output.push_str(&inline(&String::from_iter(label)));
                            output.push_str("</a>");
                            index = end + 1;
                            continue;
                        }
                    }
                }
                output.push_str(&escape_html(&character.to_string()));
                index += 1;
            }
            '~' if starts_at(&characters, index, "~~") => {
                if let Some(end) = find_pair(&characters, index + 2, "~~") {
                    output.push_str("<del>");
                    output.push_str(&inline(&String::from_iter(&characters[index + 2..end])));
                    output.push_str("</del>");
                    index = end + 2;
                } else {
                    output.push_str(&escape_html(&character.to_string()));
                    index += 1;
                }
            }
            '*' | '_' => {
                let double =
                    starts_at(&characters, index, "**") || starts_at(&characters, index, "__");
                let width = if double { 2 } else { 1 };
                let marker = String::from_iter(characters[index..index + width].iter());
                if let Some(end) = find_pair(&characters, index + width, &marker) {
                    let tag = if double { "strong" } else { "em" };
                    let inner = String::from_iter(&characters[index + width..end]);
                    if !inner.trim().is_empty() {
                        output.push_str(&format!("<{tag}>"));
                        output.push_str(&inline(&inner));
                        output.push_str(&format!("</{tag}>"));
                        index = end + width;
                        continue;
                    }
                }
                output.push_str(&escape_html(&character.to_string()));
                index += 1;
            }
            other => {
                output.push_str(&escape_html(&other.to_string()));
                index += 1;
            }
        }
    }
    output
}

fn find(haystack: &[char], from: usize, needle: char) -> Option<usize> {
    (from..haystack.len()).find(|index| haystack[*index] == needle)
}

fn find_pair(haystack: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() {
        return None;
    }
    (from..haystack.len().saturating_sub(needle.len() - 1))
        .find(|index| starts_at(haystack, *index, &String::from_iter(&needle)))
}

fn starts_at(haystack: &[char], index: usize, needle: &str) -> bool {
    let needle: Vec<char> = needle.chars().collect();
    !needle.is_empty()
        && index + needle.len() <= haystack.len()
        && haystack[index..index + needle.len()] == needle[..]
}

fn split_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| inline(cell.trim()))
        .collect()
}

fn is_rule(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 3 && ["---", "***", "___"].contains(&trimmed)
}

fn is_separator_row(line: &str) -> bool {
    let trimmed = line.trim().trim_matches('|');
    trimmed.contains('-')
        && trimmed.split('|').all(|cell| {
            !cell.trim().is_empty() && cell.trim().chars().all(|c| matches!(c, '-' | ':' | ' '))
        })
}

/// Renders Markdown into a standalone HTML document.
pub fn to_html(markdown: &str) -> String {
    let mut body = String::new();
    let lines: Vec<&str> = markdown.lines().collect();
    let mut index = 0;
    let mut paragraph: Vec<String> = Vec::new();

    let flush_paragraph = |paragraph: &mut Vec<String>, body: &mut String| {
        if paragraph.is_empty() {
            return;
        }
        body.push_str("<p>");
        body.push_str(&paragraph.join("<br />"));
        body.push_str("</p>\n");
        paragraph.clear();
    };

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush_paragraph(&mut paragraph, &mut body);
            let fence = &trimmed[..3];
            let mut code: Vec<String> = Vec::new();
            index += 1;
            while index < lines.len() && !lines[index].trim().starts_with(fence) {
                code.push(escape_html(lines[index]));
                index += 1;
            }
            index += 1;
            body.push_str("<pre><code>");
            body.push_str(&code.join("\n"));
            body.push_str("</code></pre>\n");
            continue;
        }

        if trimmed.is_empty() {
            flush_paragraph(&mut paragraph, &mut body);
            index += 1;
            continue;
        }

        if is_rule(trimmed) {
            flush_paragraph(&mut paragraph, &mut body);
            body.push_str("<hr />\n");
            index += 1;
            continue;
        }

        if let Some(level) = trimmed
            .find(' ')
            .filter(|_| trimmed.starts_with('#'))
            .map(|space| trimmed[..space].chars().filter(|c| *c == '#').count())
            .filter(|count| (1..=6).contains(count))
        {
            flush_paragraph(&mut paragraph, &mut body);
            let text = inline(trimmed[level..].trim());
            body.push_str(&format!("<h{level}>{text}</h{level}>\n"));
            index += 1;
            continue;
        }

        if trimmed.contains('|') && index + 1 < lines.len() && is_separator_row(lines[index + 1]) {
            flush_paragraph(&mut paragraph, &mut body);
            let header = split_row(trimmed);
            body.push_str("<table>\n<thead>\n<tr>");
            for cell in &header {
                body.push_str(&format!("<th>{cell}</th>"));
            }
            body.push_str("</tr>\n</thead>\n<tbody>\n");
            index += 2;
            while index < lines.len()
                && lines[index].contains('|')
                && !lines[index].trim().is_empty()
            {
                body.push_str("<tr>");
                for cell in split_row(lines[index]) {
                    body.push_str(&format!("<td>{cell}</td>"));
                }
                body.push_str("</tr>\n");
                index += 1;
            }
            body.push_str("</tbody>\n</table>\n");
            continue;
        }

        if trimmed.starts_with("> ") || trimmed == ">" {
            flush_paragraph(&mut paragraph, &mut body);
            let mut quote: Vec<String> = Vec::new();
            while index < lines.len()
                && (lines[index].trim().starts_with("> ") || lines[index].trim() == ">")
            {
                quote.push(inline(lines[index].trim().trim_start_matches('>').trim()));
                index += 1;
            }
            body.push_str("<blockquote>");
            body.push_str(&quote.join("<br />"));
            body.push_str("</blockquote>\n");
            continue;
        }

        let bullet =
            trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ");
        let ordered = trimmed
            .find('.')
            .map(|dot| dot > 0 && trimmed[..dot].chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(false)
            && trimmed[trimmed.find('.').unwrap() + 1..].starts_with(' ');
        if bullet || ordered {
            flush_paragraph(&mut paragraph, &mut body);
            let tag = if ordered { "ol" } else { "ul" };
            body.push_str(&format!("<{tag}>\n"));
            while index < lines.len() {
                let item = lines[index].trim();
                let is_bullet =
                    item.starts_with("- ") || item.starts_with("* ") || item.starts_with("+ ");
                let is_ordered = item
                    .find('.')
                    .map(|dot| dot > 0 && item[..dot].chars().all(|c| c.is_ascii_digit()))
                    .unwrap_or(false)
                    && item.len() > item.find('.').unwrap() + 1
                    && item.as_bytes()[item.find('.').unwrap() + 1] == b' ';
                if (ordered && !is_ordered) || (bullet && !is_bullet && !is_ordered) {
                    break;
                }
                let content = match item.find(' ') {
                    Some(space) => inline(&item[space + 1..]),
                    None => String::new(),
                };
                body.push_str(&format!("<li>{content}</li>\n"));
                index += 1;
            }
            body.push_str(&format!("</{tag}>\n"));
            continue;
        }

        paragraph.push(inline(trimmed));
        index += 1;
    }
    flush_paragraph(&mut paragraph, &mut body);

    page(&body)
}

/// Wraps an HTML body in the document every renderer path prints from, so a
/// Pandoc-produced body and a built-in Markdown body reach the page the same way.
pub fn page(body: &str) -> String {
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\" />\n<style>\n{}\n</style>\n</head>\n<body>\n{}\n</body>\n</html>\n",
        STYLE, body
    )
}

/// Styles the rendered document so the PDF path produces a readable page.
const STYLE: &str = r#"
body { font-family: -apple-system, "Helvetica Neue", Arial, sans-serif; font-size: 13px; line-height: 1.6; color: #241a15; margin: 40px; }
h1, h2, h3, h4, h5, h6 { margin: 18px 0 8px; line-height: 1.25; }
h1 { font-size: 26px; } h2 { font-size: 21px; } h3 { font-size: 17px; }
p { margin: 0 0 12px; }
ul, ol { margin: 0 0 12px; padding-left: 22px; }
code { font-family: "SF Mono", Menlo, monospace; font-size: 11.5px; background: rgba(120, 80, 55, 0.09); padding: 1px 4px; border-radius: 4px; }
pre { background: rgba(120, 80, 55, 0.07); padding: 12px; border-radius: 8px; overflow-x: auto; }
pre code { background: transparent; padding: 0; }
blockquote { margin: 0 0 12px; padding-left: 12px; border-left: 3px solid rgba(200, 110, 60, 0.5); color: #5d463a; }
table { border-collapse: collapse; margin: 0 0 12px; }
th, td { border: 1px solid rgba(90, 60, 40, 0.25); padding: 6px 10px; text-align: left; }
th { background: rgba(120, 80, 55, 0.08); }
img { max-width: 100%; height: auto; }
hr { border: 0; border-top: 1px solid rgba(90, 60, 40, 0.2); margin: 18px 0; }
a { color: #c8541f; }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_and_emphasis() {
        let html =
            to_html("# Title\n\nSome **bold**, *some* `code` and [a link](https://example.com).\n");
        assert!(html.contains("<h1>Title</h1>"), "{html}");
        assert!(html.contains("<strong>bold</strong>"), "{html}");
        assert!(html.contains("<em>some</em>"), "{html}");
        assert!(html.contains("<code>code</code>"), "{html}");
        assert!(
            html.contains("<a href=\"https://example.com\">a link</a>"),
            "{html}"
        );
    }

    #[test]
    fn renders_lists_code_and_tables() {
        let html = to_html(
            "- one\n- two\n\n1. first\n2. second\n\n```rust\nlet x = 1;\n```\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n",
        );
        assert!(html.contains("<ul>\n<li>one</li>\n<li>two</li>"), "{html}");
        assert!(
            html.contains("<ol>\n<li>first</li>\n<li>second</li>"),
            "{html}"
        );
        assert!(
            html.contains("<pre><code>let x = 1;</code></pre>"),
            "{html}"
        );
        assert!(html.contains("<th>A</th>"), "{html}");
        assert!(html.contains("<td>2</td>"), "{html}");
    }

    #[test]
    fn escapes_raw_html_and_keeps_paragraphs() {
        let html = to_html("line one\nline two\n\n<script>alert(1)</script>\n");
        assert!(html.contains("line one<br />line two"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(!html.contains("<script>"), "{html}");
    }

    #[test]
    fn renders_relative_images() {
        let html = to_html("![Architecture](images/system.png)\n");
        assert!(
            html.contains(r#"<img src="images/system.png" alt="Architecture" />"#),
            "{html}"
        );
    }
}
