//! Bidirectional HTML <-> Markdown conversion.
//!
//! Markdown -> HTML reuses `pulldown-cmark`'s CommonMark renderer. HTML ->
//! Markdown uses a small tolerant hand-written tokenizer (real-world HTML is
//! often not well-formed XML, so `quick-xml`'s strict parser is not a good
//! fit here) supporting headings, paragraphs, emphasis, links, images, lists,
//! blockquotes, code blocks, and tables. Layout/CSS, forms, and scripts are
//! not preserved.

use crate::MarkoffError;
use crate::html_tokenizer::{Token, collapse_whitespace, tokenize};
use crate::tables::markdown_table_from_rows;
use crate::xml_utils::{MarkdownEscapeContext, markdown_escape, xml_escape};
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
                            markdown_escape(&collapsed, MarkdownEscapeContext::Html)
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
    use crate::test_support::unique_temp_path;
    use std::fs;

    #[test]
    fn converts_markdown_to_html_and_back() {
        let markdown_in = unique_temp_path("html_roundtrip_input", "md");
        let html = unique_temp_path("html_roundtrip_page", "html");
        let markdown_out = unique_temp_path("html_roundtrip_output", "md");
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
        let html = unique_temp_path("html_handwritten", "html");
        let markdown_out = unique_temp_path("html_handwritten_output", "md");
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
