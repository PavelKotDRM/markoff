use crate::document_model::{Block, Document};
use crate::style::DocumentTheme;
use crate::{Format, MarkoffError};
use std::path::Path;

mod inline_parser;
mod markdown_parser;
mod renderer;
mod structured;
pub(crate) use structured::{parse_structured_value, render_structured_value};

pub(crate) fn is_document_schema(source: &str, format: Format) -> Result<bool, MarkoffError> {
    Ok(structured::parse_structured_value(source, format)?
        .get("blocks")
        .is_some())
}

pub(crate) use inline_parser::expand_inline_footnotes;
use markdown_parser::markdown_to_document;
pub(crate) use renderer::document_to_markdown;
use renderer::{parse_document, render_document};

pub(crate) fn convert_structured_data_format(
    input: &Path,
    output: &Path,
    from: Format,
    to: Format,
) -> Result<(), MarkoffError> {
    structured::convert_structured_data_format(input, output, from, to)
}

pub(crate) fn convert_structured_data_to_pdf(
    input: &Path,
    output: &Path,
    format: Format,
    tables_only: bool,
    theme: &DocumentTheme,
) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let value = structured::parse_structured_value(&source, format)?;
    let document = if value.get("blocks").is_some() {
        let mut document = parse_document(&source, format)?;
        if tables_only {
            retain_table_blocks(&mut document);
        }
        document
    } else {
        if tables_only {
            return Err(MarkoffError::InvalidOption {
                message:
                    "--tables-only requires a JSON, YAML, or TOML document schema containing blocks"
                        .to_string(),
            });
        }
        Document {
            blocks: vec![Block::Code {
                code: source,
                info: Some(format.to_string()),
            }],
        }
    };
    crate::pdf_writer::write_document_to_pdf(&document, output, theme)
}

pub(crate) fn parse_markdown_document(
    source: &str,
    base_dir: &Path,
) -> Result<Document, MarkoffError> {
    markdown_to_document(source, base_dir)
}

pub(crate) fn convert_markdown_to_document(
    input: &Path,
    output: &Path,
    format: Format,
    tables_only: bool,
) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let mut document = markdown_to_document(&source, parent_directory(input))?;
    if tables_only {
        retain_table_blocks(&mut document);
    }
    let rendered = render_document(&document, format)?;
    std::fs::write(output, rendered)?;
    Ok(())
}

pub(crate) fn convert_document_to_markdown(
    input: &Path,
    output: &Path,
    format: Format,
    tables_only: bool,
) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    if !is_document_schema(&source, format)? && !tables_only {
        let sheets = crate::data::read_data_sheets(input, format, b',')?;
        let mut markdown = String::new();
        for (name, rows) in sheets {
            markdown.push_str(&format!("# {name}\n\n"));
            let rows = rows
                .iter()
                .map(|row| row.iter().map(|cell| cell.as_text()).collect())
                .collect::<Vec<_>>();
            markdown.push_str(&crate::tables::markdown_table_from_rows(&rows));
            markdown.push_str("\n\n");
        }
        std::fs::write(output, markdown)?;
        return Ok(());
    }
    let mut document = parse_document(&source, format)?;
    if tables_only {
        retain_table_blocks(&mut document);
    }
    std::fs::write(
        output,
        document_to_markdown(&document, parent_directory(output))?,
    )?;
    Ok(())
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn retain_table_blocks(document: &mut Document) {
    let mut retained = Vec::new();
    for mut block in std::mem::take(&mut document.blocks) {
        let keep = match &mut block {
            Block::Table { .. } => true,
            Block::List { items, .. } => {
                for item in items.iter_mut() {
                    let mut nested = Document {
                        blocks: std::mem::take(&mut item.blocks),
                    };
                    retain_table_blocks(&mut nested);
                    item.blocks = nested.blocks;
                }
                items.retain(|item| !item.blocks.is_empty());
                !items.is_empty()
            }
            Block::Quote { blocks, .. } | Block::FootnoteDefinition { blocks, .. } => {
                let mut nested = Document {
                    blocks: std::mem::take(blocks),
                };
                retain_table_blocks(&mut nested);
                *blocks = nested.blocks;
                !blocks.is_empty()
            }
            _ => false,
        };
        if keep {
            retained.push(block);
        }
    }
    document.blocks = retained;
}

#[cfg(test)]
mod tests;
