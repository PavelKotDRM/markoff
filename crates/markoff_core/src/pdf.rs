use crate::MarkoffError;
use crate::document_model::{Block, Document, Inline};
use crate::error::invalid_data;
use epaint_default_fonts::UBUNTU_LIGHT;
use pdfium_bundled::pdfium_render::prelude::{
    PdfPageContentRegenerationStrategy, PdfPageObjectsCommon, PdfPagePaperSize, PdfPoints, Pdfium,
};
use std::path::Path;
use std::sync::OnceLock;

const PDF_MARGIN_LEFT: f32 = 42.0;
const PDF_MARGIN_RIGHT: f32 = 42.0;
const PDF_MARGIN_TOP: f32 = 42.0;
const PDF_MARGIN_BOTTOM: f32 = 42.0;

struct PdfParagraph {
    text: String,
    font_size: f32,
    indent: f32,
    space_before: f32,
    space_after: f32,
    preserve_whitespace: bool,
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
    let mut paragraphs = Vec::new();
    append_blocks(&document.blocks, 0.0, &mut paragraphs);

    let pdfium = bundled_pdfium()?;
    let mut pdf = pdfium.create_new_pdf().map_err(invalid_data)?;
    let font = pdf
        .fonts_mut()
        .load_true_type_from_bytes(UBUNTU_LIGHT, true)
        .map_err(invalid_data)?;
    let mut page = pdf
        .pages_mut()
        .create_page_at_end(PdfPagePaperSize::a4())
        .map_err(invalid_data)?;
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
    let mut page_width = page.width().value;
    let mut cursor_y = page.height().value - PDF_MARGIN_TOP;

    for paragraph in paragraphs {
        cursor_y -= paragraph.space_before;
        let available_width = page_width - PDF_MARGIN_LEFT - PDF_MARGIN_RIGHT - paragraph.indent;
        let max_characters = (available_width / (paragraph.font_size * 0.72))
            .floor()
            .max(1.0) as usize;

        for line in wrap_text(
            &paragraph.text,
            max_characters,
            paragraph.preserve_whitespace,
        ) {
            let line_height = paragraph.font_size * 1.35;
            if cursor_y - paragraph.font_size < PDF_MARGIN_BOTTOM {
                page.regenerate_content().map_err(invalid_data)?;
                drop(page);
                page = pdf
                    .pages_mut()
                    .create_page_at_end(PdfPagePaperSize::a4())
                    .map_err(invalid_data)?;
                page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
                page_width = page.width().value;
                cursor_y = page.height().value - PDF_MARGIN_TOP;
            }

            if line.is_empty() {
                cursor_y -= line_height;
                continue;
            }

            page.objects_mut()
                .create_text_object(
                    PdfPoints::new(PDF_MARGIN_LEFT + paragraph.indent),
                    PdfPoints::new(cursor_y),
                    line,
                    font,
                    PdfPoints::new(paragraph.font_size),
                )
                .map_err(invalid_data)?;
            cursor_y -= line_height;
        }
        cursor_y -= paragraph.space_after;
    }

    page.regenerate_content().map_err(invalid_data)?;
    drop(page);
    pdf.save_to_file(output).map_err(invalid_data)?;
    Ok(())
}

fn append_blocks(blocks: &[Block], indent: f32, output: &mut Vec<PdfParagraph>) {
    for block in blocks {
        match block {
            Block::Heading {
                level,
                text,
                content,
            } => push_paragraph(
                output,
                block_content_text(content, text.as_deref()),
                heading_font_size(*level),
                indent,
                10.0,
                6.0,
                false,
            ),
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
                    "- ".to_string()
                };
                push_paragraph(
                    output,
                    format!("{marker}{}", block_content_text(content, text.as_deref())),
                    11.0,
                    indent + *level as f32 * 18.0,
                    0.0,
                    2.0,
                    false,
                );
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
                        "-".to_string()
                    };
                    append_list_item(&item.blocks, &marker, indent, output);
                    next_number = number.saturating_add(1);
                }
                if !items.is_empty() {
                    add_spacing(output, 4.0);
                }
            }
            Block::Table { rows, cells, .. } => {
                if cells.is_empty() {
                    if let Some(rows) = rows {
                        for row in rows {
                            push_paragraph(output, row.join(" | "), 10.0, indent, 0.0, 2.0, false);
                        }
                    }
                } else {
                    for row in cells {
                        let cells = row.iter().map(|cell| inline_text(cell)).collect::<Vec<_>>();
                        push_paragraph(output, cells.join(" | "), 10.0, indent, 0.0, 2.0, false);
                    }
                }
                add_spacing(output, 4.0);
            }
            Block::Code { code, info } => {
                if let Some(info) = info.as_deref().filter(|info| !info.is_empty()) {
                    push_paragraph(output, format!("[{info}]"), 9.0, indent, 0.0, 2.0, false);
                }
                push_paragraph(output, code.clone(), 9.0, indent, 0.0, 6.0, true);
            }
            Block::Math { text } => {
                push_paragraph(output, text.clone(), 11.0, indent, 0.0, 6.0, false);
            }
            Block::Quote { text, blocks } => {
                if blocks.is_empty() {
                    push_paragraph(
                        output,
                        text.clone().unwrap_or_default(),
                        11.0,
                        indent + 14.0,
                        0.0,
                        6.0,
                        false,
                    );
                } else {
                    append_blocks(blocks, indent + 14.0, output);
                }
            }
            Block::HorizontalRule => push_paragraph(
                output,
                "------------------------------------------------".to_string(),
                9.0,
                indent,
                4.0,
                8.0,
                false,
            ),
            Block::Image { alt, .. } => push_image_placeholder(output, alt, indent),
            Block::Paragraph { text, content } => push_paragraph(
                output,
                block_content_text(content, text.as_deref()),
                11.0,
                indent,
                0.0,
                6.0,
                false,
            ),
            Block::FootnoteDefinition { label, blocks } => {
                push_paragraph(output, format!("[^{label}]"), 9.0, indent, 0.0, 2.0, false);
                append_blocks(blocks, indent + 14.0, output);
            }
            Block::Html { html } => {
                push_paragraph(output, strip_html_tags(html), 11.0, indent, 0.0, 6.0, false)
            }
        }
    }
}

fn append_list_item(blocks: &[Block], marker: &str, indent: f32, output: &mut Vec<PdfParagraph>) {
    let Some((first, rest)) = blocks.split_first() else {
        push_paragraph(output, marker.to_string(), 11.0, indent, 0.0, 2.0, false);
        return;
    };

    match first {
        Block::Paragraph { text, content } => push_paragraph(
            output,
            format!("{marker} {}", block_content_text(content, text.as_deref())),
            11.0,
            indent,
            0.0,
            2.0,
            false,
        ),
        _ => {
            push_paragraph(output, marker.to_string(), 11.0, indent, 0.0, 2.0, false);
            append_blocks(std::slice::from_ref(first), indent + 18.0, output);
        }
    }
    append_blocks(rest, indent + 18.0, output);
}

fn block_content_text(content: &[Inline], fallback: Option<&str>) -> String {
    if content.is_empty() {
        fallback.unwrap_or_default().to_string()
    } else {
        inline_text(content)
    }
}

fn inline_text(inlines: &[Inline]) -> String {
    let mut output = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text } | Inline::Code { text } => output.push_str(text),
            Inline::Emphasis { content }
            | Inline::Strong { content }
            | Inline::Strikethrough { content }
            | Inline::Underline { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Footnote { content } => output.push_str(&inline_text(content)),
            Inline::Math { text, .. } => output.push_str(text),
            Inline::Link {
                destination,
                content,
                ..
            } => {
                let label = inline_text(content);
                output.push_str(&label);
                if !destination.is_empty() && destination != &label {
                    output.push_str(" (");
                    output.push_str(destination);
                    output.push(')');
                }
            }
            Inline::Image { alt, .. } => append_image_label(&mut output, alt),
            Inline::FootnoteReference { label } => {
                output.push_str("[^");
                output.push_str(label);
                output.push(']');
            }
            Inline::Bookmark { .. } => {}
            Inline::SoftBreak => output.push(' '),
            Inline::HardBreak => output.push('\n'),
            Inline::TaskListMarker { checked } => {
                output.push_str(if *checked { "[x] " } else { "[ ] " });
            }
            Inline::Html { html } => output.push_str(&strip_html_tags(html)),
        }
    }
    output
}

fn push_image_placeholder(output: &mut Vec<PdfParagraph>, alt: &str, indent: f32) {
    let mut label = String::new();
    append_image_label(&mut label, alt);
    push_paragraph(output, label, 10.0, indent, 0.0, 6.0, false);
}

fn append_image_label(output: &mut String, alt: &str) {
    if alt.is_empty() {
        output.push_str("[image]");
    } else {
        output.push_str("[image: ");
        output.push_str(alt);
        output.push(']');
    }
}

fn push_paragraph(
    output: &mut Vec<PdfParagraph>,
    text: String,
    font_size: f32,
    indent: f32,
    space_before: f32,
    space_after: f32,
    preserve_whitespace: bool,
) {
    if text.trim().is_empty() {
        return;
    }
    output.push(PdfParagraph {
        text,
        font_size,
        indent,
        space_before,
        space_after,
        preserve_whitespace,
    });
}

fn add_spacing(output: &mut [PdfParagraph], points: f32) {
    if let Some(paragraph) = output.last_mut() {
        paragraph.space_after += points;
    }
}

fn heading_font_size(level: u8) -> f32 {
    match level {
        1 => 24.0,
        2 => 20.0,
        3 => 17.0,
        4 => 14.0,
        5 => 12.0,
        _ => 11.0,
    }
}

fn wrap_text(text: &str, max_characters: usize, preserve_whitespace: bool) -> Vec<String> {
    let max_characters = max_characters.max(1);
    if preserve_whitespace {
        return text
            .split('\n')
            .flat_map(|line| {
                let characters = line.chars().collect::<Vec<_>>();
                if characters.is_empty() {
                    vec![String::new()]
                } else {
                    characters
                        .chunks(max_characters)
                        .map(|chunk| chunk.iter().collect())
                        .collect()
                }
            })
            .collect();
    }

    let mut lines = Vec::new();
    for source_line in text.split('\n') {
        let mut line = String::new();
        for word in source_line.split_whitespace() {
            let word_length = word.chars().count();
            if word_length > max_characters {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                let characters = word.chars().collect::<Vec<_>>();
                lines.extend(
                    characters
                        .chunks(max_characters)
                        .map(|chunk| chunk.iter().collect()),
                );
            } else if line.is_empty() {
                line.push_str(word);
            } else if line.chars().count() + 1 + word_length <= max_characters {
                line.push(' ');
                line.push_str(word);
            } else {
                lines.push(std::mem::take(&mut line));
                line.push_str(word);
            }
        }
        lines.push(line);
    }
    lines
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
    text
}

pub(crate) fn convert_pdf_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let pdfium = bundled_pdfium()?;
    let document = pdfium
        .load_pdf_from_file(input, None)
        .map_err(invalid_data)?;
    let mut pages = Vec::with_capacity(usize::try_from(document.pages().len()).unwrap_or_default());
    for index in document.pages().as_range() {
        let page = document.pages().get(index).map_err(invalid_data)?;
        pages.push(page.text().map_err(invalid_data)?.all());
    }
    let text = pages.join("\n");
    let mut markdown = text.trim_end().to_string();

    let image_dir = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(
            || Path::new("image").to_path_buf(),
            |parent| parent.join("image"),
        );
    let image_links = extract_pdf_images(input, &image_dir).unwrap_or_default();
    if !image_links.is_empty() {
        markdown.push_str("\n\n");
        markdown.push_str(&image_links.join("\n\n"));
    }

    std::fs::write(output, format!("{markdown}\n"))?;
    Ok(())
}

static PDFIUM: OnceLock<Result<Pdfium, PdfiumInitializationError>> = OnceLock::new();

#[derive(Debug)]
struct PdfiumInitializationError(String);

impl std::fmt::Display for PdfiumInitializationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PdfiumInitializationError {}

fn bundled_pdfium() -> std::io::Result<&'static Pdfium> {
    match PDFIUM.get_or_init(|| {
        pdfium_bundled::bind_bundled().map_err(|error| PdfiumInitializationError(error.to_string()))
    }) {
        Ok(pdfium) => Ok(pdfium),
        Err(error) => Err(invalid_data(PdfiumInitializationError(error.0.clone()))),
    }
}

/// Extracts every image XObject referenced by the PDF's pages, saving each
/// as a file under `image_dir` and returning a Markdown image link per
/// extracted image, in page/appearance order.
fn extract_pdf_images(input: &Path, image_dir: &Path) -> Result<Vec<String>, MarkoffError> {
    let document = lopdf::Document::load(input).map_err(invalid_data)?;
    let mut links = Vec::new();
    let mut counter = 0usize;

    for (_, page_id) in document.get_pages() {
        let Ok(images) = document.get_page_images(page_id) else {
            continue;
        };
        for image in images {
            let Some((bytes, extension)) = decode_pdf_image(&document, &image) else {
                continue;
            };
            counter += 1;
            let file_name = format!("image{counter}.{extension}");
            std::fs::create_dir_all(image_dir)?;
            std::fs::write(image_dir.join(&file_name), bytes)?;
            links.push(format!("![](image/{file_name})"));
        }
    }
    Ok(links)
}

/// Decodes a PDF image XObject into standalone file bytes, returning the
/// bytes and a matching file extension. JPEG/JPEG2000 streams are already in
/// their target format and are copied as-is; other filters are decompressed
/// to raw samples and re-encoded as PNG. Color spaces and bit depths this
/// does not recognize are skipped rather than mis-rendered.
fn decode_pdf_image(
    document: &lopdf::Document,
    image: &lopdf::xobject::PdfImage<'_>,
) -> Option<(Vec<u8>, &'static str)> {
    let filters = image.filters.clone().unwrap_or_default();
    if filters.iter().any(|filter| filter == "DCTDecode") {
        return Some((image.content.to_vec(), "jpg"));
    }
    if filters.iter().any(|filter| filter == "JPXDecode") {
        return Some((image.content.to_vec(), "jp2"));
    }

    let stream = document.get_object(image.id).ok()?.as_stream().ok()?;
    let samples = stream.decompressed_content().ok()?;
    let width = u32::try_from(image.width).ok()?;
    let height = u32::try_from(image.height).ok()?;
    let bits_per_component = image.bits_per_component.unwrap_or(8);
    let color_type = match (image.color_space.as_deref(), bits_per_component) {
        (Some("DeviceRGB" | "CalRGB"), 8) => png::ColorType::Rgb,
        (Some("DeviceGray" | "CalGray"), 8) => png::ColorType::Grayscale,
        _ => return None,
    };

    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, width, height);
        encoder.set_color(color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&samples).ok()?;
    }
    Some((png_bytes, "png"))
}

#[cfg(test)]
mod tests {
    use super::bundled_pdfium;

    #[test]
    fn reuses_the_bundled_pdfium_binding() {
        let first = bundled_pdfium().unwrap();
        let second = bundled_pdfium().unwrap();

        assert!(std::ptr::eq(first, second));
    }
}
