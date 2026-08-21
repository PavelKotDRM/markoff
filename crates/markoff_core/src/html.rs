//! Bidirectional HTML <-> Markdown conversion.
//!
//! Markdown -> HTML reuses `pulldown-cmark`'s CommonMark renderer. HTML ->
//! Markdown uses a small tolerant hand-written tokenizer (real-world HTML is
//! often not well-formed XML, so `quick-xml`'s strict parser is not a good
//! fit here) supporting headings, paragraphs, emphasis, links, images, lists,
//! blockquotes, code blocks, and tables. Layout/CSS, forms, and scripts are
//! not preserved.

use crate::MarkoffError;
use crate::tables::markdown_table_from_rows;
use std::path::Path;

pub(crate) fn convert_markdown_to_html(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    use pulldown_cmark::{Options, Parser, html};

    let source = std::fs::read_to_string(input)?;
    let title = source
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .unwrap_or("Document");

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(&source, options);
    let mut body = String::new();
    html::push_html(&mut body, parser);

    let document = format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n</head>\n<body>\n{body}</body>\n</html>\n",
        xml_escape(title)
    );
    std::fs::write(output, document)?;
    Ok(())
}

pub(crate) fn convert_html_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let markdown = html_to_markdown(&source);
    std::fs::write(output, markdown)?;
    Ok(())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn markdown_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('`', "\\`")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
    "source", "track", "wbr",
];

enum Token {
    Start(String, Vec<(String, String)>, bool),
    End(String),
    Text(String),
}

fn tokenize(html: &str) -> Vec<Token> {
    let len = html.len();
    let mut index = 0;
    let mut tokens = Vec::new();
    let mut text_buffer = String::new();

    while index < len {
        if html.as_bytes()[index] == b'<' {
            if !text_buffer.is_empty() {
                tokens.push(Token::Text(std::mem::take(&mut text_buffer)));
            }
            let rest = &html[index..];
            if rest.starts_with("<!--") {
                index += rest.find("-->").map_or(rest.len(), |end| end + 3);
                continue;
            }
            if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case("<!doctype") {
                index += rest.find('>').map_or(rest.len(), |end| end + 1);
                continue;
            }
            let (tag_content, consumed) = scan_tag(rest);
            index += consumed;

            if let Some(name) = tag_content.strip_prefix('/') {
                tokens.push(Token::End(name.trim().to_ascii_lowercase()));
                continue;
            }

            let trimmed_end = tag_content.trim_end();
            let self_closing_slash = trimmed_end.ends_with('/');
            let body = trimmed_end.trim_end_matches('/');
            let (name, attributes) = parse_tag(body);
            let lower_name = name.to_ascii_lowercase();

            if lower_name == "script" || lower_name == "style" {
                let close_pattern = format!("</{lower_name}");
                let remaining = &html[index..];
                if let Some(position) = remaining.to_ascii_lowercase().find(&close_pattern) {
                    index += position;
                    let after = &html[index..];
                    index += after.find('>').map_or(after.len(), |end| end + 1);
                } else {
                    index = len;
                }
                continue;
            }

            let is_void = self_closing_slash || VOID_ELEMENTS.contains(&lower_name.as_str());
            tokens.push(Token::Start(lower_name, attributes, is_void));
        } else {
            let next = html[index..]
                .find('<')
                .map_or(len, |position| index + position);
            text_buffer.push_str(&decode_entities(&html[index..next]));
            index = next;
        }
    }
    if !text_buffer.is_empty() {
        tokens.push(Token::Text(text_buffer));
    }
    tokens
}

/// Scans a `<...>` tag body, respecting quoted attribute values so a literal
/// `>` inside `href="a>b"` does not end the tag early.
fn scan_tag(rest: &str) -> (String, usize) {
    let bytes = rest.as_bytes();
    let mut index = 1;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' if !in_double_quote => in_single_quote = !in_single_quote,
            b'"' if !in_single_quote => in_double_quote = !in_double_quote,
            b'>' if !in_single_quote && !in_double_quote => break,
            _ => {}
        }
        index += 1;
    }
    (rest[1..index].to_string(), index + 1)
}

fn parse_tag(content: &str) -> (String, Vec<(String, String)>) {
    let content = content.trim();
    let name_end = content
        .find(|character: char| character.is_whitespace())
        .unwrap_or(content.len());
    let name = content[..name_end].to_string();
    let rest = content[name_end..].trim();

    let bytes = rest.as_bytes();
    let mut index = 0;
    let mut attributes = Vec::new();
    while index < bytes.len() {
        while index < bytes.len() && (bytes[index] as char).is_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let key_start = index;
        while index < bytes.len() && bytes[index] != b'=' && !(bytes[index] as char).is_whitespace()
        {
            index += 1;
        }
        let key = rest[key_start..index].to_ascii_lowercase();
        while index < bytes.len() && (bytes[index] as char).is_whitespace() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'=' {
            index += 1;
            while index < bytes.len() && (bytes[index] as char).is_whitespace() {
                index += 1;
            }
            if index < bytes.len() && (bytes[index] == b'"' || bytes[index] == b'\'') {
                let quote = bytes[index];
                index += 1;
                let value_start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                let value = decode_entities(&rest[value_start..index]);
                if index < bytes.len() {
                    index += 1;
                }
                if !key.is_empty() {
                    attributes.push((key, value));
                }
            } else {
                let value_start = index;
                while index < bytes.len() && !(bytes[index] as char).is_whitespace() {
                    index += 1;
                }
                let value = decode_entities(&rest[value_start..index]);
                if !key.is_empty() {
                    attributes.push((key, value));
                }
            }
        } else if !key.is_empty() {
            attributes.push((key, String::new()));
        }
    }
    (name, attributes)
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        result.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(end) = tail[..tail.len().min(12)].find(';') {
            let entity = &tail[1..end];
            if let Some(replacement) = decode_one_entity(entity) {
                result.push_str(&replacement);
                rest = &tail[end + 1..];
                continue;
            }
        }
        result.push('&');
        rest = &tail[1..];
    }
    result.push_str(rest);
    result
}

fn decode_one_entity(entity: &str) -> Option<String> {
    if let Some(hex) = entity.strip_prefix('x').or_else(|| entity.strip_prefix('X')) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).map(String::from);
    }
    if let Some(decimal) = entity.strip_prefix('#') {
        if let Some(hex) = decimal.strip_prefix('x').or_else(|| decimal.strip_prefix('X')) {
            return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).map(String::from);
        }
        return decimal.parse::<u32>().ok().and_then(char::from_u32).map(String::from);
    }
    Some(
        match entity {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "nbsp" => "\u{00A0}",
            "mdash" => "\u{2014}",
            "ndash" => "\u{2013}",
            "hellip" => "\u{2026}",
            "lsquo" => "\u{2018}",
            "rsquo" => "\u{2019}",
            "ldquo" => "\u{201C}",
            "rdquo" => "\u{201D}",
            "copy" => "\u{00A9}",
            "reg" => "\u{00AE}",
            "trade" => "\u{2122}",
            "laquo" => "\u{00AB}",
            "raquo" => "\u{00BB}",
            _ => return None,
        }
        .to_string(),
    )
}

fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_was_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            if !last_was_space {
                result.push(' ');
            }
            last_was_space = true;
        } else {
            result.push(character);
            last_was_space = false;
        }
    }
    result
}

enum ListKind {
    Bullet,
    Ordered(usize),
}

fn open_block(block_stack: &mut Vec<String>, is_cell_stack: &mut Vec<bool>, is_cell: bool) {
    block_stack.push(String::new());
    is_cell_stack.push(is_cell);
}

fn close_block(block_stack: &mut Vec<String>, is_cell_stack: &mut Vec<bool>) -> Option<String> {
    is_cell_stack.pop();
    block_stack.pop()
}

fn push_marker(block_stack: &mut [String], marker: &str) {
    if let Some(buffer) = block_stack.last_mut() {
        buffer.push_str(marker);
    }
}

fn flush_block(output: &mut String, text: &str, quote_depth: usize) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if quote_depth == 0 {
        output.push_str(text);
    } else {
        let prefix = "> ".repeat(quote_depth);
        let quoted = text
            .lines()
            .map(|line| format!("{prefix}{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        output.push_str(&quoted);
    }
    output.push_str("\n\n");
}

fn html_to_markdown(html: &str) -> String {
    let tokens = tokenize(html);
    let mut output = String::new();
    let mut block_stack: Vec<String> = Vec::new();
    let mut is_cell_stack: Vec<bool> = Vec::new();
    let mut quote_depth = 0usize;
    let mut list_stack: Vec<ListKind> = Vec::new();
    let mut link_starts: Vec<(usize, String)> = Vec::new();
    let mut in_pre = false;
    let mut skip_tag: Option<String> = None;
    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut heading_level: Option<usize> = None;
    let mut active_li = 0usize;

    for token in tokens {
        if let Some(tag) = &skip_tag {
            if let Token::End(name) = &token
                && name == tag
            {
                skip_tag = None;
            }
            continue;
        }
        match token {
            Token::Start(name, attributes, _self_closing) => match name.as_str() {
                "head" | "script" | "style" | "title" => skip_tag = Some(name),
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    heading_level = name[1..].parse().ok();
                    open_block(&mut block_stack, &mut is_cell_stack, false);
                }
                "p" => {
                    if active_li == 0 {
                        open_block(&mut block_stack, &mut is_cell_stack, false);
                    }
                }
                "blockquote" => {
                    quote_depth += 1;
                    open_block(&mut block_stack, &mut is_cell_stack, false);
                }
                "ul" => list_stack.push(ListKind::Bullet),
                "ol" => list_stack.push(ListKind::Ordered(1)),
                "li" => {
                    active_li += 1;
                    open_block(&mut block_stack, &mut is_cell_stack, false);
                }
                "table" => {
                    in_table = true;
                    table_rows.clear();
                    current_row.clear();
                }
                "tr" => current_row.clear(),
                "td" | "th" => open_block(&mut block_stack, &mut is_cell_stack, true),
                "pre" => {
                    in_pre = true;
                    open_block(&mut block_stack, &mut is_cell_stack, false);
                }
                "code" if !in_pre => push_marker(&mut block_stack, "`"),
                "strong" | "b" => push_marker(&mut block_stack, "**"),
                "em" | "i" => push_marker(&mut block_stack, "*"),
                "a" => {
                    let href = attributes
                        .iter()
                        .find(|(key, _)| key == "href")
                        .map(|(_, value)| value.clone())
                        .unwrap_or_default();
                    let start = block_stack.last().map_or(0, String::len);
                    link_starts.push((start, href));
                }
                "img" => {
                    let src = attributes
                        .iter()
                        .find(|(key, _)| key == "src")
                        .map(|(_, value)| value.clone())
                        .unwrap_or_default();
                    let alt = attributes
                        .iter()
                        .find(|(key, _)| key == "alt")
                        .map(|(_, value)| value.clone())
                        .unwrap_or_default();
                    let image = format!("![{alt}]({src})");
                    if let Some(buffer) = block_stack.last_mut() {
                        buffer.push_str(&image);
                    } else {
                        output.push_str(&image);
                        output.push_str("\n\n");
                    }
                }
                "br" => push_marker(&mut block_stack, "  \n"),
                "hr" => output.push_str("---\n\n"),
                _ => {}
            },
            Token::End(name) => match name.as_str() {
                "head" | "script" | "style" | "title" => {}
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    if let (Some(text), Some(level)) = (
                        close_block(&mut block_stack, &mut is_cell_stack),
                        heading_level.take(),
                    ) {
                        let text = text.trim();
                        if !text.is_empty() {
                            output.push_str(&"#".repeat(level));
                            output.push(' ');
                            output.push_str(text);
                            output.push_str("\n\n");
                        }
                    }
                }
                "p" => {
                    if active_li == 0
                        && let Some(text) = close_block(&mut block_stack, &mut is_cell_stack)
                    {
                        flush_block(&mut output, &text, quote_depth);
                    }
                }
                "blockquote" => {
                    if let Some(text) = close_block(&mut block_stack, &mut is_cell_stack) {
                        flush_block(&mut output, &text, quote_depth);
                    }
                    quote_depth = quote_depth.saturating_sub(1);
                }
                "ul" | "ol" => {
                    list_stack.pop();
                    if list_stack.is_empty() {
                        output.push('\n');
                    }
                }
                "li" => {
                    active_li = active_li.saturating_sub(1);
                    if let Some(text) = close_block(&mut block_stack, &mut is_cell_stack) {
                        let text = text.trim();
                        if !text.is_empty() {
                            let indent = list_stack.len().saturating_sub(1);
                            let marker = match list_stack.last_mut() {
                                Some(ListKind::Ordered(counter)) => {
                                    let value = *counter;
                                    *counter += 1;
                                    format!("{value}. ")
                                }
                                _ => "- ".to_string(),
                            };
                            output.push_str(&"  ".repeat(indent));
                            output.push_str(&marker);
                            output.push_str(text);
                            output.push('\n');
                        }
                    }
                }
                "table" => {
                    in_table = false;
                    if !table_rows.is_empty() {
                        output.push_str(&markdown_table_from_rows(&table_rows));
                        output.push_str("\n\n");
                    }
                }
                "tr" => {
                    if in_table {
                        table_rows.push(std::mem::take(&mut current_row));
                    }
                }
                "td" | "th" => {
                    if let Some(text) = close_block(&mut block_stack, &mut is_cell_stack) {
                        current_row.push(text.trim().to_string());
                    }
                }
                "pre" => {
                    if let Some(text) = close_block(&mut block_stack, &mut is_cell_stack) {
                        output.push_str("```\n");
                        output.push_str(text.trim_matches('\n'));
                        output.push_str("\n```\n\n");
                    }
                    in_pre = false;
                }
                "code" if !in_pre => push_marker(&mut block_stack, "`"),
                "strong" | "b" => push_marker(&mut block_stack, "**"),
                "em" | "i" => push_marker(&mut block_stack, "*"),
                "a" => {
                    if let Some((start, href)) = link_starts.pop()
                        && let Some(buffer) = block_stack.last_mut()
                        && start <= buffer.len()
                    {
                        let text = buffer[start..].to_string();
                        buffer.truncate(start);
                        buffer.push_str(&format!("[{text}]({href})"));
                    }
                }
                _ => {}
            },
            Token::Text(text) => {
                let is_cell = *is_cell_stack.last().unwrap_or(&false);
                if let Some(buffer) = block_stack.last_mut() {
                    if in_pre {
                        buffer.push_str(&text);
                    } else {
                        let collapsed = collapse_whitespace(&text);
                        buffer.push_str(&if is_cell {
                            collapsed
                        } else {
                            markdown_escape(&collapsed)
                        });
                    }
                }
            }
        }
    }

    let trimmed = output.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::{convert_html_to_markdown, convert_markdown_to_html};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_path(name: &str, extension: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("markoff_html_{name}_{nanos}.{extension}"))
    }

    #[test]
    fn converts_markdown_to_html_and_back() {
        let markdown_in = unique_temp_path("input", "md");
        let html = unique_temp_path("page", "html");
        let markdown_out = unique_temp_path("output", "md");
        fs::write(
            &markdown_in,
            "# Title\n\nA **bold** and *italic* [link](https://example.com) paragraph.\n\n- First\n- Second\n",
        )
        .unwrap();

        convert_markdown_to_html(&markdown_in, &html).unwrap();
        let rendered_html = fs::read_to_string(&html).unwrap();
        assert!(rendered_html.contains("<h1>Title</h1>"));
        assert!(rendered_html.contains("<strong>bold</strong>"));

        convert_html_to_markdown(&html, &markdown_out).unwrap();
        let rendered_markdown = fs::read_to_string(&markdown_out).unwrap();
        assert!(rendered_markdown.contains("# Title"));
        assert!(rendered_markdown.contains("**bold**"));
        assert!(rendered_markdown.contains("*italic*"));
        assert!(rendered_markdown.contains("[link](https://example.com)"));
        assert!(rendered_markdown.contains("- First"));
        assert!(rendered_markdown.contains("- Second"));

        fs::remove_file(markdown_in).ok();
        fs::remove_file(html).ok();
        fs::remove_file(markdown_out).ok();
    }

    #[test]
    fn parses_hand_written_html_with_void_elements_and_table() {
        let html = unique_temp_path("handwritten", "html");
        let markdown_out = unique_temp_path("output", "md");
        fs::write(
            &html,
            "<html><head><title>Ignore</title></head><body>\n<h2>Report</h2>\n<p>Line one<br>Line two</p>\n<table><tr><th>Name</th><th>Score</th></tr><tr><td>Ada</td><td>42</td></tr></table>\n<hr>\n</body></html>",
        )
        .unwrap();

        convert_html_to_markdown(&html, &markdown_out).unwrap();
        let rendered = fs::read_to_string(&markdown_out).unwrap();
        assert!(rendered.contains("## Report"));
        assert!(rendered.contains("Line one"));
        assert!(rendered.contains("Line two"));
        assert!(rendered.contains("| Name | Score |"));
        assert!(rendered.contains("| Ada | 42 |"));
        assert!(rendered.contains("---"));
        assert!(!rendered.contains("Ignore"));

        fs::remove_file(html).ok();
        fs::remove_file(markdown_out).ok();
    }
}
