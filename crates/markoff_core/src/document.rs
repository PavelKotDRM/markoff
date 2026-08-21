use crate::Format;
use crate::MarkoffError;
use crate::docx_inline::markdown_list_item;
use crate::error::invalid_data;
use crate::tables::{markdown_table_from_rows, parse_markdown_table};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A single block-level element of a document. This is the schema used to
/// represent a whole Markdown/DOCX document (not just its tables) in JSON,
/// YAML, and TOML.
///
/// Inline formatting (bold, italic, links, footnote references, ...) is kept
/// as raw Markdown text inside a block's `text` field rather than modeled
/// separately, and literal list numbers are not preserved (only order and
/// nesting), consistent with the existing Markdown/DOCX conversion behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Block {
    /// A heading, e.g. `# Title`.
    Heading {
        /// Heading level from 1 (`#`) to 6 (`######`).
        level: u8,
        /// Heading text; may include inline Markdown formatting.
        text: String,
    },
    /// A single bulleted or numbered list item.
    ListItem {
        /// `true` for a numbered item, `false` for a bulleted item.
        ordered: bool,
        /// Nesting depth, starting at 0 for a top-level item.
        level: usize,
        /// Item text; may include inline Markdown formatting.
        text: String,
    },
    /// A table; the first row is the header row.
    Table {
        /// Table rows, including the header row.
        rows: Vec<Vec<String>>,
    },
    /// A fenced code block.
    CodeBlock {
        /// Raw code block content.
        code: String,
    },
    /// A blockquote.
    Quote {
        /// Quote text; may include inline Markdown formatting.
        text: String,
    },
    /// A horizontal rule (`---`).
    HorizontalRule,
    /// An embedded image, stored inline as base64 rather than a file
    /// reference so JSON/YAML/TOML documents are self-contained.
    Image {
        /// Alt text; may be empty.
        alt: String,
        /// Lowercase file extension without a dot, e.g. `png`.
        format: String,
        /// Standard base64-encoded image bytes.
        data: String,
    },
    /// Any other paragraph, including footnote definitions and text that
    /// does not match a more specific block type.
    Paragraph {
        /// Paragraph text; may include inline Markdown formatting.
        text: String,
    },
}

/// A whole document represented as an ordered sequence of blocks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Document {
    blocks: Vec<Block>,
}

pub(crate) fn convert_markdown_to_document(
    input: &Path,
    output: &Path,
    format: Format,
) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let base_dir = input.parent().filter(|path| !path.as_os_str().is_empty());
    let document = markdown_to_document(&source, base_dir);
    let rendered = render_document(&document, format)?;
    std::fs::write(output, rendered)?;
    Ok(())
}

pub(crate) fn convert_document_to_markdown(
    input: &Path,
    output: &Path,
    format: Format,
) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let document = parse_document(&source, format)?;
    let base_dir = output.parent().filter(|path| !path.as_os_str().is_empty());
    std::fs::write(output, document_to_markdown(&document, base_dir)?)?;
    Ok(())
}

fn markdown_to_document(markdown: &str, base_dir: Option<&Path>) -> Document {
    Document {
        blocks: split_into_raw_blocks(markdown)
            .into_iter()
            .map(|text| classify_block(text, base_dir))
            .collect(),
    }
}

fn document_to_markdown(
    document: &Document,
    base_dir: Option<&Path>,
) -> Result<String, MarkoffError> {
    let mut image_index = 0usize;
    let mut rendered_blocks = Vec::with_capacity(document.blocks.len());
    for block in &document.blocks {
        rendered_blocks.push(render_block(block, base_dir, &mut image_index)?);
    }
    Ok(format!("{}\n", rendered_blocks.join("\n\n")))
}

fn render_document(document: &Document, format: Format) -> Result<String, MarkoffError> {
    match format {
        Format::Json => Ok(serde_json::to_string_pretty(document).map_err(invalid_data)?),
        Format::Yaml => Ok(serde_yaml::to_string(document).map_err(invalid_data)?),
        Format::Toml => Ok(toml::to_string_pretty(document).map_err(invalid_data)?),
        _ => unreachable!("only JSON, YAML, and TOML render a document"),
    }
}

fn parse_document(source: &str, format: Format) -> Result<Document, MarkoffError> {
    match format {
        Format::Json => Ok(serde_json::from_str(source).map_err(invalid_data)?),
        Format::Yaml => Ok(serde_yaml::from_str(source).map_err(invalid_data)?),
        Format::Toml => Ok(toml::from_str(source).map_err(invalid_data)?),
        _ => unreachable!("only JSON, YAML, and TOML parse into a document"),
    }
}

/// Splits Markdown source into the same paragraph-level blocks the DOCX
/// writer already treats as one unit, merging an indented continuation line
/// (footnote/quote continuation) back into the preceding block instead of
/// splitting it out, since it is not itself a recognizable list item.
fn split_into_raw_blocks(markdown: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    for paragraph in markdown.split("\n\n") {
        let paragraph = paragraph.trim_end_matches('\n');
        if paragraph.is_empty() {
            continue;
        }
        let first_line = paragraph.lines().next().unwrap_or("");
        let is_continuation =
            first_line.starts_with("    ") && markdown_list_item(first_line).is_none();
        if is_continuation && let Some(previous) = blocks.last_mut() {
            previous.push_str("\n\n");
            previous.push_str(paragraph);
        } else {
            blocks.push(paragraph.to_string());
        }
    }
    blocks
}

fn classify_block(text: String, base_dir: Option<&Path>) -> Block {
    let first_line = text.lines().next().unwrap_or("");

    let heading_level = first_line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if (1..=6).contains(&heading_level) && first_line.as_bytes().get(heading_level) == Some(&b' ') {
        return Block::Heading {
            level: heading_level as u8,
            text: first_line[heading_level + 1..].to_string(),
        };
    }

    if text.trim() == "---" {
        return Block::HorizontalRule;
    }

    if let Some((alt, path)) = parse_image_only(text.trim())
        && let Some(image) = read_image(base_dir, &alt, path)
    {
        return image;
    }

    if is_markdown_table(&text) {
        return Block::Table {
            rows: parse_markdown_table(&text),
        };
    }

    if let Some(code) = text
        .strip_prefix("```")
        .and_then(|rest| rest.strip_suffix("```"))
    {
        let code = code.strip_prefix('\n').unwrap_or(code);
        let code = code.strip_suffix('\n').unwrap_or(code);
        return Block::CodeBlock {
            code: code.to_string(),
        };
    }

    if first_line.starts_with("> ") {
        let quoted = text
            .lines()
            .map(|line| line.strip_prefix("> ").unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");
        return Block::Quote { text: quoted };
    }

    if let Some((kind, level, content)) = markdown_list_item(first_line) {
        let rest = &text[first_line.len()..];
        return Block::ListItem {
            ordered: kind == 2,
            level,
            text: format!("{content}{rest}"),
        };
    }

    Block::Paragraph { text }
}

fn is_markdown_table(text: &str) -> bool {
    let mut lines = text.lines();
    let Some(header) = lines.next().map(str::trim) else {
        return false;
    };
    let Some(separator) = lines.next().map(str::trim) else {
        return false;
    };
    header.starts_with('|') && header.ends_with('|') && separator.contains("---")
}

/// Parses a paragraph that is *only* a Markdown image (`![alt](path)`, no
/// other surrounding text), returning `(alt, path)`.
fn parse_image_only(text: &str) -> Option<(String, &str)> {
    let rest = text.strip_prefix("![")?;
    let (alt, rest) = rest.split_once("](")?;
    let path = rest.strip_suffix(')')?;
    (!path.is_empty()).then(|| (alt.to_string(), path))
}

fn read_image(base_dir: Option<&Path>, alt: &str, path: &str) -> Option<Block> {
    let base_dir = base_dir?;
    let bytes = std::fs::read(base_dir.join(path)).ok()?;
    let format = Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("png")
        .to_ascii_lowercase();
    Some(Block::Image {
        alt: alt.to_string(),
        format,
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

fn render_block(
    block: &Block,
    base_dir: Option<&Path>,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    match block {
        Block::Heading { level, text } => Ok(format!(
            "{} {text}",
            "#".repeat((*level).clamp(1, 6) as usize)
        )),
        Block::ListItem {
            ordered,
            level,
            text,
        } => {
            let indentation = "    ".repeat(*level);
            let marker = if *ordered { "1. " } else { "- " };
            Ok(format!("{indentation}{marker}{text}"))
        }
        Block::Table { rows } => Ok(markdown_table_from_rows(rows)),
        Block::CodeBlock { code } => Ok(format!("```\n{code}\n```")),
        Block::Quote { text } => Ok(text
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n")),
        Block::HorizontalRule => Ok("---".to_string()),
        Block::Image { alt, format, data } => {
            *image_index += 1;
            let file_name = format!("image{image_index}.{format}");
            if let Some(base_dir) = base_dir {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(invalid_data)?;
                let image_dir = base_dir.join("image");
                std::fs::create_dir_all(&image_dir)?;
                std::fs::write(image_dir.join(&file_name), bytes)?;
            }
            Ok(format!("![{alt}](image/{file_name})"))
        }
        Block::Paragraph { text } => Ok(text.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, document_to_markdown, markdown_to_document};

    #[test]
    fn round_trips_headings_lists_and_tables_through_blocks() {
        let markdown = "# Title\n\nA paragraph.\n\n- First\n\n    - Nested\n\n1. Step one\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n```\ncode line\n```\n\n> Quoted\n\n---\n";
        let document = markdown_to_document(markdown, None);
        assert_eq!(
            document.blocks,
            vec![
                Block::Heading {
                    level: 1,
                    text: "Title".to_string()
                },
                Block::Paragraph {
                    text: "A paragraph.".to_string()
                },
                Block::ListItem {
                    ordered: false,
                    level: 0,
                    text: "First".to_string()
                },
                Block::ListItem {
                    ordered: false,
                    level: 1,
                    text: "Nested".to_string()
                },
                Block::ListItem {
                    ordered: true,
                    level: 0,
                    text: "Step one".to_string()
                },
                Block::Table {
                    rows: vec![
                        vec!["A".to_string(), "B".to_string()],
                        vec!["1".to_string(), "2".to_string()],
                    ]
                },
                Block::CodeBlock {
                    code: "code line".to_string()
                },
                Block::Quote {
                    text: "Quoted".to_string()
                },
                Block::HorizontalRule,
            ]
        );
        assert_eq!(document_to_markdown(&document, None).unwrap(), markdown);
    }

    #[test]
    fn merges_footnote_continuation_into_preceding_block() {
        let markdown = "[^1]: First line.\n\n    Second paragraph.\n";
        let document = markdown_to_document(markdown, None);
        assert_eq!(
            document.blocks,
            vec![Block::Paragraph {
                text: "[^1]: First line.\n\n    Second paragraph.".to_string()
            }]
        );
    }

    #[test]
    fn round_trips_an_image_through_base64_and_back_to_a_file() {
        use base64::Engine as _;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let source_dir = std::env::temp_dir().join(format!("markoff_document_image_src_{nanos}"));
        let target_dir = std::env::temp_dir().join(format!("markoff_document_image_dst_{nanos}"));
        std::fs::create_dir_all(source_dir.join("image")).unwrap();
        let png_bytes = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        std::fs::write(source_dir.join("image").join("picture.png"), png_bytes).unwrap();

        let markdown = "![A picture](image/picture.png)\n";
        let document = markdown_to_document(markdown, Some(&source_dir));
        assert_eq!(
            document.blocks,
            vec![Block::Image {
                alt: "A picture".to_string(),
                format: "png".to_string(),
                data: base64::engine::general_purpose::STANDARD.encode(png_bytes),
            }]
        );

        let rendered = document_to_markdown(&document, Some(&target_dir)).unwrap();
        assert_eq!(rendered, "![A picture](image/image1.png)\n");
        assert_eq!(
            std::fs::read(target_dir.join("image").join("image1.png")).unwrap(),
            png_bytes
        );

        std::fs::remove_dir_all(source_dir).ok();
        std::fs::remove_dir_all(target_dir).ok();
    }
}
