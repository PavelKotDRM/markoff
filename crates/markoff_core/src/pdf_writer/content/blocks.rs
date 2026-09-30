use super::super::pdf_color;
use super::super::*;
use super::inline::{
    append_inline_content, append_inline_content_with_prefix, inline_plain_text,
    inline_runs_without_images, plain_run,
};
use crate::document_model::Block;
use crate::error::invalid_data;
use crate::style::StyleTextAlign;
use base64::Engine as _;

/// Nesting information that controls how a block is laid out.
#[derive(Clone, Copy, Default)]
pub(in crate::pdf_writer) struct BlockContext {
    indent: f32,
    quote_depth: usize,
    in_list: bool,
    in_footnote: bool,
}

impl BlockContext {
    fn indented(self, extra: f32) -> Self {
        Self {
            indent: self.indent + extra,
            ..self
        }
    }

    fn quoted(self, theme: &DocumentTheme) -> Self {
        Self {
            indent: self.indent + theme.quote_indent_pt,
            quote_depth: self.quote_depth + 1,
            ..self
        }
    }

    fn listed(self, extra: f32) -> Self {
        Self {
            indent: self.indent + extra,
            in_list: true,
            ..self
        }
    }
}

pub(in crate::pdf_writer) fn append_blocks(
    blocks: &[Block],
    context: BlockContext,
    output: &mut Vec<PdfElement>,
    theme: &DocumentTheme,
) -> Result<(), MarkoffError> {
    let indent = context.indent;
    let quote_depth = context.quote_depth;
    for block in blocks {
        match block {
            Block::Heading {
                level,
                text,
                content,
            } => {
                let title = inline_plain_text(content, text.as_deref())?;
                let style = InlineStyle {
                    bold: theme.heading_bold,
                    italic: theme.heading_italic,
                    ..InlineStyle::default()
                };
                let options = ParagraphOptions {
                    font_size: if theme.enabled {
                        theme.heading_size_for(*level)
                    } else {
                        heading_font_size(*level)
                    },
                    indent,
                    space_before: theme.heading_spacing_before_pt,
                    space_after: theme.heading_spacing_after_pt,
                    first_line_indent: 0.0,
                    preserve_whitespace: false,
                    quote_depth,
                    alignment: TextAlignment::Left,
                    color: if theme.enabled {
                        pdf_color(theme.heading_color_for(*level))
                    } else {
                        PdfColor::new(31, 57, 86, 255)
                    },
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
                    format!("{} ", theme.list_bullet)
                };
                let prefix = TextRun {
                    text: marker,
                    style: InlineStyle::default(),
                };
                let item_context = context.listed(*level as f32 * theme.list_indent_pt);
                let options = paragraph_options(item_context, theme);
                append_inline_content_with_prefix(
                    content,
                    text.as_deref(),
                    vec![prefix],
                    body_style(item_context, theme),
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
                        theme.list_bullet.clone()
                    };
                    append_list_item(&item.blocks, &marker, context, output, theme)?;
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
                    first_line_indent: 0.0,
                    preserve_whitespace: false,
                    quote_depth,
                    alignment: TextAlignment::Center,
                    color: PdfColor::BLACK,
                    heading: None,
                };
                append_inline_content(&[], Some(text), style, options, output)?;
            }
            Block::Quote { text, blocks } => {
                let quote_context = context.quoted(theme);
                if blocks.is_empty() {
                    let options = if theme.enabled {
                        paragraph_options(quote_context, theme)
                    } else {
                        ParagraphOptions {
                            font_size: 11.0,
                            indent: quote_context.indent,
                            space_before: 0.0,
                            space_after: 6.0,
                            first_line_indent: 0.0,
                            preserve_whitespace: false,
                            quote_depth: quote_context.quote_depth,
                            alignment: TextAlignment::Left,
                            color: PdfColor::GREY_20,
                            heading: None,
                        }
                    };
                    append_inline_content(
                        &[],
                        text.as_deref(),
                        body_style(quote_context, theme),
                        options,
                        output,
                    )?;
                } else {
                    append_blocks(blocks, quote_context, output, theme)?;
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
                let options = paragraph_options(context, theme);
                append_inline_content(
                    content,
                    text.as_deref(),
                    body_style(context, theme),
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
                    font_size: theme.footnote_font_size_pt,
                    indent: indent + 8.0,
                    space_before: 3.0,
                    space_after: 2.0,
                    first_line_indent: 0.0,
                    preserve_whitespace: false,
                    quote_depth,
                    alignment: TextAlignment::Left,
                    color: PdfColor::GREY_20,
                    heading: None,
                };
                append_inline_content(&[], Some(&format!("[^{label}]")), style, options, output)?;
                let footnote_context = BlockContext {
                    in_footnote: true,
                    ..context.indented(18.0)
                };
                append_blocks(blocks, footnote_context, output, theme)?;
            }
            Block::Html { html } => {
                let options = paragraph_options(context, theme);
                append_inline_content(
                    &[],
                    Some(&strip_html_tags(html)),
                    body_style(context, theme),
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
    context: BlockContext,
    output: &mut Vec<PdfElement>,
    theme: &DocumentTheme,
) -> Result<(), MarkoffError> {
    let item_context = BlockContext {
        in_list: true,
        ..context
    };
    let nested_context = context.listed(theme.list_indent_pt);
    let Some((first, rest)) = blocks.split_first() else {
        append_inline_content(
            &[],
            Some(marker),
            InlineStyle::default(),
            paragraph_options(item_context, theme),
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
                body_style(item_context, theme),
                paragraph_options(item_context, theme),
                output,
            )?;
        }
        _ => {
            append_inline_content(
                &[],
                Some(marker),
                InlineStyle::default(),
                paragraph_options(item_context, theme),
                output,
            )?;
            append_blocks(std::slice::from_ref(first), nested_context, output, theme)?;
        }
    }
    append_blocks(rest, nested_context, output, theme)
}

fn body_style(context: BlockContext, theme: &DocumentTheme) -> InlineStyle {
    InlineStyle {
        italic: theme.enabled && context.quote_depth > 0 && theme.quote_italic,
        ..InlineStyle::default()
    }
}

fn paragraph_options(context: BlockContext, theme: &DocumentTheme) -> ParagraphOptions {
    let plain_body = context.quote_depth == 0 && !context.in_list && !context.in_footnote;
    ParagraphOptions {
        font_size: if theme.enabled && context.in_footnote {
            theme.footnote_font_size_pt
        } else {
            theme.font_size_pt
        },
        indent: context.indent,
        space_before: theme.paragraph_spacing_before_pt,
        space_after: if theme.enabled {
            theme.paragraph_spacing_after_pt
        } else {
            6.0
        },
        first_line_indent: if plain_body {
            theme.first_line_indent_pt
        } else {
            0.0
        },
        preserve_whitespace: false,
        quote_depth: context.quote_depth,
        alignment: match theme.text_align {
            StyleTextAlign::Left => TextAlignment::Left,
            StyleTextAlign::Center => TextAlignment::Center,
            StyleTextAlign::Right => TextAlignment::Right,
            StyleTextAlign::Justify => TextAlignment::Justify,
        },
        color: if theme.enabled && context.quote_depth > 0 {
            pdf_color(theme.quote_text_color)
        } else {
            pdf_color(theme.text_color)
        },
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
