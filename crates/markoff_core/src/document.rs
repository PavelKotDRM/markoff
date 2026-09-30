use crate::document_model::{Block, Document};
use crate::{Format, MarkoffError};
use std::path::Path;

mod inline_parser;
mod markdown_parser;
mod renderer;
mod structured;

pub(crate) use inline_parser::expand_inline_footnotes;
use markdown_parser::markdown_to_document;
use renderer::{document_to_markdown, parse_document, render_document};

pub(crate) fn convert_structured_data_format(
    input: &Path,
    output: &Path,
    from: Format,
    to: Format,
) -> Result<(), MarkoffError> {
    structured::convert_structured_data_format(input, output, from, to)
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
