use crate::MarkoffError;
use crate::html_tokenizer::{Token, collapse_whitespace, tokenize};
use crate::tables::markdown_table_from_rows;
use crate::xml_utils::{MarkdownEscapeContext, markdown_escape, xml_attribute_escape};
use std::path::Path;

pub(crate) fn convert_html_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let markdown = html_to_markdown(&source)?;
    std::fs::write(output, markdown)?;
    Ok(())
}

enum ListKind {
    Bullet,
    Ordered(usize),
}

struct BlockBuffer {
    text: String,
    is_cell: bool,
}

fn open_block(block_stack: &mut Vec<BlockBuffer>, is_cell: bool) {
    block_stack.push(BlockBuffer {
        text: String::new(),
        is_cell,
    });
}

fn close_block(block_stack: &mut Vec<BlockBuffer>) -> Option<String> {
    block_stack.pop().map(|buffer| buffer.text)
}

fn push_marker(block_stack: &mut [BlockBuffer], marker: &str) {
    if let Some(buffer) = block_stack.last_mut() {
        buffer.text.push_str(marker);
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

fn html_to_markdown(html: &str) -> Result<String, MarkoffError> {
    let tokens = tokenize(html)?;
    let mut output = String::new();
    let mut block_stack: Vec<BlockBuffer> = Vec::new();
    let mut quote_depth = 0usize;
    let mut list_stack: Vec<ListKind> = Vec::new();
    let mut link_starts: Vec<Option<(usize, String, Option<String>)>> = Vec::new();
    let mut in_pre = false;
    let mut skip_tag: Option<String> = None;
    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut table_alignments = Vec::new();
    let mut heading_level: Option<usize> = None;
    let mut active_li = 0usize;
    let mut footnote_reference_depth = 0usize;
    let mut footnote_definition: Option<(String, usize)> = None;
    let mut footnote_definitions = Vec::new();
    let mut math_spans: Vec<Option<&str>> = Vec::new();
    let mut code_info: Option<String> = None;

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
            Token::Start(name, attributes, _self_closing) => {
                if name == "div"
                    && has_html_class(&attributes, "footnote-definition")
                    && let Some(label) = html_attribute(&attributes, "id")
                {
                    footnote_definition = Some((label, output.len()));
                    continue;
                }
                match name.as_str() {
                    "head" | "script" | "style" | "title" => skip_tag = Some(name),
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        heading_level = name[1..].parse().ok();
                        open_block(&mut block_stack, false);
                    }
                    "p" => {
                        if active_li == 0 {
                            open_block(&mut block_stack, false);
                        }
                    }
                    "blockquote" => {
                        quote_depth += 1;
                        open_block(&mut block_stack, false);
                    }
                    "ul" => list_stack.push(ListKind::Bullet),
                    "ol" => {
                        let start = html_attribute(&attributes, "start")
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(1);
                        list_stack.push(ListKind::Ordered(start));
                    }
                    "li" => {
                        active_li += 1;
                        open_block(&mut block_stack, false);
                    }
                    "table" => {
                        in_table = true;
                        table_rows.clear();
                        current_row.clear();
                        table_alignments.clear();
                    }
                    "tr" => current_row.clear(),
                    "td" | "th" => {
                        reject_unsupported_table_span(&attributes)?;
                        if name == "th" {
                            table_alignments.push(
                                html_attribute(&attributes, "style")
                                    .and_then(|style| table_alignment_from_style(&style)),
                            );
                        }
                        open_block(&mut block_stack, true);
                    }
                    "pre" => {
                        in_pre = true;
                        code_info = None;
                        open_block(&mut block_stack, false);
                    }
                    "code" if !in_pre => push_marker(&mut block_stack, "`"),
                    "code" if in_pre => {
                        code_info = html_attribute(&attributes, "class").and_then(|classes| {
                            classes.split_ascii_whitespace().find_map(|class| {
                                class.strip_prefix("language-").map(str::to_string)
                            })
                        })
                    }
                    "strong" | "b" => push_marker(&mut block_stack, "**"),
                    "em" | "i" => push_marker(&mut block_stack, "*"),
                    "del" | "s" => push_marker(&mut block_stack, "~~"),
                    "u" => push_marker(&mut block_stack, "<u>"),
                    "sup" if has_html_class(&attributes, "footnote-reference") => {
                        footnote_reference_depth += 1;
                    }
                    "sup" if has_html_class(&attributes, "footnote-definition-label") => {
                        skip_tag = Some(name);
                    }
                    "sup" => push_marker(&mut block_stack, "$^{"),
                    "sub" => push_marker(&mut block_stack, "$_{"),
                    "span" => {
                        let class = html_attribute(&attributes, "class").unwrap_or_default();
                        let marker = if class
                            .split_ascii_whitespace()
                            .any(|class| class == "math-inline")
                        {
                            Some("$")
                        } else if class
                            .split_ascii_whitespace()
                            .any(|class| class == "math-display")
                        {
                            Some("$$")
                        } else {
                            None
                        };
                        if let Some(marker) = marker {
                            push_marker(&mut block_stack, marker);
                        }
                        math_spans.push(marker);
                    }
                    "input"
                        if html_attribute(&attributes, "type").as_deref() == Some("checkbox") =>
                    {
                        let marker = if attributes.iter().any(|(key, _)| key == "checked") {
                            "[x]"
                        } else {
                            "[ ]"
                        };
                        push_marker(&mut block_stack, marker);
                    }
                    "a" => {
                        if has_html_class(&attributes, "footnote-backref") {
                            skip_tag = Some(name);
                            continue;
                        }
                        let href = attributes
                            .iter()
                            .find(|(key, _)| key == "href")
                            .map(|(_, value)| value.clone())
                            .unwrap_or_default();
                        if footnote_reference_depth > 0
                            && let Some(label) = href.strip_prefix('#')
                        {
                            push_marker(&mut block_stack, &format!("[^{label}]"));
                            skip_tag = Some(name);
                            continue;
                        }
                        if let Some(bookmark) = html_attribute(&attributes, "id")
                            .or_else(|| html_attribute(&attributes, "name"))
                            && href.is_empty()
                        {
                            let marker =
                                format!("<a id=\"{}\"></a>", xml_attribute_escape(&bookmark));
                            push_marker(&mut block_stack, &marker);
                            link_starts.push(None);
                            continue;
                        }
                        let start = block_stack.last().map_or(0, |buffer| buffer.text.len());
                        link_starts.push((!href.is_empty()).then_some((
                            start,
                            href,
                            html_attribute(&attributes, "title"),
                        )));
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
                        let alt = markdown_escape(&alt, MarkdownEscapeContext::Html);
                        let title = html_attribute(&attributes, "title").map_or_else(
                            String::new,
                            |title| {
                                format!(" \"{}\"", title.replace('\\', "\\\\").replace('"', "\\\""))
                            },
                        );
                        let image = format!("![{alt}]({src}{title})");
                        if let Some(buffer) = block_stack.last_mut() {
                            buffer.text.push_str(&image);
                        } else {
                            output.push_str(&image);
                            output.push_str("\n\n");
                        }
                    }
                    "br" => push_marker(&mut block_stack, "  \n"),
                    "hr" => output.push_str("---\n\n"),
                    _ => {}
                }
            }
            Token::End(name) => {
                if name == "div"
                    && let Some((label, start)) = footnote_definition.take()
                {
                    if start <= output.len() {
                        let body = output.split_off(start);
                        let body = body.trim();
                        if !body.is_empty() {
                            footnote_definitions.push(format_footnote_definition(&label, body));
                        }
                    }
                    continue;
                }
                match name.as_str() {
                    "head" | "script" | "style" | "title" => {}
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        if let (Some(text), Some(level)) =
                            (close_block(&mut block_stack), heading_level.take())
                        {
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
                            && let Some(text) = close_block(&mut block_stack)
                        {
                            flush_block(&mut output, &text, quote_depth);
                        }
                    }
                    "blockquote" => {
                        if let Some(text) = close_block(&mut block_stack) {
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
                        if let Some(text) = close_block(&mut block_stack) {
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
                                output.push_str(&"    ".repeat(indent));
                                output.push_str(&marker);
                                output.push_str(text);
                                output.push('\n');
                            }
                        }
                    }
                    "table" => {
                        in_table = false;
                        if !table_rows.is_empty() {
                            let table = markdown_table_from_rows(&table_rows);
                            output.push_str(&format_table_alignment(&table, &table_alignments));
                            output.push_str("\n\n");
                        }
                    }
                    "tr" => {
                        if in_table {
                            table_rows.push(std::mem::take(&mut current_row));
                        }
                    }
                    "td" | "th" => {
                        if let Some(text) = close_block(&mut block_stack) {
                            current_row.push(text.trim().to_string());
                        }
                    }
                    "pre" => {
                        if let Some(text) = close_block(&mut block_stack) {
                            output.push_str("```");
                            if let Some(info) = code_info.take() {
                                output.push_str(&info);
                            }
                            output.push('\n');
                            output.push_str(text.trim_matches('\n'));
                            output.push_str("\n```\n\n");
                        }
                        in_pre = false;
                    }
                    "code" if !in_pre => push_marker(&mut block_stack, "`"),
                    "strong" | "b" => push_marker(&mut block_stack, "**"),
                    "em" | "i" => push_marker(&mut block_stack, "*"),
                    "del" | "s" => push_marker(&mut block_stack, "~~"),
                    "u" => push_marker(&mut block_stack, "</u>"),
                    "sup" if footnote_reference_depth > 0 => {
                        footnote_reference_depth -= 1;
                    }
                    "sup" => push_marker(&mut block_stack, "}$"),
                    "sub" => push_marker(&mut block_stack, "}$"),
                    "span" => {
                        if let Some(Some(marker)) = math_spans.pop() {
                            push_marker(&mut block_stack, marker);
                        }
                    }
                    "a" => {
                        if let Some(Some((start, href, title))) = link_starts.pop()
                            && let Some(buffer) = block_stack.last_mut()
                            && start <= buffer.text.len()
                        {
                            let text = buffer.text[start..].to_string();
                            buffer.text.truncate(start);
                            let title = title.map_or_else(String::new, |title| {
                                format!(" \"{}\"", title.replace('\\', "\\\\").replace('"', "\\\""))
                            });
                            buffer.text.push_str(&format!("[{text}]({href}{title})"));
                        }
                    }
                    _ => {}
                }
            }
            Token::Text(text) => {
                if let Some(buffer) = block_stack.last_mut() {
                    if in_pre {
                        buffer.text.push_str(&text);
                    } else {
                        let in_math = math_spans.last().is_some_and(Option::is_some);
                        let collapsed = if in_math {
                            text
                        } else {
                            collapse_whitespace(&text)
                        };
                        if !in_math
                            && (buffer.text.ends_with("[x]") || buffer.text.ends_with("[ ]"))
                            && !collapsed.starts_with(' ')
                        {
                            buffer.text.push(' ');
                        }
                        let rendered = if in_math || buffer.is_cell {
                            collapsed
                        } else {
                            markdown_escape(&collapsed, MarkdownEscapeContext::Html)
                        };
                        buffer.text.push_str(&rendered);
                    }
                }
            }
        }
    }

    let trimmed = output.trim_end();
    let mut markdown = if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    };
    if !footnote_definitions.is_empty() {
        markdown = markdown.trim_end_matches('\n').to_string();
        if !markdown.is_empty() {
            markdown.push_str("\n\n");
        }
        markdown.push_str(&footnote_definitions.join("\n\n"));
    }
    Ok(markdown)
}

fn reject_unsupported_table_span(attributes: &[(String, String)]) -> Result<(), MarkoffError> {
    for name in ["colspan", "rowspan"] {
        let Some(value) = html_attribute(attributes, name) else {
            continue;
        };
        let span = value.parse::<usize>().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid HTML table {name} value `{value}`"),
            )
        })?;
        if span != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "HTML table {name}={span} is unsupported because Markdown cannot preserve merged cells"
                ),
            )
            .into());
        }
    }
    Ok(())
}

fn html_attribute(attributes: &[(String, String)], key: &str) -> Option<String> {
    attributes
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
}

fn has_html_class(attributes: &[(String, String)], class: &str) -> bool {
    html_attribute(attributes, "class")
        .is_some_and(|classes| classes.split_ascii_whitespace().any(|value| value == class))
}

fn table_alignment_from_style(style: &str) -> Option<&'static str> {
    let alignment = style.split(';').find_map(|declaration| {
        let (property, value) = declaration.split_once(':')?;
        property
            .trim()
            .eq_ignore_ascii_case("text-align")
            .then(|| value.trim().to_ascii_lowercase())
    })?;
    match alignment.as_str() {
        "left" => Some("left"),
        "center" => Some("center"),
        "right" => Some("right"),
        _ => None,
    }
}

fn format_table_alignment(table: &str, alignments: &[Option<&str>]) -> String {
    if !alignments.iter().any(Option::is_some) {
        return table.to_string();
    }
    let mut lines = table.lines().map(str::to_string).collect::<Vec<_>>();
    let Some(header) = lines.first() else {
        return table.to_string();
    };
    let columns = header.trim().trim_matches('|').split('|').count();
    let separator = (0..columns)
        .map(|index| match alignments.get(index).copied().flatten() {
            Some("left") => ":---",
            Some("center") => ":---:",
            Some("right") => "---:",
            _ => "---",
        })
        .collect::<Vec<_>>()
        .join(" | ");
    if lines.len() > 1 {
        lines[1] = format!("| {separator} |");
    }
    lines.join("\n")
}

fn format_footnote_definition(label: &str, body: &str) -> String {
    let mut lines = body.lines();
    let mut rendered = format!("[^{label}]: {}", lines.next().unwrap_or_default());
    for line in lines {
        if line.is_empty() {
            rendered.push_str("\n\n");
        } else {
            rendered.push_str("\n    ");
            rendered.push_str(line);
        }
    }
    rendered
}
