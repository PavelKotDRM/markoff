use crate::MarkoffError;
use crate::document_model::{Document, TableAlignment};
use pdfium_bundled::pdfium_render::prelude::{
    PdfColor, PdfDocument, PdfFontToken, PdfPage, Pdfium,
};
use std::collections::HashMap;
use std::path::Path;

mod content;
mod navigation;
mod render;
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
    content::append_blocks(&document.blocks, 0.0, 0, &mut elements)?;

    let pdfium = crate::pdf::bundled_pdfium()?;
    let mut writer = PdfWriter::new(pdfium)?;
    for element in elements {
        writer.write_element(element)?;
    }
    writer.finish(output)
}
