use crate::MarkoffError;
use serde::Deserialize;
use std::path::Path;

/// RGB color resolved from a style theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleColor {
    /// Red channel.
    pub red: u8,
    /// Green channel.
    pub green: u8,
    /// Blue channel.
    pub blue: u8,
}

impl StyleColor {
    const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub(crate) fn css(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
    }

    pub(crate) fn hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
    }
}

pub(crate) type Rgb = StyleColor;

/// Horizontal alignment of body paragraphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleTextAlign {
    /// Align lines to the left edge.
    Left,
    /// Center each line.
    Center,
    /// Align lines to the right edge.
    Right,
    /// Stretch all lines except the last one to the full width.
    Justify,
}

impl StyleTextAlign {
    /// Name used in TOML themes.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
            Self::Justify => "justify",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "left" => Some(Self::Left),
            "center" => Some(Self::Center),
            "right" => Some(Self::Right),
            "justify" => Some(Self::Justify),
            _ => None,
        }
    }
}

/// Paper size for paged output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StylePageSize {
    /// ISO A3, 297 x 420 mm.
    A3,
    /// ISO A4, 210 x 297 mm.
    A4,
    /// ISO A5, 148 x 210 mm.
    A5,
    /// US Letter, 8.5 x 11 in.
    Letter,
    /// US Legal, 8.5 x 14 in.
    Legal,
}

impl StylePageSize {
    /// Name used in TOML themes.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::A3 => "A3",
            Self::A4 => "A4",
            Self::A5 => "A5",
            Self::Letter => "Letter",
            Self::Legal => "Legal",
        }
    }

    /// Width and height in points for the given orientation.
    #[must_use]
    pub fn dimensions_pt(self, orientation: StylePageOrientation) -> (f32, f32) {
        let (width, height) = match self {
            Self::A3 => (841.89, 1190.55),
            Self::A4 => (595.28, 841.89),
            Self::A5 => (419.53, 595.28),
            Self::Letter => (612.0, 792.0),
            Self::Legal => (612.0, 1008.0),
        };
        match orientation {
            StylePageOrientation::Portrait => (width, height),
            StylePageOrientation::Landscape => (height, width),
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "a3" => Some(Self::A3),
            "a4" => Some(Self::A4),
            "a5" => Some(Self::A5),
            "letter" => Some(Self::Letter),
            "legal" => Some(Self::Legal),
            _ => None,
        }
    }
}

/// Page orientation for paged output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StylePageOrientation {
    /// Height is greater than width.
    Portrait,
    /// Width is greater than height.
    Landscape,
}

impl StylePageOrientation {
    /// Name used in TOML themes.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "portrait" => Some(Self::Portrait),
            "landscape" => Some(Self::Landscape),
            _ => None,
        }
    }
}

/// Resolved values of a TOML style theme.
///
/// Every property omitted from a theme file uses the value from
/// [`StyleThemePreview::default`].
#[derive(Clone, Debug, PartialEq)]
pub struct StyleThemePreview {
    /// Body font family requested by the theme.
    pub font_family: String,
    /// Body font size in points.
    pub font_size_pt: f32,
    /// Body text color.
    pub text_color: StyleColor,
    /// Space before paragraphs in points.
    pub paragraph_spacing_before_pt: f32,
    /// Space after paragraphs in points.
    pub paragraph_spacing_after_pt: f32,
    /// Relative line height.
    pub line_height: f32,
    /// Alignment of body paragraphs.
    pub text_align: StyleTextAlign,
    /// Indent of the first line of body paragraphs in points.
    pub first_line_indent_pt: f32,
    /// Heading font family requested by the theme.
    pub heading_font_family: String,
    /// Heading color used for levels without an explicit level color.
    pub heading_color: StyleColor,
    /// Heading colors for levels one through six.
    pub heading_colors: [StyleColor; 6],
    /// Heading sizes for levels one through six in points.
    pub heading_sizes_pt: [f32; 6],
    /// Space before headings in points.
    pub heading_spacing_before_pt: f32,
    /// Space after headings in points.
    pub heading_spacing_after_pt: f32,
    /// Whether headings are bold.
    pub heading_bold: bool,
    /// Whether headings are italic.
    pub heading_italic: bool,
    /// Paper size.
    pub page_size: StylePageSize,
    /// Paper orientation.
    pub page_orientation: StylePageOrientation,
    /// Top page margin in points.
    pub margin_top_pt: f32,
    /// Right page margin in points.
    pub margin_right_pt: f32,
    /// Bottom page margin in points.
    pub margin_bottom_pt: f32,
    /// Left page margin in points.
    pub margin_left_pt: f32,
    /// Header text; `{page}` and `{pages}` are replaced by page numbers.
    pub header_text: String,
    /// Footer text; `{page}` and `{pages}` are replaced by page numbers.
    pub footer_text: String,
    /// Header and footer font size in points.
    pub header_footer_font_size_pt: f32,
    /// Hyperlink text color.
    pub link_color: StyleColor,
    /// Whether hyperlinks are underlined.
    pub link_underline: bool,
    /// Blockquote text color.
    pub quote_text_color: StyleColor,
    /// Optional blockquote background color.
    pub quote_background: Option<StyleColor>,
    /// Blockquote left border color.
    pub quote_border_color: StyleColor,
    /// Blockquote left border width in points.
    pub quote_border_width_pt: f32,
    /// Blockquote indent in points.
    pub quote_indent_pt: f32,
    /// Whether blockquote text is italic.
    pub quote_italic: bool,
    /// Indent of each list nesting level in points.
    pub list_indent_pt: f32,
    /// Marker used for unordered list items.
    pub list_bullet: String,
    /// Code font family requested by the theme.
    pub code_font_family: String,
    /// Code font size in points.
    pub code_font_size_pt: f32,
    /// Code text color.
    pub code_text_color: StyleColor,
    /// Code block background color.
    pub code_background: StyleColor,
    /// Optional inline code background color.
    pub code_inline_background: Option<StyleColor>,
    /// Inner padding of code blocks in points.
    pub code_padding_pt: f32,
    /// Table text size in points.
    pub table_font_size_pt: f32,
    /// Table header background color.
    pub table_header_background: StyleColor,
    /// Table header text color.
    pub table_header_color: StyleColor,
    /// Table border color.
    pub table_border_color: StyleColor,
    /// Table border width in points.
    pub table_border_width_pt: f32,
    /// Table cell padding in points.
    pub table_cell_padding_pt: f32,
    /// Optional background of every second body row.
    pub table_stripe_background: Option<StyleColor>,
    /// Horizontal rule color.
    pub rule_color: StyleColor,
    /// Horizontal rule thickness in points.
    pub rule_width_pt: f32,
    /// Footnote text size in points.
    pub footnote_font_size_pt: f32,
    /// Maximum image width as a percentage of the text width.
    pub image_max_width_percent: f32,
}

impl Default for StyleThemePreview {
    fn default() -> Self {
        let heading_color = StyleColor::new(31, 78, 121);
        Self {
            font_family: "Calibri".to_string(),
            font_size_pt: 11.0,
            text_color: StyleColor::new(0, 0, 0),
            paragraph_spacing_before_pt: 0.0,
            paragraph_spacing_after_pt: 8.0,
            line_height: 1.2,
            text_align: StyleTextAlign::Left,
            first_line_indent_pt: 0.0,
            heading_font_family: "Calibri".to_string(),
            heading_color,
            heading_colors: [heading_color; 6],
            heading_sizes_pt: [16.0, 14.0, 12.0, 11.0, 10.0, 9.0],
            heading_spacing_before_pt: 10.0,
            heading_spacing_after_pt: 6.0,
            heading_bold: true,
            heading_italic: false,
            page_size: StylePageSize::A4,
            page_orientation: StylePageOrientation::Portrait,
            margin_top_pt: 42.0,
            margin_right_pt: 42.0,
            margin_bottom_pt: 42.0,
            margin_left_pt: 42.0,
            header_text: String::new(),
            footer_text: String::new(),
            header_footer_font_size_pt: 9.0,
            link_color: StyleColor::new(5, 99, 193),
            link_underline: true,
            quote_text_color: StyleColor::new(64, 64, 64),
            quote_background: None,
            quote_border_color: StyleColor::new(179, 179, 179),
            quote_border_width_pt: 1.5,
            quote_indent_pt: 14.0,
            quote_italic: false,
            list_indent_pt: 18.0,
            list_bullet: "•".to_string(),
            code_font_family: "Consolas".to_string(),
            code_font_size_pt: 9.5,
            code_text_color: StyleColor::new(51, 51, 51),
            code_background: StyleColor::new(246, 248, 250),
            code_inline_background: Some(StyleColor::new(246, 248, 250)),
            code_padding_pt: 7.0,
            table_font_size_pt: 9.5,
            table_header_background: StyleColor::new(235, 240, 246),
            table_header_color: StyleColor::new(0, 0, 0),
            table_border_color: StyleColor::new(180, 188, 198),
            table_border_width_pt: 0.5,
            table_cell_padding_pt: 4.0,
            table_stripe_background: None,
            rule_color: StyleColor::new(180, 188, 198),
            rule_width_pt: 0.7,
            footnote_font_size_pt: 9.0,
            image_max_width_percent: 100.0,
        }
    }
}

impl StyleThemePreview {
    /// Heading color for a Markdown heading level (1-6).
    #[must_use]
    pub fn heading_color_for(&self, level: u8) -> StyleColor {
        self.heading_colors[usize::from(level.clamp(1, 6) - 1)]
    }

    /// Heading size for a Markdown heading level (1-6).
    #[must_use]
    pub fn heading_size_for(&self, level: u8) -> f32 {
        self.heading_sizes_pt[usize::from(level.clamp(1, 6) - 1)]
    }

    /// Page width and height in points.
    #[must_use]
    pub fn page_dimensions_pt(&self) -> (f32, f32) {
        self.page_size.dimensions_pt(self.page_orientation)
    }
}

/// Theme passed to writers. `enabled` is `false` when no theme file was
/// supplied; writers then keep their built-in formatting.
#[derive(Clone, Debug, Default)]
pub(crate) struct DocumentTheme {
    pub(crate) enabled: bool,
    pub(crate) values: StyleThemePreview,
}

impl std::ops::Deref for DocumentTheme {
    type Target = StyleThemePreview;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct ThemeFile {
    document: DocumentSection,
    headings: HeadingSection,
    page: PageSection,
    links: LinkSection,
    blockquote: BlockquoteSection,
    lists: ListSection,
    code: CodeSection,
    table: TableSection,
    horizontal_rule: RuleSection,
    footnotes: FootnoteSection,
    images: ImageSection,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct DocumentSection {
    font_family: Option<String>,
    font_size_pt: Option<f32>,
    text_color: Option<String>,
    paragraph_spacing_before_pt: Option<f32>,
    paragraph_spacing_after_pt: Option<f32>,
    line_height: Option<f32>,
    text_align: Option<String>,
    first_line_indent_pt: Option<f32>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct HeadingSection {
    font_family: Option<String>,
    color: Option<String>,
    level_colors: Option<Vec<String>>,
    sizes_pt: Option<Vec<f32>>,
    spacing_before_pt: Option<f32>,
    spacing_after_pt: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct PageSection {
    size: Option<String>,
    orientation: Option<String>,
    margin_top_pt: Option<f32>,
    margin_right_pt: Option<f32>,
    margin_bottom_pt: Option<f32>,
    margin_left_pt: Option<f32>,
    header_text: Option<String>,
    footer_text: Option<String>,
    header_footer_font_size_pt: Option<f32>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct LinkSection {
    color: Option<String>,
    underline: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct BlockquoteSection {
    text_color: Option<String>,
    background: Option<String>,
    border_color: Option<String>,
    border_width_pt: Option<f32>,
    indent_pt: Option<f32>,
    italic: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct ListSection {
    indent_pt: Option<f32>,
    bullet: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct CodeSection {
    font_family: Option<String>,
    font_size_pt: Option<f32>,
    text_color: Option<String>,
    background: Option<String>,
    inline_background: Option<String>,
    padding_pt: Option<f32>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct TableSection {
    font_size_pt: Option<f32>,
    header_background: Option<String>,
    header_color: Option<String>,
    border_color: Option<String>,
    border_width_pt: Option<f32>,
    cell_padding_pt: Option<f32>,
    stripe_background: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RuleSection {
    color: Option<String>,
    width_pt: Option<f32>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct FootnoteSection {
    font_size_pt: Option<f32>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct ImageSection {
    max_width_percent: Option<f32>,
}

pub(crate) fn load_document_theme(path: Option<&Path>) -> Result<DocumentTheme, MarkoffError> {
    let Some(path) = path else {
        return Ok(DocumentTheme::default());
    };
    let source = std::fs::read_to_string(path).map_err(|error| MarkoffError::InvalidOption {
        message: format!("unable to read style theme {}: {error}", path.display()),
    })?;
    let file: ThemeFile = toml::from_str(&source).map_err(|error| MarkoffError::InvalidOption {
        message: format!("invalid style theme {}: {error}", path.display()),
    })?;
    resolve_theme(file)
}

/// Loads and validates a TOML style theme for GUI preview.
///
/// # Errors
///
/// Returns [`MarkoffError::InvalidOption`] when the file cannot be read or
/// contains unknown fields, invalid colors, or invalid values.
pub fn load_style_theme_preview(path: &Path) -> Result<StyleThemePreview, MarkoffError> {
    load_document_theme(Some(path)).map(|theme| theme.values)
}

/// Returns an editable TOML style theme containing every supported setting
/// with its default value and a short explanatory comment.
///
/// Loading the returned text as a theme produces the same values that are
/// used for any property omitted from a partial theme.
#[must_use]
pub fn default_style_theme_toml() -> String {
    style_theme_toml(&StyleThemePreview::default())
}

pub(crate) fn style_theme_toml(theme: &StyleThemePreview) -> String {
    let sizes = theme
        .heading_sizes_pt
        .iter()
        .map(|size| toml_number(*size))
        .collect::<Vec<_>>()
        .join(", ");
    let level_colors = theme
        .heading_colors
        .iter()
        .map(|color| format!("\"{}\"", color.css()))
        .collect::<Vec<_>>()
        .join(", ");
    let level_colors_setting = if theme.heading_colors == [theme.heading_color; 6] {
        format!("# level_colors = [{level_colors}]")
    } else {
        format!("level_colors = [{level_colors}]")
    };
    format!(
        r##"# Markoff style theme.
# Applies to PDF, HTML, DOC/DOCX, and ODT output:
#   markoff convert input.md -o output.pdf --style style-theme.toml
# Omitted properties use Markoff's default values.
# Remove lines you do not need; unknown keys are rejected.
# Sizes, spacing, indents, and margins are in points (1 pt = 1/72 inch).
# Colors use the #RRGGBB format; "none" disables an optional background.

[document]
# Body font family. It must be installed or available to the viewer.
font_family = {font_family}
# Body font size, positive number.
font_size_pt = {font_size_pt}
# Body text color.
text_color = "{text_color}"
# Space before and after each paragraph, zero or greater.
paragraph_spacing_before_pt = {paragraph_spacing_before_pt}
paragraph_spacing_after_pt = {paragraph_spacing_after_pt}
# Line height as a multiple of the font size, positive number.
line_height = {line_height}
# Paragraph alignment: "left", "center", "right", or "justify".
text_align = "{text_align}"
# Indent of the first line of each paragraph, zero or greater.
first_line_indent_pt = {first_line_indent_pt}

[headings]
# Heading font family.
font_family = {heading_font_family}
# Heading text color for all levels.
color = "{heading_color}"
# Optional separate colors for levels 1-6; overrides "color".
{level_colors_setting}
# Sizes for heading levels 1-6: exactly six positive numbers.
sizes_pt = [{sizes}]
# Space before and after each heading, zero or greater.
spacing_before_pt = {heading_spacing_before_pt}
spacing_after_pt = {heading_spacing_after_pt}
# Heading weight and slant.
bold = {heading_bold}
italic = {heading_italic}

[page]
# Paper size: "A3", "A4", "A5", "Letter", or "Legal".
size = "{page_size}"
# "portrait" or "landscape".
orientation = "{page_orientation}"
# Page margins for PDF, DOC/DOCX, and ODT; HTML applies them to <body>.
margin_top_pt = {margin_top_pt}
margin_right_pt = {margin_right_pt}
margin_bottom_pt = {margin_bottom_pt}
margin_left_pt = {margin_left_pt}
# Text repeated at the top and bottom of every page; empty disables it.
# {{page}} is replaced by the page number and {{pages}} by the page count.
# Example: footer_text = "Page {{page}} of {{pages}}"
header_text = {header_text}
footer_text = {footer_text}
# Header and footer font size.
header_footer_font_size_pt = {header_footer_font_size_pt}

[links]
# Hyperlink text color and underline.
color = "{link_color}"
underline = {link_underline}

[blockquote]
# Quote text color.
text_color = "{quote_text_color}"
# Quote background color or "none".
background = {quote_background}
# Color and width of the bar on the left side of the quote.
border_color = "{quote_border_color}"
border_width_pt = {quote_border_width_pt}
# Left indent of the quote.
indent_pt = {quote_indent_pt}
# Italic quote text.
italic = {quote_italic}

[lists]
# Indent of each nesting level.
indent_pt = {list_indent_pt}
# Marker of unordered list items (1-4 characters).
bullet = {list_bullet}

[code]
# Monospaced font for inline code and code blocks.
font_family = {code_font_family}
# Code font size, positive number.
font_size_pt = {code_font_size_pt}
# Code text color.
text_color = "{code_text_color}"
# Code block background color.
background = "{code_background}"
# Inline code background color or "none".
inline_background = {code_inline_background}
# Inner padding of code blocks.
padding_pt = {code_padding_pt}

[table]
# Table text size, positive number.
font_size_pt = {table_font_size_pt}
# Background of the first (header) table row.
header_background = "{table_header_background}"
# Text color of the header row.
header_color = "{table_header_color}"
# Table cell border color and width.
border_color = "{table_border_color}"
border_width_pt = {table_border_width_pt}
# Space between a cell border and its text.
cell_padding_pt = {table_cell_padding_pt}
# Background of every second body row or "none".
stripe_background = {table_stripe_background}

[horizontal_rule]
# Color and thickness of "---" separators.
color = "{rule_color}"
width_pt = {rule_width_pt}

[footnotes]
# Footnote text size.
font_size_pt = {footnote_font_size_pt}

[images]
# Maximum image width as a percentage of the text width (1-100).
max_width_percent = {image_max_width_percent}
"##,
        font_family = toml_string(&theme.font_family),
        font_size_pt = toml_number(theme.font_size_pt),
        text_color = theme.text_color.css(),
        paragraph_spacing_before_pt = toml_number(theme.paragraph_spacing_before_pt),
        paragraph_spacing_after_pt = toml_number(theme.paragraph_spacing_after_pt),
        line_height = toml_number(theme.line_height),
        text_align = theme.text_align.name(),
        first_line_indent_pt = toml_number(theme.first_line_indent_pt),
        heading_font_family = toml_string(&theme.heading_font_family),
        heading_color = theme.heading_color.css(),
        level_colors_setting = level_colors_setting,
        heading_spacing_before_pt = toml_number(theme.heading_spacing_before_pt),
        heading_spacing_after_pt = toml_number(theme.heading_spacing_after_pt),
        heading_bold = theme.heading_bold,
        heading_italic = theme.heading_italic,
        page_size = theme.page_size.name(),
        page_orientation = theme.page_orientation.name(),
        margin_top_pt = toml_number(theme.margin_top_pt),
        margin_right_pt = toml_number(theme.margin_right_pt),
        margin_bottom_pt = toml_number(theme.margin_bottom_pt),
        margin_left_pt = toml_number(theme.margin_left_pt),
        header_text = toml_string(&theme.header_text),
        footer_text = toml_string(&theme.footer_text),
        header_footer_font_size_pt = toml_number(theme.header_footer_font_size_pt),
        link_color = theme.link_color.css(),
        link_underline = theme.link_underline,
        quote_text_color = theme.quote_text_color.css(),
        quote_background = toml_optional_color(theme.quote_background),
        quote_border_color = theme.quote_border_color.css(),
        quote_border_width_pt = toml_number(theme.quote_border_width_pt),
        quote_indent_pt = toml_number(theme.quote_indent_pt),
        quote_italic = theme.quote_italic,
        list_indent_pt = toml_number(theme.list_indent_pt),
        list_bullet = toml_string(&theme.list_bullet),
        code_font_family = toml_string(&theme.code_font_family),
        code_font_size_pt = toml_number(theme.code_font_size_pt),
        code_text_color = theme.code_text_color.css(),
        code_background = theme.code_background.css(),
        code_inline_background = toml_optional_color(theme.code_inline_background),
        code_padding_pt = toml_number(theme.code_padding_pt),
        table_font_size_pt = toml_number(theme.table_font_size_pt),
        table_header_background = theme.table_header_background.css(),
        table_header_color = theme.table_header_color.css(),
        table_border_color = theme.table_border_color.css(),
        table_border_width_pt = toml_number(theme.table_border_width_pt),
        table_cell_padding_pt = toml_number(theme.table_cell_padding_pt),
        table_stripe_background = toml_optional_color(theme.table_stripe_background),
        rule_color = theme.rule_color.css(),
        rule_width_pt = toml_number(theme.rule_width_pt),
        footnote_font_size_pt = toml_number(theme.footnote_font_size_pt),
        image_max_width_percent = toml_number(theme.image_max_width_percent),
    )
}

/// Writes the default editable style theme returned by
/// [`default_style_theme_toml`] to `path`, creating parent directories.
///
/// # Errors
///
/// Returns [`MarkoffError::OutputExists`] when `path` exists and `overwrite`
/// is `false`, [`MarkoffError::OutputDirectory`] when the parent directory
/// cannot be created, or [`MarkoffError::Io`] when writing fails.
pub fn write_default_style_theme(path: &Path, overwrite: bool) -> Result<(), MarkoffError> {
    write_style_theme(path, &StyleThemePreview::default(), overwrite)
}

pub(crate) fn write_style_theme(
    path: &Path,
    theme: &StyleThemePreview,
    overwrite: bool,
) -> Result<(), MarkoffError> {
    write_theme_toml(path, &style_theme_toml(theme), overwrite)
}

fn write_theme_toml(path: &Path, source: &str, overwrite: bool) -> Result<(), MarkoffError> {
    if !overwrite && path.exists() {
        return Err(MarkoffError::OutputExists {
            path: path.display().to_string(),
        });
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|_| MarkoffError::OutputDirectory {
            path: path.display().to_string(),
        })?;
    }
    std::fs::write(path, source)?;
    Ok(())
}

fn toml_number(value: f32) -> String {
    format!("{value:?}")
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

fn toml_optional_color(value: Option<StyleColor>) -> String {
    value.map_or_else(
        || "\"none\"".to_string(),
        |color| format!("\"{}\"", color.css()),
    )
}

fn resolve_theme(file: ThemeFile) -> Result<DocumentTheme, MarkoffError> {
    let mut theme = StyleThemePreview::default();
    let ThemeFile {
        document,
        headings,
        page,
        links,
        blockquote,
        lists,
        code,
        table,
        horizontal_rule,
        footnotes,
        images,
    } = file;

    assign_string(
        &mut theme.font_family,
        document.font_family,
        "document.font_family",
    )?;
    assign_positive(
        &mut theme.font_size_pt,
        document.font_size_pt,
        "document.font_size_pt",
    )?;
    assign_color(
        &mut theme.text_color,
        document.text_color,
        "document.text_color",
    )?;
    assign_non_negative(
        &mut theme.paragraph_spacing_before_pt,
        document.paragraph_spacing_before_pt,
        "document.paragraph_spacing_before_pt",
    )?;
    assign_non_negative(
        &mut theme.paragraph_spacing_after_pt,
        document.paragraph_spacing_after_pt,
        "document.paragraph_spacing_after_pt",
    )?;
    assign_positive(
        &mut theme.line_height,
        document.line_height,
        "document.line_height",
    )?;
    assign_parsed(
        &mut theme.text_align,
        document.text_align,
        StyleTextAlign::parse,
        "document.text_align must be \"left\", \"center\", \"right\", or \"justify\"",
    )?;
    assign_non_negative(
        &mut theme.first_line_indent_pt,
        document.first_line_indent_pt,
        "document.first_line_indent_pt",
    )?;

    assign_string(
        &mut theme.heading_font_family,
        headings.font_family,
        "headings.font_family",
    )?;
    assign_color(&mut theme.heading_color, headings.color, "headings.color")?;
    theme.heading_colors = [theme.heading_color; 6];
    if let Some(colors) = headings.level_colors {
        if colors.len() != 6 {
            return Err(invalid_theme(
                "headings.level_colors must contain six #RRGGBB colors",
            ));
        }
        for (target, value) in theme.heading_colors.iter_mut().zip(colors) {
            *target = parse_color(&value).ok_or_else(|| {
                invalid_theme("headings.level_colors must contain six #RRGGBB colors")
            })?;
        }
    }
    if let Some(sizes) = headings.sizes_pt {
        if sizes.len() != 6 || sizes.iter().any(|size| !size.is_finite() || *size <= 0.0) {
            return Err(invalid_theme(
                "headings.sizes_pt must contain six positive numbers",
            ));
        }
        theme.heading_sizes_pt.copy_from_slice(&sizes);
    }
    assign_non_negative(
        &mut theme.heading_spacing_before_pt,
        headings.spacing_before_pt,
        "headings.spacing_before_pt",
    )?;
    assign_non_negative(
        &mut theme.heading_spacing_after_pt,
        headings.spacing_after_pt,
        "headings.spacing_after_pt",
    )?;
    assign_value(&mut theme.heading_bold, headings.bold);
    assign_value(&mut theme.heading_italic, headings.italic);

    assign_parsed(
        &mut theme.page_size,
        page.size,
        StylePageSize::parse,
        "page.size must be \"A3\", \"A4\", \"A5\", \"Letter\", or \"Legal\"",
    )?;
    assign_parsed(
        &mut theme.page_orientation,
        page.orientation,
        StylePageOrientation::parse,
        "page.orientation must be \"portrait\" or \"landscape\"",
    )?;
    assign_non_negative(
        &mut theme.margin_top_pt,
        page.margin_top_pt,
        "page.margin_top_pt",
    )?;
    assign_non_negative(
        &mut theme.margin_right_pt,
        page.margin_right_pt,
        "page.margin_right_pt",
    )?;
    assign_non_negative(
        &mut theme.margin_bottom_pt,
        page.margin_bottom_pt,
        "page.margin_bottom_pt",
    )?;
    assign_non_negative(
        &mut theme.margin_left_pt,
        page.margin_left_pt,
        "page.margin_left_pt",
    )?;
    assign_single_line(&mut theme.header_text, page.header_text, "page.header_text")?;
    assign_single_line(&mut theme.footer_text, page.footer_text, "page.footer_text")?;
    assign_positive(
        &mut theme.header_footer_font_size_pt,
        page.header_footer_font_size_pt,
        "page.header_footer_font_size_pt",
    )?;
    let (page_width, page_height) = theme.page_dimensions_pt();
    if theme.margin_left_pt + theme.margin_right_pt >= page_width - 72.0
        || theme.margin_top_pt + theme.margin_bottom_pt >= page_height - 72.0
    {
        return Err(invalid_theme(
            "page margins must leave at least 72 pt (1 inch) of text area",
        ));
    }

    assign_color(&mut theme.link_color, links.color, "links.color")?;
    assign_value(&mut theme.link_underline, links.underline);

    assign_color(
        &mut theme.quote_text_color,
        blockquote.text_color,
        "blockquote.text_color",
    )?;
    assign_optional_color(
        &mut theme.quote_background,
        blockquote.background,
        "blockquote.background",
    )?;
    assign_color(
        &mut theme.quote_border_color,
        blockquote.border_color,
        "blockquote.border_color",
    )?;
    assign_non_negative(
        &mut theme.quote_border_width_pt,
        blockquote.border_width_pt,
        "blockquote.border_width_pt",
    )?;
    assign_non_negative(
        &mut theme.quote_indent_pt,
        blockquote.indent_pt,
        "blockquote.indent_pt",
    )?;
    assign_value(&mut theme.quote_italic, blockquote.italic);

    assign_non_negative(
        &mut theme.list_indent_pt,
        lists.indent_pt,
        "lists.indent_pt",
    )?;
    if let Some(bullet) = lists.bullet {
        let length = bullet.chars().count();
        if length == 0 || length > 4 || bullet.chars().any(char::is_control) {
            return Err(invalid_theme(
                "lists.bullet must contain 1-4 printable characters",
            ));
        }
        theme.list_bullet = bullet;
    }

    assign_string(
        &mut theme.code_font_family,
        code.font_family,
        "code.font_family",
    )?;
    assign_positive(
        &mut theme.code_font_size_pt,
        code.font_size_pt,
        "code.font_size_pt",
    )?;
    assign_color(
        &mut theme.code_text_color,
        code.text_color,
        "code.text_color",
    )?;
    assign_color(
        &mut theme.code_background,
        code.background,
        "code.background",
    )?;
    assign_optional_color(
        &mut theme.code_inline_background,
        code.inline_background,
        "code.inline_background",
    )?;
    assign_non_negative(
        &mut theme.code_padding_pt,
        code.padding_pt,
        "code.padding_pt",
    )?;

    assign_positive(
        &mut theme.table_font_size_pt,
        table.font_size_pt,
        "table.font_size_pt",
    )?;
    assign_color(
        &mut theme.table_header_background,
        table.header_background,
        "table.header_background",
    )?;
    assign_color(
        &mut theme.table_header_color,
        table.header_color,
        "table.header_color",
    )?;
    assign_color(
        &mut theme.table_border_color,
        table.border_color,
        "table.border_color",
    )?;
    assign_non_negative(
        &mut theme.table_border_width_pt,
        table.border_width_pt,
        "table.border_width_pt",
    )?;
    assign_non_negative(
        &mut theme.table_cell_padding_pt,
        table.cell_padding_pt,
        "table.cell_padding_pt",
    )?;
    assign_optional_color(
        &mut theme.table_stripe_background,
        table.stripe_background,
        "table.stripe_background",
    )?;

    assign_color(
        &mut theme.rule_color,
        horizontal_rule.color,
        "horizontal_rule.color",
    )?;
    assign_non_negative(
        &mut theme.rule_width_pt,
        horizontal_rule.width_pt,
        "horizontal_rule.width_pt",
    )?;

    assign_positive(
        &mut theme.footnote_font_size_pt,
        footnotes.font_size_pt,
        "footnotes.font_size_pt",
    )?;

    if let Some(percent) = images.max_width_percent {
        if !percent.is_finite() || percent < 1.0 || percent > 100.0 {
            return Err(invalid_theme(
                "images.max_width_percent must be between 1 and 100",
            ));
        }
        theme.image_max_width_percent = percent;
    }

    Ok(DocumentTheme {
        enabled: true,
        values: theme,
    })
}

fn assign_value<T>(target: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *target = value;
    }
}

fn assign_parsed<T>(
    target: &mut T,
    value: Option<String>,
    parse: fn(&str) -> Option<T>,
    message: &str,
) -> Result<(), MarkoffError> {
    if let Some(value) = value {
        *target = parse(value.trim()).ok_or_else(|| invalid_theme(message))?;
    }
    Ok(())
}

fn assign_string(
    target: &mut String,
    value: Option<String>,
    field: &str,
) -> Result<(), MarkoffError> {
    if let Some(value) = value {
        if value.trim().is_empty() {
            return Err(invalid_theme(&format!("{field} must not be empty")));
        }
        *target = value;
    }
    Ok(())
}

fn assign_single_line(
    target: &mut String,
    value: Option<String>,
    field: &str,
) -> Result<(), MarkoffError> {
    if let Some(value) = value {
        if value.chars().any(char::is_control) {
            return Err(invalid_theme(&format!(
                "{field} must be a single line of text"
            )));
        }
        *target = value;
    }
    Ok(())
}

fn assign_positive(target: &mut f32, value: Option<f32>, field: &str) -> Result<(), MarkoffError> {
    if let Some(value) = value {
        if !value.is_finite() || value <= 0.0 {
            return Err(invalid_theme(&format!("{field} must be positive")));
        }
        *target = value;
    }
    Ok(())
}

fn assign_non_negative(
    target: &mut f32,
    value: Option<f32>,
    field: &str,
) -> Result<(), MarkoffError> {
    if let Some(value) = value {
        if !value.is_finite() || value < 0.0 {
            return Err(invalid_theme(&format!("{field} must not be negative")));
        }
        *target = value;
    }
    Ok(())
}

fn assign_color(target: &mut Rgb, value: Option<String>, field: &str) -> Result<(), MarkoffError> {
    if let Some(value) = value {
        *target = parse_color(&value)
            .ok_or_else(|| invalid_theme(&format!("{field} must use #RRGGBB")))?;
    }
    Ok(())
}

fn assign_optional_color(
    target: &mut Option<Rgb>,
    value: Option<String>,
    field: &str,
) -> Result<(), MarkoffError> {
    if let Some(value) = value {
        *target =
            if value.trim().eq_ignore_ascii_case("none") {
                None
            } else {
                Some(parse_color(&value).ok_or_else(|| {
                    invalid_theme(&format!("{field} must use #RRGGBB or \"none\""))
                })?)
            };
    }
    Ok(())
}

fn parse_color(value: &str) -> Option<Rgb> {
    let value = value.trim().strip_prefix('#')?;
    if value.len() != 6 || !value.is_ascii() {
        return None;
    }
    Some(Rgb {
        red: u8::from_str_radix(&value[0..2], 16).ok()?,
        green: u8::from_str_radix(&value[2..4], 16).ok()?,
        blue: u8::from_str_radix(&value[4..6], 16).ok()?,
    })
}

fn invalid_theme(message: &str) -> MarkoffError {
    MarkoffError::InvalidOption {
        message: format!("invalid style theme: {message}"),
    }
}

/// Splits header or footer text into literal segments and page fields.
pub(crate) fn page_field_segments(text: &str) -> Vec<PageTextSegment<'_>> {
    let mut segments = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let next = [
            ("{page}", PageTextSegment::Page),
            ("{pages}", PageTextSegment::Pages),
        ]
        .into_iter()
        .filter_map(|(token, segment)| rest.find(token).map(|index| (index, token, segment)))
        .min_by_key(|(index, _, _)| *index);
        match next {
            Some((index, token, segment)) => {
                if index > 0 {
                    segments.push(PageTextSegment::Text(&rest[..index]));
                }
                segments.push(segment);
                rest = &rest[index + token.len()..];
            }
            None => {
                segments.push(PageTextSegment::Text(rest));
                break;
            }
        }
    }
    segments
}

/// Part of header or footer text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PageTextSegment<'a> {
    Text(&'a str),
    Page,
    Pages,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_theme(source: &str) -> Result<DocumentTheme, MarkoffError> {
        resolve_theme(
            toml::from_str(source).map_err(|error| MarkoffError::InvalidOption {
                message: error.to_string(),
            })?,
        )
    }

    #[test]
    fn default_template_round_trips_to_default_theme() {
        let loaded = parse_theme(&default_style_theme_toml()).unwrap();
        assert!(loaded.enabled);
        assert_eq!(loaded.values, StyleThemePreview::default());
    }

    #[test]
    fn default_template_lists_every_theme_key() {
        let template = default_style_theme_toml();
        let uncommented = template
            .lines()
            .map(|line| {
                line.strip_prefix("# level_colors")
                    .map_or(line.to_string(), |rest| format!("level_colors{rest}"))
            })
            .collect::<Vec<_>>()
            .join("\n");
        let value: toml::Table = toml::from_str(&uncommented).unwrap();
        let file: ThemeFile = toml::from_str(&uncommented).unwrap();
        let parsed = resolve_theme(file).unwrap();
        assert_eq!(parsed.values, StyleThemePreview::default());
        let expected = [
            (
                "document",
                &[
                    "font_family",
                    "font_size_pt",
                    "text_color",
                    "paragraph_spacing_before_pt",
                    "paragraph_spacing_after_pt",
                    "line_height",
                    "text_align",
                    "first_line_indent_pt",
                ][..],
            ),
            (
                "headings",
                &[
                    "font_family",
                    "color",
                    "level_colors",
                    "sizes_pt",
                    "spacing_before_pt",
                    "spacing_after_pt",
                    "bold",
                    "italic",
                ][..],
            ),
            (
                "page",
                &[
                    "size",
                    "orientation",
                    "margin_top_pt",
                    "margin_right_pt",
                    "margin_bottom_pt",
                    "margin_left_pt",
                    "header_text",
                    "footer_text",
                    "header_footer_font_size_pt",
                ][..],
            ),
            ("links", &["color", "underline"][..]),
            (
                "blockquote",
                &[
                    "text_color",
                    "background",
                    "border_color",
                    "border_width_pt",
                    "indent_pt",
                    "italic",
                ][..],
            ),
            ("lists", &["indent_pt", "bullet"][..]),
            (
                "code",
                &[
                    "font_family",
                    "font_size_pt",
                    "text_color",
                    "background",
                    "inline_background",
                    "padding_pt",
                ][..],
            ),
            (
                "table",
                &[
                    "font_size_pt",
                    "header_background",
                    "header_color",
                    "border_color",
                    "border_width_pt",
                    "cell_padding_pt",
                    "stripe_background",
                ][..],
            ),
            ("horizontal_rule", &["color", "width_pt"][..]),
            ("footnotes", &["font_size_pt"][..]),
            ("images", &["max_width_percent"][..]),
        ];
        assert_eq!(value.len(), expected.len());
        for (section, keys) in expected {
            let table = value[section].as_table().unwrap();
            assert_eq!(table.len(), keys.len(), "{section}");
            for key in keys {
                assert!(table.contains_key(*key), "{section}.{key}");
            }
        }
    }

    #[test]
    fn resolves_extended_theme_settings() {
        let theme = parse_theme(
            r##"
[document]
text_align = "justify"
first_line_indent_pt = 12
[headings]
color = "#112233"
level_colors = ["#010101", "#020202", "#030303", "#040404", "#050505", "#060606"]
italic = true
bold = false
[page]
size = "letter"
orientation = "landscape"
footer_text = "Page {page} of {pages}"
[blockquote]
background = "#EEEEEE"
[code]
inline_background = "none"
[table]
stripe_background = "#FAFAFA"
[lists]
bullet = "–"
[images]
max_width_percent = 50
"##,
        )
        .unwrap();
        assert_eq!(theme.text_align, StyleTextAlign::Justify);
        assert_eq!(theme.heading_color_for(3), StyleColor::new(3, 3, 3));
        assert!(!theme.heading_bold && theme.heading_italic);
        assert_eq!(theme.page_dimensions_pt(), (792.0, 612.0));
        assert_eq!(theme.quote_background, Some(StyleColor::new(238, 238, 238)));
        assert_eq!(theme.code_inline_background, None);
        assert_eq!(theme.list_bullet, "–");
        assert_eq!(theme.image_max_width_percent, 50.0);
        assert_eq!(
            page_field_segments(&theme.footer_text),
            vec![
                PageTextSegment::Text("Page "),
                PageTextSegment::Page,
                PageTextSegment::Text(" of "),
                PageTextSegment::Pages,
            ]
        );
    }

    #[test]
    fn rejects_invalid_extended_values() {
        for source in [
            "[document]\ntext_align = \"middle\"",
            "[page]\nsize = \"B5\"",
            "[page]\norientation = \"sideways\"",
            "[page]\nmargin_left_pt = 400\nmargin_right_pt = 200",
            "[page]\nfooter_text = \"a\\nb\"",
            "[headings]\nlevel_colors = [\"#000000\"]",
            "[lists]\nbullet = \"\"",
            "[images]\nmax_width_percent = 150",
            "[table]\nstripe_background = \"grey\"",
            "[links]\nunderline = \"yes\"",
        ] {
            assert!(parse_theme(source).is_err(), "{source}");
        }
    }

    #[test]
    fn writes_default_template_without_overwriting_by_default() {
        let directory = std::env::temp_dir().join(format!(
            "markoff-style-template-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = directory.join("themes").join("default.toml");
        write_default_style_theme(&path, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            default_style_theme_toml()
        );
        assert!(matches!(
            write_default_style_theme(&path, false),
            Err(MarkoffError::OutputExists { .. })
        ));
        write_default_style_theme(&path, true).unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }
}
