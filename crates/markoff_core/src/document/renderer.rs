use crate::Format;
use crate::MarkoffError;
use crate::document_model::{Block, Document, Inline, ListItem, TableAlignment};
use crate::error::invalid_data;
use crate::tables::markdown_table_from_rows;
use crate::xml_utils::{MarkdownEscapeContext, markdown_escape, xml_attribute_escape};
use base64::Engine as _;
use std::path::Path;

const MAX_DOCUMENT_NESTING_DEPTH: usize = 128;

pub(super) fn document_to_markdown(
    document: &Document,
    base_dir: &Path,
) -> Result<String, MarkoffError> {
    validate_document_depth(document)?;
    let mut image_index = 0usize;
    let mut rendered_blocks = Vec::with_capacity(document.blocks.len());
    for block in &document.blocks {
        rendered_blocks.push(render_block(block, base_dir, &mut image_index)?);
    }
    Ok(format!("{}\n", rendered_blocks.join("\n\n")))
}

pub(super) fn render_document(document: &Document, format: Format) -> Result<String, MarkoffError> {
    validate_document_depth(document)?;
    match format {
        Format::Json => Ok(serde_json::to_string_pretty(document).map_err(invalid_data)?),
        Format::Yaml => Ok(serde_yaml::to_string(document).map_err(invalid_data)?),
        Format::Toml => Ok(toml::to_string_pretty(document).map_err(invalid_data)?),
        _ => unreachable!("only JSON, YAML, and TOML render a document"),
    }
}

pub(super) fn parse_document(source: &str, format: Format) -> Result<Document, MarkoffError> {
    let document = match format {
        Format::Json => serde_json::from_str(source).map_err(invalid_data)?,
        Format::Yaml => serde_yaml::from_str(source).map_err(invalid_data)?,
        Format::Toml => toml::from_str(source).map_err(invalid_data)?,
        _ => unreachable!("only JSON, YAML, and TOML parse into a document"),
    };
    validate_document_depth(&document)?;
    Ok(document)
}

fn render_block(
    block: &Block,
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    match block {
        Block::Heading {
            level,
            text,
            content,
        } => Ok(format!(
            "{} {}",
            "#".repeat((*level).clamp(1, 6) as usize),
            render_content(content, text.as_deref(), base_dir, image_index)?
        )),
        Block::ListItem {
            ordered,
            level,
            number,
            text,
            content,
        } => {
            let indentation = "    ".repeat(*level);
            let marker = if *ordered {
                format!("{}. ", number.unwrap_or(1))
            } else {
                "- ".to_string()
            };
            Ok(format!(
                "{indentation}{marker}{}",
                render_content(content, text.as_deref(), base_dir, image_index)?
            ))
        }
        Block::List {
            ordered,
            start,
            items,
        } => render_list(*ordered, *start, items, base_dir, image_index, ""),
        Block::Table {
            rows,
            cells,
            alignments,
        } => render_table(rows.as_deref(), cells, alignments, base_dir, image_index),
        Block::Code { code, info } => {
            let fence = code_fence(code, 2);
            let info = info.as_deref().unwrap_or_default();
            Ok(format!("{fence}{info}\n{code}\n{fence}"))
        }
        Block::Math { text } => Ok(format!("$$\n{text}\n$$")),
        Block::Quote { text, blocks } => {
            let quoted = if blocks.is_empty() {
                text.clone().unwrap_or_default()
            } else {
                render_blocks(blocks, base_dir, image_index)?.join("\n\n")
            };
            Ok(quoted
                .lines()
                .map(|line| {
                    if line.is_empty() {
                        ">".to_string()
                    } else {
                        format!("> {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Block::HorizontalRule => Ok("---".to_string()),
        Block::Image { alt, format, data } => {
            let destination = write_image(data, format, base_dir, image_index)?;
            Ok(render_image_link(alt, &destination, None))
        }
        Block::Paragraph { text, content } => {
            render_content(content, text.as_deref(), base_dir, image_index)
        }
        Block::FootnoteDefinition { label, blocks } => {
            let rendered = render_blocks(blocks, base_dir, image_index)?;
            let body = rendered.join("\n\n");
            if body.is_empty() {
                return Ok(format!("[^{label}]:"));
            }
            let first_is_paragraph = matches!(blocks.first(), Some(Block::Paragraph { .. }));
            if first_is_paragraph {
                let mut lines = body.lines();
                let first_line = lines.next().unwrap_or_default();
                let mut output = format!("[^{label}]: {first_line}");
                for line in lines {
                    if line.is_empty() {
                        output.push('\n');
                    } else {
                        output.push_str("\n    ");
                        output.push_str(line);
                    }
                }
                Ok(output)
            } else {
                Ok(format!("[^{label}]:\n{}", indent_lines(&body, "    ")))
            }
        }
        Block::Html { html } => Ok(html.clone()),
    }
}

fn render_blocks(
    blocks: &[Block],
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<Vec<String>, MarkoffError> {
    blocks
        .iter()
        .map(|block| render_block(block, base_dir, image_index))
        .collect()
}

fn render_content(
    content: &[Inline],
    legacy_text: Option<&str>,
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    if content.is_empty() {
        return Ok(legacy_text.unwrap_or_default().to_string());
    }
    let rendered = content
        .iter()
        .map(|inline| render_inline(inline, base_dir, image_index))
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.concat())?;
    Ok(legacy_text.map_or(rendered.clone(), |text| format!("{text}{rendered}")))
}

fn validate_document_depth(document: &Document) -> Result<(), MarkoffError> {
    let mut blocks = document
        .blocks
        .iter()
        .map(|block| (block, 0usize))
        .collect::<Vec<_>>();
    let mut inlines = Vec::new();

    while let Some((block, depth)) = blocks.pop() {
        check_depth(depth)?;
        match block {
            Block::Heading { content, .. }
            | Block::ListItem { content, .. }
            | Block::Paragraph { content, .. } => {
                inlines.extend(content.iter().map(|inline| (inline, depth + 1)));
            }
            Block::List { items, .. } => {
                for item in items {
                    blocks.extend(item.blocks.iter().map(|block| (block, depth + 1)));
                }
            }
            Block::Table { cells, .. } => {
                for row in cells {
                    for cell in row {
                        inlines.extend(cell.iter().map(|inline| (inline, depth + 1)));
                    }
                }
            }
            Block::Quote { blocks: nested, .. }
            | Block::FootnoteDefinition { blocks: nested, .. } => {
                blocks.extend(nested.iter().map(|block| (block, depth + 1)));
            }
            Block::Code { .. }
            | Block::Math { .. }
            | Block::HorizontalRule
            | Block::Image { .. }
            | Block::Html { .. } => {}
        }
    }

    while let Some((inline, depth)) = inlines.pop() {
        check_depth(depth)?;
        let content = match inline {
            Inline::Emphasis { content }
            | Inline::Strong { content }
            | Inline::Strikethrough { content }
            | Inline::Underline { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. }
            | Inline::Footnote { content } => Some(content),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Math { .. }
            | Inline::Image { .. }
            | Inline::FootnoteReference { .. }
            | Inline::Bookmark { .. }
            | Inline::SoftBreak
            | Inline::HardBreak
            | Inline::TaskListMarker { .. }
            | Inline::Html { .. } => None,
        };
        if let Some(content) = content {
            inlines.extend(content.iter().map(|inline| (inline, depth + 1)));
        }
    }

    Ok(())
}

fn check_depth(depth: usize) -> Result<(), MarkoffError> {
    if depth > MAX_DOCUMENT_NESTING_DEPTH {
        return Err(MarkoffError::InvalidInput {
            path: format!(
                "document nesting exceeds the supported depth of {MAX_DOCUMENT_NESTING_DEPTH}"
            ),
        });
    }
    Ok(())
}

fn render_inline(
    inline: &Inline,
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    match inline {
        Inline::Text { text } => Ok(escape_plain_text(text)),
        Inline::Emphasis { content } => Ok(format!(
            "*{}*",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Strong { content } => Ok(format!(
            "**{}**",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Strikethrough { content } => Ok(format!(
            "~~{}~~",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Underline { content } => Ok(format!(
            "<u>{}</u>",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Superscript { content } => Ok(format!(
            "$^{{{}}}$",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Subscript { content } => Ok(format!(
            "$_{{{}}}$",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Code { text } => {
            let fence = code_fence(text, 0);
            let padding = !text.is_empty()
                && (text.starts_with(' ')
                    && text.ends_with(' ')
                    && text.chars().any(|character| character != ' ')
                    || text.starts_with('`')
                    || text.ends_with('`'));
            let padding = if padding { " " } else { "" };
            Ok(format!("{fence}{padding}{text}{padding}{fence}"))
        }
        Inline::Math { text, display } => {
            if *display {
                Ok(format!("$${text}$$"))
            } else {
                Ok(format!("${text}$"))
            }
        }
        Inline::Link {
            destination,
            title,
            content,
        } => Ok(render_link(
            &render_content(content, None, base_dir, image_index)?,
            destination,
            title.as_deref(),
        )),
        Inline::Image {
            alt,
            destination,
            title,
            format,
            data,
        } => {
            let destination = if let (Some(format), Some(data)) = (format, data) {
                write_image(data, format, base_dir, image_index)?
            } else {
                destination.clone()
            };
            Ok(render_image_link(alt, &destination, title.as_deref()))
        }
        Inline::FootnoteReference { label } => Ok(format!("[^{label}]")),
        Inline::Footnote { content } => Ok(format!(
            "^[{}]",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Bookmark { name } => Ok(format!("<a id=\"{}\"></a>", xml_attribute_escape(name))),
        Inline::SoftBreak => Ok(" ".to_string()),
        Inline::HardBreak => Ok("  \n".to_string()),
        Inline::TaskListMarker { checked } => {
            Ok(if *checked { "[x] " } else { "[ ] " }.to_string())
        }
        Inline::Html { html } => Ok(html.clone()),
    }
}

fn escape_plain_text(text: &str) -> String {
    markdown_escape(text, MarkdownEscapeContext::Docx)
        .replace('_', "\\_")
        .replace('>', "\\>")
}

fn code_fence(text: &str, minimum: usize) -> String {
    let mut longest = 0;
    let mut current = 0;
    for character in text.chars() {
        if character == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    "`".repeat(longest.max(minimum) + 1)
}

fn render_link(label: &str, destination: &str, title: Option<&str>) -> String {
    let destination = if destination
        .chars()
        .any(|character| character.is_whitespace() || character == '<' || character == '>')
    {
        format!("<{}>", destination.replace('>', "\\>"))
    } else {
        destination.replace('\\', "\\\\").replace(')', "\\)")
    };
    let title = title.map_or_else(String::new, |title| {
        format!(" \"{}\"", title.replace('\\', "\\\\").replace('"', "\\\""))
    });
    format!("[{label}]({destination}{title})")
}

fn render_image_link(alt: &str, destination: &str, title: Option<&str>) -> String {
    let link = render_link(&escape_plain_text(alt), destination, title);
    format!("!{}", link)
}

fn write_image(
    data: &str,
    format: &str,
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    *image_index += 1;
    let file_name = format!("image{image_index}.{format}");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(invalid_data)?;
    let image_dir = base_dir.join("image");
    std::fs::create_dir_all(&image_dir)?;
    std::fs::write(image_dir.join(&file_name), bytes)?;
    Ok(format!("image/{file_name}"))
}

fn render_table(
    legacy_rows: Option<&[Vec<String>]>,
    cells: &[Vec<Vec<Inline>>],
    alignments: &[TableAlignment],
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    if cells.is_empty() {
        return Ok(legacy_rows.map_or_else(String::new, markdown_table_from_rows));
    }
    let rows = cells
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| render_content(cell, None, base_dir, image_index))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut rendered = markdown_table_from_rows(&rows);
    if alignments
        .iter()
        .any(|alignment| *alignment != TableAlignment::None)
    {
        let mut lines = rendered.lines().map(str::to_string).collect::<Vec<_>>();
        if let Some(header) = rows.first() {
            let separator = header
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    match alignments
                        .get(index)
                        .copied()
                        .unwrap_or(TableAlignment::None)
                    {
                        TableAlignment::None => "---".to_string(),
                        TableAlignment::Left => ":---".to_string(),
                        TableAlignment::Center => ":---:".to_string(),
                        TableAlignment::Right => "---:".to_string(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" | ");
            if lines.len() > 1 {
                lines[1] = format!("| {separator} |");
            }
        }
        rendered = lines.join("\n");
    }
    Ok(rendered)
}

fn render_list(
    ordered: bool,
    start: Option<u64>,
    items: &[ListItem],
    base_dir: &Path,
    image_index: &mut usize,
    indent: &str,
) -> Result<String, MarkoffError> {
    let mut next_number = start.unwrap_or(1);
    let mut rendered_items = Vec::with_capacity(items.len());
    for item in items {
        let number = item.number.unwrap_or(next_number);
        let marker = if ordered {
            format!("{number}. ")
        } else {
            "- ".to_string()
        };
        next_number = number.saturating_add(1);
        let item_prefix = format!("{indent}{marker}");
        let continuation_indent = " ".repeat(item_prefix.len());
        let nested_indent = format!("{indent}    ");
        let mut item_blocks = item.blocks.iter();
        let mut rendered = String::new();
        if let Some(first) = item_blocks.next() {
            if matches!(first, Block::Paragraph { .. }) {
                let paragraph = render_block(first, base_dir, image_index)?;
                rendered.push_str(&indent_lines_with_first(
                    &paragraph,
                    &item_prefix,
                    &continuation_indent,
                ));
            } else {
                rendered.push_str(item_prefix.trim_end());
                let child = indent_block(first, &nested_indent, base_dir, image_index)?;
                if !child.is_empty() {
                    rendered.push('\n');
                    rendered.push_str(&child);
                }
            }
        } else {
            rendered.push_str(item_prefix.trim_end());
        }
        for block in item_blocks {
            let child = match block {
                Block::List {
                    ordered,
                    start,
                    items,
                } => render_list(
                    *ordered,
                    *start,
                    items,
                    base_dir,
                    image_index,
                    &nested_indent,
                )?,
                _ => indent_block(block, &nested_indent, base_dir, image_index)?,
            };
            if !child.is_empty() {
                if !rendered.is_empty() {
                    rendered.push_str(if matches!(block, Block::List { .. }) {
                        "\n"
                    } else {
                        "\n\n"
                    });
                }
                rendered.push_str(&child);
            }
        }
        rendered_items.push(rendered);
    }
    Ok(rendered_items.join("\n"))
}

fn indent_block(
    block: &Block,
    indent: &str,
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    let markdown = render_block(block, base_dir, image_index)?;
    Ok(indent_lines(&markdown, indent))
}

fn indent_lines(markdown: &str, indent: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn indent_lines_with_first(markdown: &str, first: &str, continuation: &str) -> String {
    let mut lines = markdown.lines();
    let Some(first_line) = lines.next() else {
        return first.trim_end().to_string();
    };
    let mut rendered = format!("{first}{first_line}");
    for line in lines {
        rendered.push('\n');
        if !line.is_empty() {
            rendered.push_str(continuation);
            rendered.push_str(line);
        }
    }
    rendered
}
