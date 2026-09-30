use crate::MarkoffError;
use crate::document_model::{Block, Document, Inline, TableAlignment};
use crate::error::invalid_data;
use base64::Engine as _;
use epaint_default_fonts::{HACK_REGULAR, UBUNTU_LIGHT};
use pdfium_bundled::pdfium_render::prelude::{
    PdfColor, PdfDocument, PdfFontToken, PdfPage, PdfPageAnnotationCommon,
    PdfPageContentRegenerationStrategy, PdfPageObjectCommon, PdfPageObjectsCommon,
    PdfPagePaperSize, PdfPageTextRenderMode, PdfPoints, PdfRect, Pdfium,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

const PDF_MARGIN_LEFT: f32 = 42.0;
const PDF_MARGIN_RIGHT: f32 = 42.0;
const PDF_MARGIN_TOP: f32 = 42.0;
const PDF_MARGIN_BOTTOM: f32 = 42.0;
const INTERNAL_LINK_PREFIX: &str = "markoff-internal:";
const CODE_BACKGROUND: PdfColor = PdfColor::new(246, 248, 250, 255);
const TABLE_HEADER_BACKGROUND: PdfColor = PdfColor::new(235, 240, 246, 255);
const TABLE_BORDER: PdfColor = PdfColor::new(180, 188, 198, 255);
const HEADING_COLOR: PdfColor = PdfColor::new(31, 57, 86, 255);
const LINK_COLOR: PdfColor = PdfColor::new(28, 86, 150, 255);

#[derive(Clone, Default, PartialEq, Eq)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    underline: bool,
    code: bool,
    superscript: bool,
    subscript: bool,
    link: Option<Arc<str>>,
}

#[derive(Clone)]
struct TextRun {
    text: String,
    style: InlineStyle,
}

#[derive(Clone)]
struct HeadingOutline {
    title: String,
    level: u8,
    page: usize,
}

#[derive(Clone)]
struct ParagraphOptions {
    font_size: f32,
    indent: f32,
    space_before: f32,
    space_after: f32,
    preserve_whitespace: bool,
    quote_depth: usize,
    alignment: TextAlignment,
    color: PdfColor,
    heading: Option<(u8, String)>,
}

#[derive(Clone, Copy)]
enum TextAlignment {
    Left,
    Center,
}

struct PdfParagraph {
    runs: Vec<TextRun>,
    options: ParagraphOptions,
}

struct PdfTable {
    rows: Vec<Vec<Vec<TextRun>>>,
    alignments: Vec<TableAlignment>,
    indent: f32,
}

#[derive(Clone, Copy)]
struct TableLayout {
    column_count: usize,
    left: f32,
    column_width: f32,
    font_size: f32,
    line_height: f32,
    padding: f32,
}

struct PdfImage {
    bytes: Vec<u8>,
    alt: String,
    indent: f32,
    link: Option<Arc<str>>,
}

enum InlinePart {
    Run(TextRun),
    Image(PdfImage),
    Anchor(String),
}

enum PdfElement {
    Paragraph(PdfParagraph),
    CodeBlock {
        code: String,
        info: Option<String>,
        indent: f32,
    },
    Table(PdfTable),
    Image(PdfImage),
    HorizontalRule {
        indent: f32,
    },
    Anchor(String),
}

struct PdfWriter<'a> {
    page: Option<PdfPage<'a>>,
    document: PdfDocument<'a>,
    page_width: f32,
    page_height: f32,
    page_index: usize,
    cursor_y: f32,
    body_font: PdfFontToken,
    code_font: PdfFontToken,
    headings: Vec<HeadingOutline>,
    anchors: HashMap<String, usize>,
    heading_slugs: HashMap<String, usize>,
    has_internal_links: bool,
}

pub(crate) fn convert_markdown_to_pdf(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let base_dir = input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let document = crate::document::parse_markdown_document(&source, base_dir)?;
    write_document_to_pdf(&document, output)
}

pub(crate) fn write_document_to_pdf(
    document: &Document,
    output: &Path,
) -> Result<(), MarkoffError> {
    let mut elements = Vec::new();
    append_blocks(&document.blocks, 0.0, 0, &mut elements)?;

    let pdfium = crate::pdf::bundled_pdfium()?;
    let mut writer = PdfWriter::new(pdfium)?;
    for element in elements {
        writer.write_element(element)?;
    }
    writer.finish(output)
}

fn append_blocks(
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

fn append_inline_content(
    content: &[Inline],
    fallback: Option<&str>,
    initial_style: InlineStyle,
    options: ParagraphOptions,
    output: &mut Vec<PdfElement>,
) -> Result<(), MarkoffError> {
    append_inline_content_with_prefix(
        content,
        fallback,
        Vec::new(),
        initial_style,
        options,
        output,
    )
}

fn append_inline_content_with_prefix(
    content: &[Inline],
    fallback: Option<&str>,
    prefix: Vec<TextRun>,
    initial_style: InlineStyle,
    options: ParagraphOptions,
    output: &mut Vec<PdfElement>,
) -> Result<(), MarkoffError> {
    let mut parts = Vec::new();
    if content.is_empty() {
        if let Some(text) = fallback {
            push_text_part(
                &mut parts,
                TextRun {
                    text: text.to_string(),
                    style: initial_style,
                },
            );
        }
    } else {
        collect_inline_parts(content, &initial_style, &mut parts)?;
    }
    append_parts(parts, prefix, options, output);
    Ok(())
}

fn collect_inline_parts(
    inlines: &[Inline],
    style: &InlineStyle,
    output: &mut Vec<InlinePart>,
) -> Result<(), MarkoffError> {
    for inline in inlines {
        match inline {
            Inline::Text { text } => push_text_part(
                output,
                TextRun {
                    text: text.clone(),
                    style: style.clone(),
                },
            ),
            Inline::Emphasis { content } => {
                let mut nested = style.clone();
                nested.italic = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Strong { content } => {
                let mut nested = style.clone();
                nested.bold = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Strikethrough { content } => {
                let mut nested = style.clone();
                nested.strikethrough = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Underline { content } => {
                let mut nested = style.clone();
                nested.underline = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Superscript { content } => {
                let mut nested = style.clone();
                nested.superscript = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Subscript { content } => {
                let mut nested = style.clone();
                nested.subscript = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Code { text } => {
                let mut nested = style.clone();
                nested.code = true;
                push_text_part(
                    output,
                    TextRun {
                        text: text.clone(),
                        style: nested,
                    },
                );
            }
            Inline::Math { text, .. } => {
                let mut nested = style.clone();
                nested.italic = true;
                push_text_part(
                    output,
                    TextRun {
                        text: text.clone(),
                        style: nested,
                    },
                );
            }
            Inline::Link {
                destination,
                content,
                ..
            } => {
                let mut nested = style.clone();
                let is_internal = destination.starts_with('#');
                let is_safe_external = is_safe_external_uri(destination);
                nested.link =
                    (is_internal || is_safe_external).then(|| Arc::from(destination.as_str()));
                nested.underline = nested.link.is_some();
                collect_inline_parts(content, &nested, output)?;
                if !destination.is_empty() && !is_internal && !is_safe_external {
                    push_text_part(
                        output,
                        TextRun {
                            text: format!(" ({destination})"),
                            style: style.clone(),
                        },
                    );
                }
            }
            Inline::Image {
                alt,
                destination,
                data,
                ..
            } => {
                let bytes = data
                    .as_ref()
                    .map(|data| {
                        base64::engine::general_purpose::STANDARD
                            .decode(data)
                            .map_err(invalid_data)
                    })
                    .transpose()?;
                if let Some(bytes) = bytes {
                    output.push(InlinePart::Image(PdfImage {
                        bytes,
                        alt: alt.clone(),
                        indent: 0.0,
                        link: style.link.clone(),
                    }));
                } else {
                    let label = if alt.is_empty() {
                        destination.as_str()
                    } else {
                        alt.as_str()
                    };
                    push_text_part(
                        output,
                        TextRun {
                            text: format!("[image: {label}]"),
                            style: style.clone(),
                        },
                    );
                }
            }
            Inline::FootnoteReference { label } => {
                let mut nested = style.clone();
                nested.superscript = true;
                nested.link = Some(Arc::from(format!("#fn-{label}")));
                push_text_part(
                    output,
                    TextRun {
                        text: format!("[{label}]"),
                        style: nested,
                    },
                );
            }
            Inline::Footnote { content } => {
                let mut nested = style.clone();
                nested.superscript = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Bookmark { name } => output.push(InlinePart::Anchor(name.clone())),
            Inline::SoftBreak => push_text_part(
                output,
                TextRun {
                    text: " ".to_string(),
                    style: style.clone(),
                },
            ),
            Inline::HardBreak => push_text_part(
                output,
                TextRun {
                    text: "\n".to_string(),
                    style: style.clone(),
                },
            ),
            Inline::TaskListMarker { checked } => push_text_part(
                output,
                TextRun {
                    text: if *checked {
                        "[x] ".to_string()
                    } else {
                        "[ ] ".to_string()
                    },
                    style: style.clone(),
                },
            ),
            Inline::Html { html } => {
                let text = strip_html_tags(html);
                if !text.is_empty() {
                    push_text_part(
                        output,
                        TextRun {
                            text,
                            style: style.clone(),
                        },
                    );
                }
            }
        }
    }
    Ok(())
}

fn is_safe_external_uri(destination: &str) -> bool {
    destination
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .is_some_and(|scheme| matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel"))
}

fn push_text_part(output: &mut Vec<InlinePart>, run: TextRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(InlinePart::Run(previous)) = output.last_mut()
        && previous.style == run.style
    {
        previous.text.push_str(&run.text);
    } else {
        output.push(InlinePart::Run(run));
    }
}

fn push_run(output: &mut Vec<TextRun>, run: TextRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(previous) = output.last_mut()
        && previous.style == run.style
    {
        previous.text.push_str(&run.text);
    } else {
        output.push(run);
    }
}

fn append_parts(
    parts: Vec<InlinePart>,
    prefix: Vec<TextRun>,
    options: ParagraphOptions,
    output: &mut Vec<PdfElement>,
) {
    let mut runs = prefix;
    let mut has_layout_content = false;
    let mut has_paragraph = false;
    for part in parts {
        match part {
            InlinePart::Run(run) => push_run(&mut runs, run),
            InlinePart::Image(mut image) => {
                if flush_paragraph(
                    &mut runs,
                    &options,
                    has_layout_content,
                    has_paragraph,
                    false,
                    output,
                ) {
                    has_paragraph = true;
                }
                has_layout_content = true;
                image.indent += options.indent;
                output.push(PdfElement::Image(image));
            }
            InlinePart::Anchor(name) => {
                if flush_paragraph(
                    &mut runs,
                    &options,
                    has_layout_content,
                    has_paragraph,
                    false,
                    output,
                ) {
                    has_paragraph = true;
                }
                has_layout_content = true;
                output.push(PdfElement::Anchor(name));
            }
        }
    }
    flush_paragraph(
        &mut runs,
        &options,
        has_layout_content,
        has_paragraph,
        true,
        output,
    );
}

fn flush_paragraph(
    runs: &mut Vec<TextRun>,
    options: &ParagraphOptions,
    has_content: bool,
    has_paragraph: bool,
    last: bool,
    output: &mut Vec<PdfElement>,
) -> bool {
    if runs.is_empty() {
        return false;
    }
    let mut options = options.clone();
    if has_content {
        options.space_before = 0.0;
    }
    if has_paragraph {
        options.heading = None;
    }
    if !last {
        options.space_after = 0.0;
    }
    output.push(PdfElement::Paragraph(PdfParagraph {
        runs: std::mem::take(runs),
        options,
    }));
    true
}

fn inline_runs_without_images(inlines: &[Inline]) -> Result<Vec<TextRun>, MarkoffError> {
    let mut parts = Vec::new();
    collect_inline_parts(inlines, &InlineStyle::default(), &mut parts)?;
    let mut runs: Vec<TextRun> = Vec::new();
    for part in parts {
        match part {
            InlinePart::Run(run) => {
                if let Some(previous) = runs.last_mut()
                    && previous.style == run.style
                {
                    previous.text.push_str(&run.text);
                } else {
                    runs.push(run);
                }
            }
            InlinePart::Image(image) => {
                let label = if image.alt.is_empty() {
                    "[image]".to_string()
                } else {
                    format!("[image: {}]", image.alt)
                };
                runs.push(TextRun {
                    text: label,
                    style: InlineStyle::default(),
                });
            }
            InlinePart::Anchor(_) => {}
        }
    }
    Ok(runs)
}

fn plain_run(text: &str) -> TextRun {
    TextRun {
        text: text.to_string(),
        style: InlineStyle::default(),
    }
}

fn inline_plain_text(inlines: &[Inline], fallback: Option<&str>) -> Result<String, MarkoffError> {
    if inlines.is_empty() {
        Ok(fallback.unwrap_or_default().to_string())
    } else {
        Ok(inline_runs_without_images(inlines)?
            .into_iter()
            .map(|run| run.text)
            .collect())
    }
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

fn strip_html_tags(html: &str) -> String {
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

fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '_' {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            separator = false;
        } else if character == '-' {
            if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
            separator = false;
        } else {
            separator = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn normalize_anchor(anchor: &str) -> String {
    let anchor = anchor.strip_prefix('#').unwrap_or(anchor);
    slugify(anchor)
}

fn estimate_character_width(character: char, style: &InlineStyle, font_size: f32) -> f32 {
    if style.code {
        return font_size * 0.61;
    }
    let factor = if character.is_whitespace() {
        0.31
    } else if character.is_uppercase() {
        0.64
    } else if character.is_ascii_punctuation() {
        0.42
    } else {
        0.54
    };
    font_size * factor
}

fn estimate_run_width(run: &TextRun, font_size: f32) -> f32 {
    let size = if run.style.superscript || run.style.subscript {
        font_size * 0.72
    } else {
        font_size
    };
    run.text
        .chars()
        .map(|character| estimate_character_width(character, &run.style, size))
        .sum()
}

#[derive(Clone)]
struct StyledCharacter {
    character: char,
    style: InlineStyle,
}

fn wrap_runs(runs: &[TextRun], limit: usize, preserve_whitespace: bool) -> Vec<Vec<TextRun>> {
    let limit = limit.max(1);
    let mut lines = Vec::<Vec<StyledCharacter>>::new();
    let mut line = Vec::<StyledCharacter>::new();
    let mut word = Vec::<StyledCharacter>::new();
    let mut pending_space: Option<StyledCharacter> = None;

    for run in runs {
        for character in run.text.chars() {
            let styled = StyledCharacter {
                character,
                style: run.style.clone(),
            };
            if preserve_whitespace {
                if character == '\n' {
                    lines.push(std::mem::take(&mut line));
                } else {
                    if line.len() >= limit {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.push(styled);
                }
            } else if character == '\n' {
                place_word(&mut line, &mut lines, &mut word, &mut pending_space, limit);
                lines.push(std::mem::take(&mut line));
                pending_space = None;
            } else if character.is_whitespace() {
                place_word(&mut line, &mut lines, &mut word, &mut pending_space, limit);
                if !line.is_empty() {
                    pending_space = Some(styled);
                }
            } else {
                word.push(styled);
            }
        }
    }
    place_word(&mut line, &mut lines, &mut word, &mut pending_space, limit);
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines.into_iter().map(characters_to_runs).collect()
}

fn place_word(
    line: &mut Vec<StyledCharacter>,
    lines: &mut Vec<Vec<StyledCharacter>>,
    word: &mut Vec<StyledCharacter>,
    pending_space: &mut Option<StyledCharacter>,
    limit: usize,
) {
    if word.is_empty() {
        return;
    }
    let space_count = usize::from(pending_space.is_some() && !line.is_empty());
    if line.len() + space_count + word.len() > limit && !line.is_empty() {
        if let Some(space_index) = line.iter().rposition(|item| item.character.is_whitespace()) {
            let remaining = line.split_off(space_index + 1);
            while line
                .last()
                .is_some_and(|item| item.character.is_whitespace())
            {
                line.pop();
            }
            lines.push(std::mem::take(line));
            *line = remaining;
        } else {
            lines.push(std::mem::take(line));
        }
        *pending_space = None;
    }
    if let Some(space) = pending_space.take()
        && !line.is_empty()
        && line.len() < limit
    {
        line.push(space);
    }

    for character in word.drain(..) {
        if line.len() >= limit {
            lines.push(std::mem::take(line));
        }
        line.push(character);
    }
}

fn characters_to_runs(characters: Vec<StyledCharacter>) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = Vec::new();
    for item in characters {
        if let Some(last) = runs.last_mut()
            && last.style == item.style
        {
            last.text.push(item.character);
        } else {
            runs.push(TextRun {
                text: item.character.to_string(),
                style: item.style,
            });
        }
    }
    runs
}

impl<'a> PdfWriter<'a> {
    fn new(pdfium: &'a Pdfium) -> Result<Self, MarkoffError> {
        let mut document = pdfium.create_new_pdf().map_err(invalid_data)?;
        let body_font = document
            .fonts_mut()
            .load_true_type_from_bytes(UBUNTU_LIGHT, true)
            .map_err(invalid_data)?;
        let code_font = document
            .fonts_mut()
            .load_true_type_from_bytes(HACK_REGULAR, true)
            .map_err(invalid_data)?;
        let mut page = document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::a4())
            .map_err(invalid_data)?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        let page_width = page.width().value;
        let page_height = page.height().value;

        Ok(Self {
            page: Some(page),
            document,
            page_width,
            page_height,
            page_index: 0,
            cursor_y: page_height - PDF_MARGIN_TOP,
            body_font,
            code_font,
            headings: Vec::new(),
            anchors: HashMap::new(),
            heading_slugs: HashMap::new(),
            has_internal_links: false,
        })
    }

    fn page_mut(&mut self) -> &mut PdfPage<'a> {
        self.page
            .as_mut()
            .expect("PDF writer always has an active page")
    }

    fn commit_page(&mut self) -> Result<(), MarkoffError> {
        if let Some(mut page) = self.page.take() {
            page.regenerate_content().map_err(invalid_data)?;
        }
        Ok(())
    }

    fn next_page(&mut self) -> Result<(), MarkoffError> {
        self.commit_page()?;
        let mut page = self
            .document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::a4())
            .map_err(invalid_data)?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        self.page_width = page.width().value;
        self.page_height = page.height().value;
        self.cursor_y = self.page_height - PDF_MARGIN_TOP;
        self.page_index += 1;
        self.page = Some(page);
        Ok(())
    }

    fn ensure_space(&mut self, height: f32) -> Result<(), MarkoffError> {
        if self.cursor_y - height < PDF_MARGIN_BOTTOM {
            self.next_page()?;
        }
        Ok(())
    }

    fn write_element(&mut self, element: PdfElement) -> Result<(), MarkoffError> {
        match element {
            PdfElement::Paragraph(paragraph) => self.write_paragraph(paragraph),
            PdfElement::CodeBlock { code, info, indent } => {
                self.write_code_block(&code, info.as_deref(), indent)
            }
            PdfElement::Table(table) => self.write_table(table),
            PdfElement::Image(image) => self.write_image(image),
            PdfElement::HorizontalRule { indent } => {
                self.ensure_space(16.0)?;
                let left = PDF_MARGIN_LEFT + indent;
                let right = self.page_width - PDF_MARGIN_RIGHT;
                let y = self.cursor_y - 5.0;
                self.draw_line(left, y, right, y, TABLE_BORDER, PdfPoints::new(0.7))?;
                self.cursor_y -= 14.0;
                Ok(())
            }
            PdfElement::Anchor(name) => {
                self.anchors
                    .insert(normalize_anchor(&name), self.page_index);
                Ok(())
            }
        }
    }

    fn write_paragraph(&mut self, paragraph: PdfParagraph) -> Result<(), MarkoffError> {
        self.cursor_y -= paragraph.options.space_before;
        let available_width =
            self.page_width - PDF_MARGIN_LEFT - PDF_MARGIN_RIGHT - paragraph.options.indent;
        let max_characters = (available_width / (paragraph.options.font_size * 0.72))
            .floor()
            .max(1.0) as usize;
        let lines = wrap_runs(
            &paragraph.runs,
            max_characters,
            paragraph.options.preserve_whitespace,
        );
        let line_height = paragraph.options.font_size * 1.35;

        for (line_index, line) in lines.iter().enumerate() {
            self.ensure_space(line_height)?;
            if line_index == 0
                && let Some((level, title)) = paragraph.options.heading.as_ref()
            {
                self.record_heading(*level, title);
            }

            let default_x = PDF_MARGIN_LEFT + paragraph.options.indent;
            let line_width = line
                .iter()
                .map(|run| estimate_run_width(run, paragraph.options.font_size))
                .sum::<f32>();
            let x = match paragraph.options.alignment {
                TextAlignment::Left => default_x,
                TextAlignment::Center => {
                    default_x + ((available_width - line_width) / 2.0).max(0.0)
                }
            };

            if paragraph.options.quote_depth > 0 {
                let quote_x = (default_x - 7.0).max(PDF_MARGIN_LEFT - 10.0);
                self.draw_line(
                    quote_x,
                    self.cursor_y - paragraph.options.font_size * 0.2,
                    quote_x,
                    self.cursor_y + paragraph.options.font_size * 0.8,
                    PdfColor::GREY_70,
                    PdfPoints::new(1.5),
                )?;
            }
            self.draw_runs(
                line,
                x,
                self.cursor_y,
                paragraph.options.font_size,
                paragraph.options.color,
                false,
            )?;
            self.cursor_y -= line_height;
        }
        self.cursor_y -= paragraph.options.space_after;
        Ok(())
    }

    fn record_heading(&mut self, level: u8, title: &str) {
        if title.trim().is_empty() {
            return;
        }
        let base_slug = slugify(title);
        if !base_slug.is_empty() {
            let count = self.heading_slugs.entry(base_slug.clone()).or_default();
            let slug = if *count == 0 {
                base_slug
            } else {
                format!("{base_slug}-{}", *count)
            };
            *count += 1;
            self.anchors
                .insert(normalize_anchor(&slug), self.page_index);
        }
        self.headings.push(HeadingOutline {
            title: title.to_string(),
            level,
            page: self.page_index,
        });
    }

    fn draw_runs(
        &mut self,
        runs: &[TextRun],
        start_x: f32,
        baseline: f32,
        base_size: f32,
        base_color: PdfColor,
        code_block: bool,
    ) -> Result<(), MarkoffError> {
        let mut x = start_x;
        for run in runs {
            if run.text.is_empty() {
                continue;
            }
            let font_size = if run.style.superscript || run.style.subscript {
                base_size * 0.72
            } else {
                base_size
            };
            let y = if run.style.superscript {
                baseline + base_size * 0.32
            } else if run.style.subscript {
                baseline - base_size * 0.2
            } else {
                baseline
            };
            let color = if run.style.link.is_some() {
                LINK_COLOR
            } else if run.style.code {
                PdfColor::GREY_20
            } else {
                base_color
            };
            let width = estimate_run_width(run, base_size);

            if run.style.code && !code_block {
                self.draw_rectangle(
                    PdfRect::new_from_values(
                        y - font_size * 0.22,
                        x - 2.0,
                        y + font_size * 0.84,
                        x + width + 2.0,
                    ),
                    Some((TABLE_BORDER, PdfPoints::new(0.35))),
                    Some(CODE_BACKGROUND),
                )?;
            }

            let font = if run.style.code {
                self.code_font
            } else {
                self.body_font
            };
            let mut object = self
                .page_mut()
                .objects_mut()
                .create_text_object(
                    PdfPoints::new(x),
                    PdfPoints::new(y),
                    &run.text,
                    font,
                    PdfPoints::new(font_size),
                )
                .map_err(invalid_data)?;
            let text = object
                .as_text_object_mut()
                .ok_or_else(|| invalid_data(std::io::Error::other("expected a PDF text object")))?;
            text.set_fill_color(color).map_err(invalid_data)?;
            if run.style.bold {
                text.set_render_mode(PdfPageTextRenderMode::FilledThenStroked)
                    .map_err(invalid_data)?;
                text.set_stroke_color(color).map_err(invalid_data)?;
                text.set_stroke_width(PdfPoints::new(0.18))
                    .map_err(invalid_data)?;
            }
            if run.style.italic {
                text.skew_degrees(0.0, 12.0).map_err(invalid_data)?;
            }
            drop(object);

            if run.style.underline || run.style.link.is_some() {
                self.draw_line(x, y - 1.5, x + width, y - 1.5, color, PdfPoints::new(0.45))?;
            }
            if run.style.strikethrough {
                self.draw_line(
                    x,
                    y + font_size * 0.31,
                    x + width,
                    y + font_size * 0.31,
                    color,
                    PdfPoints::new(0.45),
                )?;
            }
            if let Some(destination) = run.style.link.as_deref() {
                self.add_link_annotation(destination, x, y, width, font_size)?;
            }
            x += width;
        }
        Ok(())
    }

    fn write_code_block(
        &mut self,
        code: &str,
        info: Option<&str>,
        indent: f32,
    ) -> Result<(), MarkoffError> {
        if let Some(info) = info.filter(|info| !info.is_empty()) {
            let label = PdfParagraph {
                runs: vec![TextRun {
                    text: format!("[{info}]"),
                    style: InlineStyle::default(),
                }],
                options: ParagraphOptions {
                    font_size: 8.5,
                    indent,
                    space_before: 0.0,
                    space_after: 2.0,
                    preserve_whitespace: false,
                    quote_depth: 0,
                    alignment: TextAlignment::Left,
                    color: PdfColor::GREY_50,
                    heading: None,
                },
            };
            self.write_paragraph(label)?;
        }

        let code = code.replace('\t', "    ");
        let size = 9.5;
        let line_height = size * 1.4;
        let left = PDF_MARGIN_LEFT + indent;
        let width = (self.page_width - PDF_MARGIN_RIGHT - left).max(1.0);
        let max_characters = (width / (size * 0.65)).floor().max(1.0) as usize;
        let source_lines = if code.is_empty() {
            vec![String::new()]
        } else {
            code.split('\n').map(str::to_string).collect()
        };
        let mut code_lines = Vec::new();
        for source_line in source_lines {
            let run = TextRun {
                text: source_line,
                style: InlineStyle {
                    code: true,
                    ..InlineStyle::default()
                },
            };
            code_lines.extend(wrap_runs(&[run], max_characters, true));
        }

        self.cursor_y -= 2.0;
        for line in code_lines {
            self.ensure_space(line_height + 2.0)?;
            self.draw_rectangle(
                PdfRect::new_from_values(
                    self.cursor_y - line_height + 2.0,
                    left,
                    self.cursor_y + 3.0,
                    left + width,
                ),
                None,
                Some(CODE_BACKGROUND),
            )?;
            self.draw_runs(
                &line,
                left + 7.0,
                self.cursor_y,
                size,
                PdfColor::GREY_20,
                true,
            )?;
            self.cursor_y -= line_height;
        }
        self.cursor_y -= 7.0;
        Ok(())
    }

    fn write_table(&mut self, table: PdfTable) -> Result<(), MarkoffError> {
        let column_count = table.rows.iter().map(Vec::len).max().unwrap_or_default();
        if column_count == 0 {
            return Ok(());
        }
        let left = PDF_MARGIN_LEFT + table.indent;
        let available_width = (self.page_width - PDF_MARGIN_RIGHT - left).max(1.0);
        let column_width = available_width / column_count as f32;
        let font_size = 9.5;
        let line_height = font_size * 1.3;
        let padding = 4.0;
        let layout = TableLayout {
            column_count,
            left,
            column_width,
            font_size,
            line_height,
            padding,
        };
        let max_lines_per_page =
            ((self.page_height - PDF_MARGIN_TOP - PDF_MARGIN_BOTTOM - padding * 2.0) / line_height)
                .floor()
                .max(1.0) as usize;
        let max_lines_per_segment = if table.rows.len() > 1 {
            max_lines_per_page.saturating_sub(1).max(1)
        } else {
            max_lines_per_page
        };

        for (row_index, row) in table.rows.iter().enumerate() {
            let mut cells = row.clone();
            cells.resize_with(column_count, Vec::new);
            if row_index == 0 {
                for cell in &mut cells {
                    for run in cell {
                        run.style.bold = true;
                    }
                }
            }
            let wrapped = cells
                .iter()
                .map(|cell| {
                    let max_characters = ((column_width - padding * 2.0) / (font_size * 0.68))
                        .floor()
                        .max(1.0) as usize;
                    let lines = wrap_runs(cell, max_characters, false);
                    if lines.is_empty() {
                        vec![Vec::new()]
                    } else {
                        lines
                    }
                })
                .collect::<Vec<_>>();
            let row_line_count = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let mut line_start = 0;

            while line_start < row_line_count {
                let line_count = (row_line_count - line_start).min(max_lines_per_segment);
                let row_height = line_count as f32 * line_height + padding * 2.0;
                if self.cursor_y - row_height < PDF_MARGIN_BOTTOM {
                    self.next_page()?;
                    if row_index > 0 {
                        let header = table.rows.first().cloned().unwrap_or_default();
                        if !header.is_empty() {
                            self.write_repeated_table_header(&header, layout)?;
                        }
                    }
                }
                self.draw_table_segment(
                    &wrapped,
                    line_start,
                    line_count,
                    table.alignments.as_slice(),
                    row_index == 0,
                    layout,
                )?;
                self.cursor_y -= row_height;
                line_start += line_count;
            }
        }
        self.cursor_y -= 8.0;
        Ok(())
    }

    fn write_repeated_table_header(
        &mut self,
        header: &[Vec<TextRun>],
        layout: TableLayout,
    ) -> Result<(), MarkoffError> {
        let mut cells = header.to_vec();
        cells.resize_with(layout.column_count, Vec::new);
        for cell in &mut cells {
            for run in cell {
                run.style.bold = true;
            }
        }
        let wrapped = cells
            .iter()
            .map(|cell| {
                let limit = ((layout.column_width - layout.padding * 2.0)
                    / (layout.font_size * 0.68))
                    .floor()
                    .max(1.0) as usize;
                wrap_runs(cell, limit, false)
                    .into_iter()
                    .next()
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let height = layout.line_height + layout.padding * 2.0;
        self.ensure_space(height)?;
        self.draw_table_segment(
            &wrapped
                .iter()
                .map(|line| vec![line.clone()])
                .collect::<Vec<_>>(),
            0,
            1,
            &[],
            true,
            layout,
        )?;
        self.cursor_y -= height;
        Ok(())
    }

    fn draw_table_segment(
        &mut self,
        cells: &[Vec<Vec<TextRun>>],
        line_start: usize,
        line_count: usize,
        alignments: &[TableAlignment],
        header: bool,
        layout: TableLayout,
    ) -> Result<(), MarkoffError> {
        let row_height = line_count as f32 * layout.line_height + layout.padding * 2.0;
        let top = self.cursor_y;
        let bottom = top - row_height;
        let fill = if header {
            TABLE_HEADER_BACKGROUND
        } else {
            PdfColor::WHITE
        };

        for column in 0..layout.column_count {
            let cell_left = layout.left + column as f32 * layout.column_width;
            let cell_right = if column + 1 == layout.column_count {
                self.page_width - PDF_MARGIN_RIGHT
            } else {
                cell_left + layout.column_width
            };
            self.draw_rectangle(
                PdfRect::new_from_values(bottom, cell_left, top, cell_right),
                Some((TABLE_BORDER, PdfPoints::new(0.45))),
                Some(fill),
            )?;

            let lines = cells.get(column).map(Vec::as_slice).unwrap_or_default();
            for local_line in 0..line_count {
                let Some(line) = lines.get(line_start + local_line) else {
                    continue;
                };
                let width = line
                    .iter()
                    .map(|run| estimate_run_width(run, layout.font_size))
                    .sum::<f32>();
                let content_width = (cell_right - cell_left - layout.padding * 2.0).max(1.0);
                let alignment = alignments
                    .get(column)
                    .copied()
                    .unwrap_or(TableAlignment::None);
                let offset = match alignment {
                    TableAlignment::None | TableAlignment::Left => 0.0,
                    TableAlignment::Center => ((content_width - width) / 2.0).max(0.0),
                    TableAlignment::Right => (content_width - width).max(0.0),
                };
                let baseline = top
                    - layout.padding
                    - layout.font_size
                    - local_line as f32 * layout.line_height;
                self.draw_runs(
                    line,
                    cell_left + layout.padding + offset,
                    baseline,
                    layout.font_size,
                    PdfColor::BLACK,
                    false,
                )?;
            }
        }
        Ok(())
    }

    fn write_image(&mut self, image: PdfImage) -> Result<(), MarkoffError> {
        let dynamic_image = image::load_from_memory(&image.bytes).map_err(invalid_data)?;
        let source_width = dynamic_image.width() as f32;
        let source_height = dynamic_image.height() as f32;
        if source_width <= 0.0 || source_height <= 0.0 {
            return Err(invalid_data(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "image has no drawable area",
            ))
            .into());
        }
        let left = PDF_MARGIN_LEFT + image.indent;
        let available_width = (self.page_width - PDF_MARGIN_RIGHT - left).max(1.0);
        let available_height =
            (self.page_height - PDF_MARGIN_TOP - PDF_MARGIN_BOTTOM - 35.0).max(1.0);
        let scale = (available_width / source_width)
            .min(available_height / source_height)
            .min(1.0);
        let width = source_width * scale;
        let height = source_height * scale;
        self.ensure_space(height + 8.0)?;
        let bottom = self.cursor_y - height;
        self.page_mut()
            .objects_mut()
            .create_image_object(
                PdfPoints::new(left),
                PdfPoints::new(bottom),
                &dynamic_image,
                Some(PdfPoints::new(width)),
                Some(PdfPoints::new(height)),
            )
            .map_err(invalid_data)?;
        if let Some(link) = image.link.as_deref() {
            self.add_link_annotation_bounds(
                link,
                PdfRect::new_from_values(bottom, left, bottom + height, left + width),
            )?;
        }
        self.cursor_y = bottom - 5.0;
        if !image.alt.is_empty() {
            let caption = PdfParagraph {
                runs: vec![TextRun {
                    text: image.alt,
                    style: InlineStyle {
                        italic: true,
                        ..InlineStyle::default()
                    },
                }],
                options: ParagraphOptions {
                    font_size: 9.0,
                    indent: image.indent,
                    space_before: 0.0,
                    space_after: 6.0,
                    preserve_whitespace: false,
                    quote_depth: 0,
                    alignment: TextAlignment::Center,
                    color: PdfColor::GREY_30,
                    heading: None,
                },
            };
            self.write_paragraph(caption)?;
        }
        Ok(())
    }

    fn add_link_annotation(
        &mut self,
        destination: &str,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Result<(), MarkoffError> {
        if width <= 0.0 || height <= 0.0 {
            return Ok(());
        }
        self.add_link_annotation_bounds(
            destination,
            PdfRect::new_from_values(y - height * 0.25, x, y + height * 0.8, x + width),
        )
    }

    fn add_link_annotation_bounds(
        &mut self,
        destination: &str,
        bounds: PdfRect,
    ) -> Result<(), MarkoffError> {
        if bounds.width().value <= 0.0 || bounds.height().value <= 0.0 {
            return Ok(());
        }
        let uri = if let Some(anchor) = destination.strip_prefix('#') {
            self.has_internal_links = true;
            let encoded =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(anchor.as_bytes());
            format!("{INTERNAL_LINK_PREFIX}{encoded}")
        } else {
            destination.to_string()
        };
        let mut annotation = self
            .page_mut()
            .annotations_mut()
            .create_link_annotation(&uri)
            .map_err(invalid_data)?;
        annotation.set_bounds(bounds).map_err(invalid_data)?;
        Ok(())
    }

    fn draw_rectangle(
        &mut self,
        bounds: PdfRect,
        stroke: Option<(PdfColor, PdfPoints)>,
        fill: Option<PdfColor>,
    ) -> Result<(), MarkoffError> {
        let (stroke_color, stroke_width) = stroke
            .map(|(color, width)| (Some(color), Some(width)))
            .unwrap_or((None, None));
        self.page_mut()
            .objects_mut()
            .create_path_object_rect(bounds, stroke_color, stroke_width, fill)
            .map_err(invalid_data)?;
        Ok(())
    }

    fn draw_line(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: PdfColor,
        width: PdfPoints,
    ) -> Result<(), MarkoffError> {
        self.page_mut()
            .objects_mut()
            .create_path_object_line(
                PdfPoints::new(x1),
                PdfPoints::new(y1),
                PdfPoints::new(x2),
                PdfPoints::new(y2),
                color,
                width,
            )
            .map_err(invalid_data)?;
        Ok(())
    }

    fn finish(mut self, output: &Path) -> Result<(), MarkoffError> {
        self.commit_page()?;
        self.document.save_to_file(output).map_err(invalid_data)?;
        if !self.headings.is_empty() || self.has_internal_links {
            add_pdf_navigation(output, &self.headings, &self.anchors)?;
        }
        Ok(())
    }
}

fn add_pdf_navigation(
    path: &Path,
    headings: &[HeadingOutline],
    anchors: &HashMap<String, usize>,
) -> Result<(), MarkoffError> {
    let mut document = lopdf::Document::load(path).map_err(invalid_data)?;
    let pages = document.get_pages();
    let mut heading_stack = Vec::<(u8, u32)>::new();

    for heading in headings {
        let page_number = u32::try_from(heading.page + 1).map_err(invalid_data)?;
        let page = pages
            .get(&page_number)
            .copied()
            .ok_or_else(|| invalid_data(std::io::Error::other("PDF heading page is missing")))?;
        while heading_stack
            .last()
            .is_some_and(|(level, _)| *level >= heading.level)
        {
            heading_stack.pop();
        }
        let parent = heading_stack.last().map(|(_, bookmark)| *bookmark);
        let format = if heading.level == 1 { 2 } else { 0 };
        let bookmark = document.add_bookmark(
            lopdf::Bookmark::new(heading.title.clone(), [0.1, 0.2, 0.35], format, page),
            parent,
        );
        heading_stack.push((heading.level, bookmark));
    }

    if let Some(outline) = document.build_outline() {
        let catalog = document.catalog_mut().map_err(invalid_data)?;
        catalog.set("Outlines", outline);
        catalog.set("PageMode", "UseOutlines");
    }

    let page_ids = pages.values().copied().collect::<Vec<_>>();
    for page_id in page_ids {
        let annotations = page_annotations(&document, page_id)?;
        for (annotation_index, annotation) in annotations.iter().enumerate() {
            let Some(uri) = annotation_uri(&document, annotation)? else {
                continue;
            };
            let Some(encoded_target) = uri.strip_prefix(INTERNAL_LINK_PREFIX) else {
                continue;
            };
            let target = String::from_utf8(
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(encoded_target)
                    .map_err(invalid_data)?,
            )
            .map_err(invalid_data)?;
            let target = normalize_anchor(&target);
            let action = if let Some(page_number) = anchors.get(&target) {
                let page_number = u32::try_from(*page_number + 1).map_err(invalid_data)?;
                let page = pages.get(&page_number).copied().ok_or_else(|| {
                    invalid_data(std::io::Error::other(
                        "PDF link destination page is missing",
                    ))
                })?;
                let mut action = lopdf::Dictionary::new();
                action.set("S", "GoTo");
                action.set(
                    "D",
                    vec![
                        lopdf::Object::Reference(page),
                        lopdf::Object::Name(b"Fit".to_vec()),
                    ],
                );
                lopdf::Object::Dictionary(action)
            } else {
                let mut action = lopdf::Dictionary::new();
                action.set("S", "URI");
                action.set("URI", format!("#{target}"));
                lopdf::Object::Dictionary(action)
            };
            set_annotation_action(&mut document, page_id, annotation_index, annotation, action)?;
        }
    }

    document.save(path).map_err(invalid_data)?;
    Ok(())
}

fn page_annotations(
    document: &lopdf::Document,
    page_id: lopdf::ObjectId,
) -> Result<Vec<lopdf::Object>, MarkoffError> {
    let page = document
        .get_object(page_id)
        .map_err(invalid_data)?
        .as_dict()
        .map_err(invalid_data)?;
    let Ok(annotations) = page.get(b"Annots") else {
        return Ok(Vec::new());
    };
    let Ok(annotations) = annotations.as_array() else {
        return Ok(Vec::new());
    };
    Ok(annotations.clone())
}

fn annotation_uri(
    document: &lopdf::Document,
    annotation: &lopdf::Object,
) -> Result<Option<String>, MarkoffError> {
    let annotation = if let Ok(annotation_id) = annotation.as_reference() {
        document.get_object(annotation_id).map_err(invalid_data)?
    } else {
        annotation
    };
    let Ok(annotation) = annotation.as_dict() else {
        return Ok(None);
    };
    let Some(action) = annotation.get(b"A").ok() else {
        return Ok(None);
    };
    let action = if let Ok(action_id) = action.as_reference() {
        document.get_object(action_id).map_err(invalid_data)?
    } else {
        action
    };
    let Ok(action) = action.as_dict() else {
        return Ok(None);
    };
    let Some(uri) = action.get(b"URI").ok() else {
        return Ok(None);
    };
    let Ok(uri) = uri.as_str() else {
        return Ok(None);
    };
    let uri = String::from_utf8(uri.to_vec()).map_err(invalid_data)?;
    Ok(Some(uri))
}

fn set_annotation_action(
    document: &mut lopdf::Document,
    page_id: lopdf::ObjectId,
    annotation_index: usize,
    annotation: &lopdf::Object,
    action: lopdf::Object,
) -> Result<(), MarkoffError> {
    if let Ok(annotation_id) = annotation.as_reference() {
        document
            .get_object_mut(annotation_id)
            .map_err(invalid_data)?
            .as_dict_mut()
            .map_err(invalid_data)?
            .set("A", action);
    } else {
        let page = document
            .get_object_mut(page_id)
            .map_err(invalid_data)?
            .as_dict_mut()
            .map_err(invalid_data)?;
        let annotations = page
            .get_mut(b"Annots")
            .map_err(invalid_data)?
            .as_array_mut()
            .map_err(invalid_data)?;
        annotations
            .get_mut(annotation_index)
            .ok_or_else(|| {
                invalid_data(std::io::Error::other(
                    "PDF annotation disappeared during navigation update",
                ))
            })?
            .as_dict_mut()
            .map_err(invalid_data)?
            .set("A", action);
    }
    Ok(())
}
