use super::super::*;
use super::inline::{
    append_inline_content, append_inline_content_with_prefix, inline_plain_text,
    inline_runs_without_images, plain_run,
};
use crate::document_model::Block;
use crate::error::invalid_data;
use base64::Engine as _;

pub(in crate::pdf_writer) fn append_blocks(
    blocks: &[Block],
    indent: f32,
    quote_depth: usize,
    output: &mut Vec<PdfElement>,
) -> Result<(), MarkoffError> {
    for block in blocks {
        match block {
            Block::Heading {
                level,
                text,
                content,
            } => {
                let title = inline_plain_text(content, text.as_deref())?;
                let style = InlineStyle {
                    bold: true,
                    ..InlineStyle::default()
                };
                let options = ParagraphOptions {
                    font_size: heading_font_size(*level),
                    indent,
                    space_before: 10.0,
                    space_after: 6.0,
                    preserve_whitespace: false,
                    quote_depth,
                    alignment: TextAlignment::Left,
                    color: HEADING_COLOR,
                    heading: Some((*level, title)),
                };
                append_inline_content(content, text.as_deref(), style, options, output)?;
            }
            Block::ListItem {
                ordered,
                level,
                number,
                text,
                content,
            } => {
                let marker = if *ordered {
                    format!("{}. ", number.unwrap_or(1))
                } else {
                    "• ".to_string()
                };
                let prefix = TextRun {
                    text: marker,
                    style: InlineStyle::default(),
                };
                let extra_indent = *level as f32 * 18.0;
                let options = paragraph_options(indent + extra_indent, quote_depth);
                append_inline_content_with_prefix(
                    content,
                    text.as_deref(),
                    vec![prefix],
                    InlineStyle::default(),
                    options,
                    output,
                )?;
            }
            Block::List {
                ordered,
                start,
                items,
            } => {
                let mut next_number = start.unwrap_or(1);
                for item in items {
                    let number = item.number.unwrap_or(next_number);
                    let marker = if *ordered {
                        format!("{number}.")
                    } else {
                        "•".to_string()
                    };
                    append_list_item(&item.blocks, &marker, indent, quote_depth, output)?;
                    next_number = number.saturating_add(1);
                }
            }
            Block::Table {
                rows,
                cells,
                alignments,
            } => {
                let rows: Vec<Vec<Vec<TextRun>>> = if cells.is_empty() {
                    rows.as_deref()
                        .unwrap_or_default()
                        .iter()
                        .map(|row| row.iter().map(|cell| vec![plain_run(cell)]).collect())
                        .collect()
                } else {
                    cells
                        .iter()
                        .map(|row| -> Result<Vec<Vec<TextRun>>, MarkoffError> {
                            row.iter()
                                .map(|cell| inline_runs_without_images(cell))
                                .collect()
                        })
                        .collect::<Result<Vec<_>, _>>()?
                };
                output.push(PdfElement::Table(PdfTable {
                    rows,
                    alignments: alignments.clone(),
                    indent,
                }));
            }
            Block::Code { code, info } => output.push(PdfElement::CodeBlock {
                code: code.clone(),
                info: info.clone(),
                indent,
            }),
            Block::Math { text } => {
                let style = InlineStyle {
                    italic: true,
                    ..InlineStyle::default()
                };
                let options = ParagraphOptions {
                    font_size: 12.0,
                    indent,
                    space_before: 2.0,
                    space_after: 8.0,
                    preserve_whitespace: false,
                    quote_depth,
                    alignment: TextAlignment::Center,
                    color: PdfColor::BLACK,
                    heading: None,
                };
                append_inline_content(&[], Some(text), style, options, output)?;
            }
            Block::Quote { text, blocks } => {
                if blocks.is_empty() {
                    let options = ParagraphOptions {
                        font_size: 11.0,
                        indent: indent + 14.0,
                        space_before: 0.0,
                        space_after: 6.0,
                        preserve_whitespace: false,
                        quote_depth: quote_depth + 1,
                        alignment: TextAlignment::Left,
                        color: PdfColor::GREY_20,
                        heading: None,
                    };
                    append_inline_content(
                        &[],
                        text.as_deref(),
                        InlineStyle::default(),
                        options,
                        output,
                    )?;
                } else {
                    append_blocks(blocks, indent + 14.0, quote_depth + 1, output)?;
                }
            }
            Block::HorizontalRule => {
                output.push(PdfElement::HorizontalRule { indent });
            }
            Block::Image {
                alt,
                format: _,
                data,
            } => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(invalid_data)?;
                output.push(PdfElement::Image(PdfImage {
                    bytes,
                    alt: alt.clone(),
                    indent,
                    link: None,
                }));
            }
            Block::Paragraph { text, content } => {
                let options = paragraph_options(indent, quote_depth);
                append_inline_content(
                    content,
                    text.as_deref(),
                    InlineStyle::default(),
                    options,
                    output,
                )?;
            }
            Block::FootnoteDefinition { label, blocks } => {
                output.push(PdfElement::Anchor(format!("fn-{label}")));
                let style = InlineStyle {
                    bold: true,
                    ..InlineStyle::default()
                };
                let options = ParagraphOptions {
                    font_size: 9.0,
                    indent: indent + 8.0,
                    space_before: 3.0,
                    space_after: 2.0,
                    preserve_whitespace: false,
                    quote_depth,
                    alignment: TextAlignment::Left,
                    color: PdfColor::GREY_20,
                    heading: None,
                };
                append_inline_content(&[], Some(&format!("[^{label}]")), style, options, output)?;
                append_blocks(blocks, indent + 18.0, quote_depth, output)?;
            }
            Block::Html { html } => {
                let options = paragraph_options(indent, quote_depth);
                append_inline_content(
                    &[],
                    Some(&strip_html_tags(html)),
                    InlineStyle::default(),
                    options,
                    output,
                )?;
            }
        }
    }
    Ok(())
}

fn append_list_item(
    blocks: &[Block],
    marker: &str,
    indent: f32,
    quote_depth: usize,
    output: &mut Vec<PdfElement>,
) -> Result<(), MarkoffError> {
    let Some((first, rest)) = blocks.split_first() else {
        append_inline_content(
            &[],
            Some(marker),
            InlineStyle::default(),
            paragraph_options(indent, quote_depth),
            output,
        )?;
        return Ok(());
    };

    match first {
        Block::Paragraph { text, content } => {
            append_inline_content_with_prefix(
                content,
                text.as_deref(),
                vec![plain_run(&format!("{marker} "))],
                InlineStyle::default(),
                paragraph_options(indent, quote_depth),
                output,
            )?;
        }
        _ => {
            append_inline_content(
                &[],
                Some(marker),
                InlineStyle::default(),
                paragraph_options(indent, quote_depth),
                output,
            )?;
            append_blocks(
                std::slice::from_ref(first),
                indent + 18.0,
                quote_depth,
                output,
            )?;
        }
    }
    append_blocks(rest, indent + 18.0, quote_depth, output)
}

fn paragraph_options(indent: f32, quote_depth: usize) -> ParagraphOptions {
    ParagraphOptions {
        font_size: 11.0,
        indent,
        space_before: 0.0,
        space_after: 6.0,
        preserve_whitespace: false,
        quote_depth,
        alignment: TextAlignment::Left,
        color: PdfColor::BLACK,
        heading: None,
    }
}

fn heading_font_size(level: u8) -> f32 {
    match level {
        1 => 25.0,
        2 => 20.0,
        3 => 17.0,
        4 => 14.0,
        5 => 12.0,
        _ => 11.0,
    }
}

pub(super) fn strip_html_tags(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut inside_tag = false;
    for character in html.chars() {
        match character {
            '<' => inside_tag = true,
            '>' if inside_tag => inside_tag = false,
            _ if !inside_tag => text.push(character),
            _ => {}
        }
    }
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
