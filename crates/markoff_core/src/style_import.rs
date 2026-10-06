use crate::error::invalid_data;
use crate::legacy_office;
use crate::style::{
    StyleColor, StylePageOrientation, StylePageSize, StyleTextAlign, StyleThemePreview,
    write_style_theme,
};
use crate::xml_utils::{attribute_value, parse_relationships};
use crate::{Format, MarkoffError, detect_format};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::Path;

/// Extracts supported formatting from a Word, Excel, PowerPoint, or
/// OpenDocument file and writes it as an editable TOML theme.
///
/// # Errors
///
/// Returns an error when the source format is unsupported, the document
/// cannot be read, or the destination cannot be written.
pub fn write_style_theme_from_document(
    input: &Path,
    output: &Path,
    overwrite: bool,
) -> Result<(), MarkoffError> {
    if output.exists() && same_file::is_same_file(input, output)? {
        return Err(MarkoffError::InvalidInput {
            path: "style source and theme destination refer to the same file".to_string(),
        });
    }

    let format = detect_format(input)?;
    if legacy_office::modern_equivalent(format).is_some() {
        let workspace = legacy_office::OfficeWorkspace::new()?;
        let modern_input = legacy_office::convert_to_modern(input, format, &workspace)?;
        return write_style_theme_from_document(&modern_input, output, overwrite);
    }

    let theme = match format {
        Format::Docx => extract_docx_theme(input)?,
        Format::Xlsx => extract_xlsx_theme(input)?,
        Format::Ods => extract_ods_theme(input)?,
        Format::Pptx => extract_pptx_theme(input)?,
        format => {
            return Err(MarkoffError::InvalidOption {
                message: format!(
                    "style import supports DOC/DOCX, XLS/XLSX/XLSM, PPT/PPTX, and ODS files, not {format}"
                ),
            });
        }
    };

    write_style_theme(output, &theme, overwrite)
}

fn read_zip_text_part(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<String, MarkoffError> {
    let mut part = archive.by_name(name).map_err(invalid_data)?;
    let mut source = String::new();
    part.read_to_string(&mut source)?;
    Ok(source)
}

fn read_optional_zip_text_part(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<Option<String>, MarkoffError> {
    let mut part = match archive.by_name(name) {
        Ok(part) => part,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(invalid_data(error).into()),
    };
    let mut source = String::new();
    part.read_to_string(&mut source)?;
    Ok(Some(source))
}

fn extract_docx_theme(input: &Path) -> Result<StyleThemePreview, MarkoffError> {
    let file = std::fs::File::open(input)?;
    let mut archive = zip::ZipArchive::new(file).map_err(invalid_data)?;
    let document = read_zip_text_part(&mut archive, "word/document.xml")?;
    let styles = read_optional_zip_text_part(&mut archive, "word/styles.xml")?;
    let theme_fonts = read_optional_zip_text_part(&mut archive, "word/theme/theme1.xml")?
        .map(|source| parse_word_theme_fonts(&source))
        .transpose()?
        .unwrap_or_default();
    let mut theme = StyleThemePreview::default();

    if let Some(styles) = styles {
        apply_word_styles(&styles, &theme_fonts, &mut theme)?;
    }
    apply_word_page_settings(&document, &mut theme)?;
    Ok(theme)
}

#[derive(Clone, Copy)]
enum WordPropertyScope {
    None,
    Run,
    Paragraph,
}

#[derive(Clone, Default)]
struct WordRunProperties {
    font_family: Option<String>,
    font_theme: Option<String>,
    font_size_pt: Option<f32>,
    color: Option<StyleColor>,
    bold: Option<bool>,
    italic: Option<bool>,
}

#[derive(Clone, Default)]
struct WordParagraphProperties {
    spacing_before_pt: Option<f32>,
    spacing_after_pt: Option<f32>,
    line: Option<(f32, Option<String>)>,
    text_align: Option<StyleTextAlign>,
    first_line_indent_pt: Option<f32>,
}

#[derive(Clone, Default)]
struct WordStyleProperties {
    run: WordRunProperties,
    paragraph: WordParagraphProperties,
}

#[derive(Default)]
struct WordStyle {
    id: String,
    name: Option<String>,
    based_on: Option<String>,
    is_default: bool,
    properties: WordStyleProperties,
}

#[derive(Default)]
struct WordStyles {
    defaults: WordStyleProperties,
    styles: BTreeMap<String, WordStyle>,
}

fn parse_word_styles(source: &str) -> Result<WordStyles, MarkoffError> {
    let mut reader = Reader::from_str(source);
    let mut styles = WordStyles::default();
    let mut current_style = None;
    let mut in_doc_defaults = false;
    let mut scope = WordPropertyScope::None;

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => {
                let name = local_name(&tag);
                match name.as_str() {
                    "docDefaults" => in_doc_defaults = true,
                    "style" => {
                        current_style = word_style_from_tag(&tag)?;
                        scope = WordPropertyScope::None;
                    }
                    "rPr" => scope = WordPropertyScope::Run,
                    "pPr" => scope = WordPropertyScope::Paragraph,
                    _ => apply_word_style_tag(
                        &name,
                        &tag,
                        &mut current_style,
                        &mut styles.defaults,
                        in_doc_defaults,
                        scope,
                    )?,
                }
            }
            Event::Empty(tag) => {
                let name = local_name(&tag);
                if name == "style" {
                    if let Some(style) = word_style_from_tag(&tag)? {
                        styles.styles.insert(style.id.clone(), style);
                    }
                } else {
                    apply_word_style_tag(
                        &name,
                        &tag,
                        &mut current_style,
                        &mut styles.defaults,
                        in_doc_defaults,
                        scope,
                    )?;
                }
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                "docDefaults" => in_doc_defaults = false,
                "rPr" | "pPr" => scope = WordPropertyScope::None,
                "style" => {
                    if let Some(style) = current_style.take()
                        && !style.id.is_empty()
                    {
                        styles.styles.insert(style.id.clone(), style);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(styles)
}

fn word_style_from_tag(tag: &BytesStart<'_>) -> Result<Option<WordStyle>, MarkoffError> {
    if attribute_value(tag, "type")?.as_deref() != Some("paragraph") {
        return Ok(None);
    }
    Ok(Some(WordStyle {
        id: attribute_value(tag, "styleId")?.unwrap_or_default(),
        is_default: attribute_value(tag, "default")?
            .as_deref()
            .is_some_and(parse_on_off),
        ..WordStyle::default()
    }))
}

fn apply_word_style_tag(
    name: &str,
    tag: &BytesStart<'_>,
    current_style: &mut Option<WordStyle>,
    defaults: &mut WordStyleProperties,
    in_doc_defaults: bool,
    scope: WordPropertyScope,
) -> Result<(), MarkoffError> {
    match name {
        "name" => {
            if let Some(style) = current_style.as_mut() {
                style.name = attribute_value(tag, "val")?;
            }
            return Ok(());
        }
        "basedOn" => {
            if let Some(style) = current_style.as_mut() {
                style.based_on = attribute_value(tag, "val")?;
            }
            return Ok(());
        }
        _ => {}
    }

    let properties = if let Some(style) = current_style.as_mut() {
        &mut style.properties
    } else if in_doc_defaults {
        defaults
    } else {
        return Ok(());
    };

    match scope {
        WordPropertyScope::Run => match name {
            "rFonts" => {
                let font_family = attribute_value(tag, "ascii")?
                    .or(attribute_value(tag, "hAnsi")?)
                    .or(attribute_value(tag, "cs")?)
                    .filter(|font| !font.trim().is_empty());
                properties.run.font_theme = if font_family.is_none() {
                    attribute_value(tag, "asciiTheme")?
                        .or(attribute_value(tag, "hAnsiTheme")?)
                        .or(attribute_value(tag, "cstheme")?)
                        .filter(|font| !font.trim().is_empty())
                } else {
                    None
                };
                properties.run.font_family = font_family;
            }
            "sz" => {
                properties.run.font_size_pt = attribute_value(tag, "val")?
                    .and_then(|value| parse_positive(&value))
                    .map(|size| size / 2.0);
            }
            "color" => {
                properties.run.color =
                    attribute_value(tag, "val")?.and_then(|value| parse_rgb(&value));
            }
            "b" => properties.run.bold = Some(word_on_off(tag)?),
            "i" => properties.run.italic = Some(word_on_off(tag)?),
            _ => {}
        },
        WordPropertyScope::Paragraph => match name {
            "spacing" => {
                properties.paragraph.spacing_before_pt = attribute_value(tag, "before")?
                    .and_then(|value| parse_non_negative(&value))
                    .map(|value| value / 20.0);
                properties.paragraph.spacing_after_pt = attribute_value(tag, "after")?
                    .and_then(|value| parse_non_negative(&value))
                    .map(|value| value / 20.0);
                let line_rule = attribute_value(tag, "lineRule")?;
                properties.paragraph.line = attribute_value(tag, "line")?
                    .and_then(|value| parse_positive(&value))
                    .map(|line| (line, line_rule));
            }
            "jc" => {
                properties.paragraph.text_align =
                    attribute_value(tag, "val")?.and_then(|value| parse_word_alignment(&value));
            }
            "ind" => {
                let first_line =
                    attribute_value(tag, "firstLine")?.and_then(|value| parse_non_negative(&value));
                properties.paragraph.first_line_indent_pt =
                    first_line.map_or(Some(0.0), |value| Some(value / 20.0));
            }
            _ => {}
        },
        WordPropertyScope::None => {}
    }
    Ok(())
}

fn word_on_off(tag: &BytesStart<'_>) -> Result<bool, MarkoffError> {
    Ok(attribute_value(tag, "val")?
        .as_deref()
        .is_none_or(parse_on_off))
}

fn parse_on_off(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "off" | "no"
    )
}

fn parse_word_alignment(value: &str) -> Option<StyleTextAlign> {
    match value.to_ascii_lowercase().as_str() {
        "left" | "start" => Some(StyleTextAlign::Left),
        "center" => Some(StyleTextAlign::Center),
        "right" | "end" => Some(StyleTextAlign::Right),
        "both" | "distribute" => Some(StyleTextAlign::Justify),
        _ => None,
    }
}

#[derive(Default)]
struct WordThemeFonts {
    major: Option<String>,
    minor: Option<String>,
}

#[derive(Clone, Copy)]
enum WordThemeFontGroup {
    Major,
    Minor,
}

fn parse_word_theme_fonts(source: &str) -> Result<WordThemeFonts, MarkoffError> {
    let mut reader = Reader::from_str(source);
    let mut fonts = WordThemeFonts::default();
    let mut group = None;
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "majorFont" => group = Some(WordThemeFontGroup::Major),
                "minorFont" => group = Some(WordThemeFontGroup::Minor),
                "latin" => {
                    let typeface =
                        attribute_value(&tag, "typeface")?.filter(|font| !font.trim().is_empty());
                    match group {
                        Some(WordThemeFontGroup::Major) => fonts.major = typeface,
                        Some(WordThemeFontGroup::Minor) => fonts.minor = typeface,
                        None => {}
                    }
                }
                _ => {}
            },
            Event::Empty(tag) if tag.local_name().as_ref() == "latin" => {
                let typeface =
                    attribute_value(&tag, "typeface")?.filter(|font| !font.trim().is_empty());
                match group {
                    Some(WordThemeFontGroup::Major) => fonts.major = typeface,
                    Some(WordThemeFontGroup::Minor) => fonts.minor = typeface,
                    None => {}
                }
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                "majorFont" | "minorFont" => group = None,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(fonts)
}

fn resolve_word_font(
    properties: &WordRunProperties,
    theme_fonts: &WordThemeFonts,
) -> Option<String> {
    properties.font_family.clone().or_else(|| {
        properties
            .font_theme
            .as_deref()
            .and_then(|name| match name.to_ascii_lowercase().as_str() {
                value if value.starts_with("major") => theme_fonts.major.clone(),
                value if value.starts_with("minor") => theme_fonts.minor.clone(),
                _ => None,
            })
    })
}

fn apply_word_styles(
    source: &str,
    theme_fonts: &WordThemeFonts,
    theme: &mut StyleThemePreview,
) -> Result<(), MarkoffError> {
    let styles = parse_word_styles(source)?;
    let normal_id = styles
        .styles
        .values()
        .find(|style| style.is_default)
        .or_else(|| {
            styles.styles.values().find(|style| {
                style.id.eq_ignore_ascii_case("Normal")
                    || style
                        .name
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case("Normal"))
            })
        })
        .map(|style| style.id.clone());
    let normal = normal_id
        .as_deref()
        .map(|id| resolve_word_style(&styles, id))
        .transpose()?
        .unwrap_or_else(|| styles.defaults.clone());

    if let Some(font) = resolve_word_font(&normal.run, theme_fonts) {
        theme.font_family = font;
    }
    if let Some(size) = normal.run.font_size_pt {
        theme.font_size_pt = size;
    }
    if let Some(color) = normal.run.color {
        theme.text_color = color;
    }
    if let Some(spacing) = normal.paragraph.spacing_before_pt {
        theme.paragraph_spacing_before_pt = spacing;
    }
    if let Some(spacing) = normal.paragraph.spacing_after_pt {
        theme.paragraph_spacing_after_pt = spacing;
    }
    if let Some(line) = normal.paragraph.line
        && let Some(height) = word_line_height(line, theme.font_size_pt)
    {
        theme.line_height = height;
    }
    if let Some(align) = normal.paragraph.text_align {
        theme.text_align = align;
    }
    if let Some(indent) = normal.paragraph.first_line_indent_pt {
        theme.first_line_indent_pt = indent;
    }
    theme.heading_font_family.clone_from(&theme.font_family);

    let mut headings = Vec::new();
    for level in 1..=6 {
        let expected = format!("heading{level}");
        let style_id = styles
            .styles
            .values()
            .find(|style| {
                normalize_word_style_name(&style.id) == expected
                    || style
                        .name
                        .as_deref()
                        .is_some_and(|name| normalize_word_style_name(name) == expected)
            })
            .map(|style| style.id.as_str());
        headings.push(
            style_id
                .map(|id| resolve_word_style(&styles, id))
                .transpose()?,
        );
    }
    if let Some(Some(first)) = headings.first() {
        if let Some(font) = resolve_word_font(&first.run, theme_fonts) {
            theme.heading_font_family = font;
        }
        if let Some(color) = first.run.color {
            theme.heading_color = color;
        }
        if let Some(spacing) = first.paragraph.spacing_before_pt {
            theme.heading_spacing_before_pt = spacing;
        }
        if let Some(spacing) = first.paragraph.spacing_after_pt {
            theme.heading_spacing_after_pt = spacing;
        }
        if let Some(bold) = first.run.bold {
            theme.heading_bold = bold;
        }
        if let Some(italic) = first.run.italic {
            theme.heading_italic = italic;
        }
    }
    for (index, heading) in headings.into_iter().enumerate() {
        if let Some(heading) = heading {
            if let Some(size) = heading.run.font_size_pt {
                theme.heading_sizes_pt[index] = size;
            }
            if let Some(color) = heading.run.color {
                theme.heading_colors[index] = color;
            } else {
                theme.heading_colors[index] = theme.heading_color;
            }
        }
    }
    Ok(())
}

fn resolve_word_style(styles: &WordStyles, id: &str) -> Result<WordStyleProperties, MarkoffError> {
    fn resolve(
        styles: &WordStyles,
        id: &str,
        visited: &mut HashSet<String>,
    ) -> Result<WordStyleProperties, MarkoffError> {
        if !visited.insert(id.to_string()) {
            return Err(invalid_data(std::io::Error::other(
                "cyclic Word paragraph style inheritance",
            ))
            .into());
        }
        let mut resolved = styles.defaults.clone();
        if let Some(style) = styles.styles.get(id) {
            if let Some(parent) = style.based_on.as_deref() {
                resolved = resolve(styles, parent, visited)?;
            }
            merge_word_properties(&mut resolved, &style.properties);
        }
        visited.remove(id);
        Ok(resolved)
    }

    resolve(styles, id, &mut HashSet::new())
}

fn merge_word_properties(target: &mut WordStyleProperties, source: &WordStyleProperties) {
    if source.run.font_family.is_some() {
        target.run.font_family.clone_from(&source.run.font_family);
        target.run.font_theme = None;
    } else if source.run.font_theme.is_some() {
        target.run.font_theme.clone_from(&source.run.font_theme);
        target.run.font_family = None;
    }
    if source.run.font_size_pt.is_some() {
        target.run.font_size_pt = source.run.font_size_pt;
    }
    if source.run.color.is_some() {
        target.run.color = source.run.color;
    }
    if source.run.bold.is_some() {
        target.run.bold = source.run.bold;
    }
    if source.run.italic.is_some() {
        target.run.italic = source.run.italic;
    }
    if source.paragraph.spacing_before_pt.is_some() {
        target.paragraph.spacing_before_pt = source.paragraph.spacing_before_pt;
    }
    if source.paragraph.spacing_after_pt.is_some() {
        target.paragraph.spacing_after_pt = source.paragraph.spacing_after_pt;
    }
    if source.paragraph.line.is_some() {
        target.paragraph.line.clone_from(&source.paragraph.line);
    }
    if source.paragraph.text_align.is_some() {
        target.paragraph.text_align = source.paragraph.text_align;
    }
    if source.paragraph.first_line_indent_pt.is_some() {
        target.paragraph.first_line_indent_pt = source.paragraph.first_line_indent_pt;
    }
}

fn normalize_word_style_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn word_line_height((line, rule): (f32, Option<String>), font_size_pt: f32) -> Option<f32> {
    let height = match rule.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("exact" | "atleast") => line / 20.0 / font_size_pt,
        _ => line / 240.0,
    };
    (height.is_finite() && height > 0.0).then_some(height)
}

fn apply_word_page_settings(
    source: &str,
    theme: &mut StyleThemePreview,
) -> Result<(), MarkoffError> {
    let mut reader = Reader::from_str(source);
    let mut in_section = false;
    let mut section_found = false;
    let mut page_size = None;
    let mut orientation = None;
    let mut margins = [None; 4];

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "sectPr" if !section_found => in_section = true,
                "pgSz" if in_section => {
                    let width =
                        attribute_value(&tag, "w")?.and_then(|value| parse_positive(&value));
                    let height =
                        attribute_value(&tag, "h")?.and_then(|value| parse_positive(&value));
                    let explicit_orientation = attribute_value(&tag, "orient")?;
                    if let (Some(width), Some(height)) = (width, height) {
                        let is_landscape = explicit_orientation
                            .as_deref()
                            .is_some_and(|value| value.eq_ignore_ascii_case("landscape"))
                            || width > height;
                        let (size, orientation_value) =
                            nearest_page_size(width / 20.0, height / 20.0, is_landscape);
                        page_size = Some(size);
                        orientation = Some(orientation_value);
                    }
                }
                "pgMar" if in_section => {
                    for (index, name) in ["top", "right", "bottom", "left"].into_iter().enumerate()
                    {
                        margins[index] = attribute_value(&tag, name)?
                            .and_then(|value| parse_non_negative(&value))
                            .map(|value| value / 20.0);
                    }
                }
                _ => {}
            },
            Event::Empty(tag) if in_section => match tag.local_name().as_ref() {
                "pgSz" => {
                    let width =
                        attribute_value(&tag, "w")?.and_then(|value| parse_positive(&value));
                    let height =
                        attribute_value(&tag, "h")?.and_then(|value| parse_positive(&value));
                    let explicit_orientation = attribute_value(&tag, "orient")?;
                    if let (Some(width), Some(height)) = (width, height) {
                        let is_landscape = explicit_orientation
                            .as_deref()
                            .is_some_and(|value| value.eq_ignore_ascii_case("landscape"))
                            || width > height;
                        let (size, orientation_value) =
                            nearest_page_size(width / 20.0, height / 20.0, is_landscape);
                        page_size = Some(size);
                        orientation = Some(orientation_value);
                    }
                }
                "pgMar" => {
                    for (index, name) in ["top", "right", "bottom", "left"].into_iter().enumerate()
                    {
                        margins[index] = attribute_value(&tag, name)?
                            .and_then(|value| parse_non_negative(&value))
                            .map(|value| value / 20.0);
                    }
                }
                _ => {}
            },
            Event::End(tag) if tag.local_name().as_ref() == "sectPr" && in_section => {
                section_found = true;
                in_section = false;
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if let Some(size) = page_size {
        theme.page_size = size;
    }
    if let Some(page_orientation) = orientation {
        theme.page_orientation = page_orientation;
    }
    let original_margins = [
        theme.margin_top_pt,
        theme.margin_right_pt,
        theme.margin_bottom_pt,
        theme.margin_left_pt,
    ];
    for (target, value) in [
        &mut theme.margin_top_pt,
        &mut theme.margin_right_pt,
        &mut theme.margin_bottom_pt,
        &mut theme.margin_left_pt,
    ]
    .into_iter()
    .zip(margins)
    {
        if let Some(value) = value {
            *target = value;
        }
    }
    let (width, height) = theme.page_dimensions_pt();
    if theme.margin_left_pt + theme.margin_right_pt >= width - 72.0
        || theme.margin_top_pt + theme.margin_bottom_pt >= height - 72.0
    {
        [
            theme.margin_top_pt,
            theme.margin_right_pt,
            theme.margin_bottom_pt,
            theme.margin_left_pt,
        ] = original_margins;
    }
    Ok(())
}

fn nearest_page_size(
    width_pt: f32,
    height_pt: f32,
    landscape: bool,
) -> (StylePageSize, StylePageOrientation) {
    let (short, long) = if width_pt <= height_pt {
        (width_pt, height_pt)
    } else {
        (height_pt, width_pt)
    };
    let sizes = [
        (StylePageSize::A3, 841.89_f32, 1190.55_f32),
        (StylePageSize::A4, 595.28_f32, 841.89_f32),
        (StylePageSize::A5, 419.53_f32, 595.28_f32),
        (StylePageSize::Letter, 612.0_f32, 792.0_f32),
        (StylePageSize::Legal, 612.0_f32, 1008.0_f32),
    ];
    let size = sizes
        .into_iter()
        .min_by(|left, right| {
            let left_error = (short - left.1).abs() + (long - left.2).abs();
            let right_error = (short - right.1).abs() + (long - right.2).abs();
            left_error.total_cmp(&right_error)
        })
        .map_or(StylePageSize::A4, |(size, _, _)| size);
    (
        size,
        if landscape {
            StylePageOrientation::Landscape
        } else {
            StylePageOrientation::Portrait
        },
    )
}

#[derive(Default, Clone)]
struct XlsxFont {
    font_family: Option<String>,
    font_size_pt: Option<f32>,
    color: Option<StyleColor>,
    bold: Option<bool>,
    italic: Option<bool>,
}

#[derive(Default, Clone)]
struct XlsxFill {
    color: Option<StyleColor>,
}

#[derive(Default, Clone)]
struct XlsxBorder {
    color: Option<StyleColor>,
    width_pt: Option<f32>,
}

#[derive(Default, Clone)]
struct XlsxCellFormat {
    font_id: usize,
    fill_id: usize,
    border_id: usize,
    text_align: Option<StyleTextAlign>,
}

#[derive(Default)]
struct XlsxStyles {
    fonts: Vec<XlsxFont>,
    fills: Vec<XlsxFill>,
    borders: Vec<XlsxBorder>,
    cell_formats: Vec<XlsxCellFormat>,
}

impl XlsxStyles {
    fn with_defaults() -> Self {
        Self {
            fonts: vec![XlsxFont::default()],
            fills: vec![XlsxFill::default()],
            borders: vec![XlsxBorder::default()],
            cell_formats: vec![XlsxCellFormat::default()],
        }
    }

    fn ensure_defaults(&mut self) {
        if self.fonts.is_empty() {
            self.fonts.push(XlsxFont::default());
        }
        if self.fills.is_empty() {
            self.fills.push(XlsxFill::default());
        }
        if self.borders.is_empty() {
            self.borders.push(XlsxBorder::default());
        }
        if self.cell_formats.is_empty() {
            self.cell_formats.push(XlsxCellFormat::default());
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum XlsxStyleSection {
    Fonts,
    Fills,
    Borders,
    CellXfs,
}

#[derive(Default)]
struct XlsxStyleParser {
    styles: XlsxStyles,
    section: Option<XlsxStyleSection>,
    theme_colors: [Option<StyleColor>; 12],
    font: Option<XlsxFont>,
    fill: Option<XlsxFill>,
    fill_is_solid: bool,
    border: Option<XlsxBorder>,
    border_side_active: bool,
    cell_format: Option<XlsxCellFormat>,
}

impl XlsxStyleParser {
    fn start(&mut self, name: &str, tag: &BytesStart<'_>) -> Result<(), MarkoffError> {
        match name {
            "fonts" => self.section = Some(XlsxStyleSection::Fonts),
            "fills" => self.section = Some(XlsxStyleSection::Fills),
            "borders" => self.section = Some(XlsxStyleSection::Borders),
            "cellXfs" => self.section = Some(XlsxStyleSection::CellXfs),
            "font" if self.section == Some(XlsxStyleSection::Fonts) => {
                self.font = Some(XlsxFont::default());
            }
            "fill" if self.section == Some(XlsxStyleSection::Fills) => {
                self.fill = Some(XlsxFill::default());
                self.fill_is_solid = false;
            }
            "patternFill" if self.section == Some(XlsxStyleSection::Fills) => {
                self.fill_is_solid = attribute_value(tag, "patternType")?
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case("solid"));
            }
            "border" if self.section == Some(XlsxStyleSection::Borders) => {
                self.border = Some(XlsxBorder::default());
            }
            "left" | "right" | "top" | "bottom" | "diagonal"
                if self.section == Some(XlsxStyleSection::Borders) && self.border.is_some() =>
            {
                self.border_side_active = attribute_value(tag, "style")?
                    .as_deref()
                    .is_some_and(|style| !style.eq_ignore_ascii_case("none"));
                if self.border_side_active
                    && let Some(border) = self.border.as_mut()
                {
                    border.width_pt = border_width_for_xlsx(
                        attribute_value(tag, "style")?
                            .as_deref()
                            .unwrap_or_default(),
                    );
                }
            }
            "xf" if self.section == Some(XlsxStyleSection::CellXfs) => {
                self.cell_format = Some(XlsxCellFormat {
                    font_id: xml_index_attribute(tag, "fontId")?,
                    fill_id: xml_index_attribute(tag, "fillId")?,
                    border_id: xml_index_attribute(tag, "borderId")?,
                    ..XlsxCellFormat::default()
                });
            }
            "name" if self.font.is_some() => {
                if let Some(font) = self.font.as_mut() {
                    font.font_family = attribute_value(tag, "val")?;
                }
            }
            "sz" if self.font.is_some() => {
                if let Some(font) = self.font.as_mut() {
                    font.font_size_pt =
                        attribute_value(tag, "val")?.and_then(|value| parse_positive(&value));
                }
            }
            "color" if self.font.is_some() => {
                if let Some(font) = self.font.as_mut() {
                    font.color = parse_xlsx_color(tag, &self.theme_colors)?;
                }
            }
            "b" if self.font.is_some() => {
                if let Some(font) = self.font.as_mut() {
                    font.bold = Some(xlsx_on_off(tag)?);
                }
            }
            "i" if self.font.is_some() => {
                if let Some(font) = self.font.as_mut() {
                    font.italic = Some(xlsx_on_off(tag)?);
                }
            }
            "fgColor" | "bgColor" if self.fill.is_some() && self.fill_is_solid => {
                if let Some(fill) = self.fill.as_mut()
                    && fill.color.is_none()
                {
                    fill.color = parse_xlsx_color(tag, &self.theme_colors)?;
                }
            }
            "color" if self.border_side_active => {
                if let Some(border) = self.border.as_mut()
                    && border.color.is_none()
                {
                    border.color = parse_xlsx_color(tag, &self.theme_colors)?;
                }
            }
            "alignment" if self.cell_format.is_some() => {
                if let Some(cell_format) = self.cell_format.as_mut() {
                    cell_format.text_align = attribute_value(tag, "horizontal")?
                        .and_then(|value| parse_xlsx_alignment(&value));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn end(&mut self, name: &str) {
        match name {
            "font" if self.section == Some(XlsxStyleSection::Fonts) => {
                if let Some(font) = self.font.take() {
                    self.styles.fonts.push(font);
                }
            }
            "fill" if self.section == Some(XlsxStyleSection::Fills) => {
                if let Some(mut fill) = self.fill.take() {
                    if !self.fill_is_solid {
                        fill.color = None;
                    }
                    self.styles.fills.push(fill);
                }
                self.fill_is_solid = false;
            }
            "border" if self.section == Some(XlsxStyleSection::Borders) => {
                if let Some(border) = self.border.take() {
                    self.styles.borders.push(border);
                }
                self.border_side_active = false;
            }
            "left" | "right" | "top" | "bottom" | "diagonal" => {
                self.border_side_active = false;
            }
            "xf" if self.section == Some(XlsxStyleSection::CellXfs) => {
                if let Some(cell_format) = self.cell_format.take() {
                    self.styles.cell_formats.push(cell_format);
                }
            }
            "fonts" | "fills" | "borders" | "cellXfs" => self.section = None,
            _ => {}
        }
    }
}

fn parse_xlsx_styles(
    source: &str,
    theme_colors: [Option<StyleColor>; 12],
) -> Result<XlsxStyles, MarkoffError> {
    let mut reader = Reader::from_str(source);
    let mut parser = XlsxStyleParser {
        theme_colors,
        ..XlsxStyleParser::default()
    };
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => parser.start(&local_name(&tag), &tag)?,
            Event::Empty(tag) => {
                let name = local_name(&tag);
                parser.start(&name, &tag)?;
                parser.end(&name);
            }
            Event::End(tag) => parser.end(tag.local_name().as_ref()),
            Event::Eof => break,
            _ => {}
        }
    }
    parser.styles.ensure_defaults();
    Ok(parser.styles)
}

#[derive(Clone)]
struct XlsxEffectiveStyle {
    font: XlsxFont,
    fill: XlsxFill,
    border: XlsxBorder,
    text_align: Option<StyleTextAlign>,
}

fn effective_xlsx_style(
    styles: &XlsxStyles,
    cell_format_id: usize,
) -> Result<XlsxEffectiveStyle, MarkoffError> {
    let cell_format = styles.cell_formats.get(cell_format_id).ok_or_else(|| {
        invalid_data(std::io::Error::other(format!(
            "XLSX cell references missing style index {cell_format_id}"
        )))
    })?;
    let font = styles.fonts.get(cell_format.font_id).ok_or_else(|| {
        invalid_data(std::io::Error::other(format!(
            "XLSX style references missing font index {}",
            cell_format.font_id
        )))
    })?;
    let fill = styles.fills.get(cell_format.fill_id).ok_or_else(|| {
        invalid_data(std::io::Error::other(format!(
            "XLSX style references missing fill index {}",
            cell_format.fill_id
        )))
    })?;
    let border = styles.borders.get(cell_format.border_id).ok_or_else(|| {
        invalid_data(std::io::Error::other(format!(
            "XLSX style references missing border index {}",
            cell_format.border_id
        )))
    })?;
    Ok(XlsxEffectiveStyle {
        font: font.clone(),
        fill: fill.clone(),
        border: border.clone(),
        text_align: cell_format.text_align,
    })
}

fn parse_xlsx_color(
    tag: &BytesStart<'_>,
    theme_colors: &[Option<StyleColor>; 12],
) -> Result<Option<StyleColor>, MarkoffError> {
    let rgb = attribute_value(tag, "rgb")?.and_then(|value| parse_rgb(&value));
    let theme_index = attribute_value(tag, "theme")?
        .map(|value| parse_xml_index(&value))
        .transpose()?;
    let indexed = attribute_value(tag, "indexed")?
        .map(|value| parse_xml_index(&value))
        .transpose()?;
    let color = rgb
        .or_else(|| theme_index.and_then(|index| theme_colors.get(index).copied().flatten()))
        .or_else(|| indexed_excel_color(indexed.unwrap_or(usize::MAX)));
    let tint = attribute_value(tag, "tint")?
        .map(|value| value.parse::<f32>().map_err(invalid_data))
        .transpose()?
        .unwrap_or(0.0);
    if !tint.is_finite() || !(-1.0..=1.0).contains(&tint) {
        return Err(invalid_data(std::io::Error::other(
            "XLSX color tint must be between -1 and 1",
        ))
        .into());
    }
    Ok(color.map(|color| apply_color_tint(color, tint)))
}

fn parse_xlsx_theme_colors(source: &str) -> Result<[Option<StyleColor>; 12], MarkoffError> {
    let mut reader = Reader::from_str(source);
    let mut colors = [None; 12];
    let mut current_slot = None;
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => {
                let name = local_name(&tag);
                if let Some(slot) = xlsx_theme_slot(&name) {
                    current_slot = Some(slot);
                } else if let Some(slot) = current_slot
                    && matches!(name.as_str(), "srgbClr" | "sysClr")
                {
                    let value = if name == "sysClr" {
                        attribute_value(&tag, "lastClr")?
                    } else {
                        attribute_value(&tag, "val")?
                    };
                    colors[slot] = value.and_then(|value| parse_rgb(&value));
                }
            }
            Event::Empty(tag) => {
                let name = local_name(&tag);
                if let Some(slot) = xlsx_theme_slot(&name) {
                    current_slot = Some(slot);
                } else if let Some(slot) = current_slot
                    && matches!(name.as_str(), "srgbClr" | "sysClr")
                {
                    let value = if name == "sysClr" {
                        attribute_value(&tag, "lastClr")?
                    } else {
                        attribute_value(&tag, "val")?
                    };
                    colors[slot] = value.and_then(|value| parse_rgb(&value));
                }
            }
            Event::End(tag) => {
                if xlsx_theme_slot(tag.local_name().as_ref()).is_some() {
                    current_slot = None;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(colors)
}

fn xlsx_theme_slot(name: &str) -> Option<usize> {
    match name {
        "lt1" => Some(0),
        "dk1" => Some(1),
        "lt2" => Some(2),
        "dk2" => Some(3),
        "accent1" => Some(4),
        "accent2" => Some(5),
        "accent3" => Some(6),
        "accent4" => Some(7),
        "accent5" => Some(8),
        "accent6" => Some(9),
        "hlink" => Some(10),
        "folHlink" => Some(11),
        _ => None,
    }
}

fn indexed_excel_color(index: usize) -> Option<StyleColor> {
    [
        parse_rgb("000000"),
        parse_rgb("FFFFFF"),
        parse_rgb("FF0000"),
        parse_rgb("00FF00"),
        parse_rgb("0000FF"),
        parse_rgb("FFFF00"),
        parse_rgb("FF00FF"),
        parse_rgb("00FFFF"),
    ]
    .get(index)
    .copied()
    .flatten()
}

fn apply_color_tint(color: StyleColor, tint: f32) -> StyleColor {
    let adjust = |channel: u8| {
        let channel = f32::from(channel);
        let adjusted = if tint < 0.0 {
            channel * (1.0 + tint)
        } else {
            channel + (255.0 - channel) * tint
        };
        adjusted.round().clamp(0.0, 255.0) as u8
    };
    StyleColor {
        red: adjust(color.red),
        green: adjust(color.green),
        blue: adjust(color.blue),
    }
}

fn xlsx_on_off(tag: &BytesStart<'_>) -> Result<bool, MarkoffError> {
    Ok(attribute_value(tag, "val")?
        .as_deref()
        .is_none_or(parse_on_off))
}

fn parse_xlsx_alignment(value: &str) -> Option<StyleTextAlign> {
    match value.to_ascii_lowercase().as_str() {
        "left" | "general" => Some(StyleTextAlign::Left),
        "center" | "centercontinuous" => Some(StyleTextAlign::Center),
        "right" => Some(StyleTextAlign::Right),
        "justify" | "distributed" => Some(StyleTextAlign::Justify),
        _ => None,
    }
}

fn border_width_for_xlsx(style: &str) -> Option<f32> {
    match style.to_ascii_lowercase().as_str() {
        "hair" => Some(0.25),
        "thin" | "dotted" | "dashdot" | "dashed" => Some(0.5),
        "medium" | "mediumdashed" | "mediumdashdot" => Some(1.0),
        "thick" | "double" => Some(1.5),
        _ => None,
    }
}

fn parse_xlsx_sheet_ids(source: &str) -> Result<Vec<String>, MarkoffError> {
    let mut reader = Reader::from_str(source);
    let mut ids = Vec::new();
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == "sheet" => {
                if let Some(id) = attribute_value(&tag, "id")? {
                    ids.push(id);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(ids)
}

fn normalize_zip_target(base: &str, target: &str) -> String {
    let target = target.replace('\\', "/");
    let combined = if target.starts_with('/') {
        target.trim_start_matches('/').to_string()
    } else {
        format!("{base}/{target}")
    };
    let mut parts = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

fn read_first_xlsx_rows(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Result<Vec<Vec<usize>>, MarkoffError> {
    let workbook = read_zip_text_part(archive, "xl/workbook.xml")?;
    let relationships = read_zip_text_part(archive, "xl/_rels/workbook.xml.rels")?;
    let relationship_map = parse_relationships(&relationships)?;
    let sheet_ids = parse_xlsx_sheet_ids(&workbook)?;
    let sheet_id = sheet_ids.first().ok_or_else(|| {
        invalid_data(std::io::Error::other(
            "XLSX workbook contains no worksheets",
        ))
    })?;
    let sheet_target = relationship_map.get(sheet_id).ok_or_else(|| {
        invalid_data(std::io::Error::other(format!(
            "XLSX workbook has no relationship for worksheet {sheet_id}"
        )))
    })?;
    let sheet_path = normalize_zip_target("xl", sheet_target);
    let sheet = read_zip_text_part(archive, &sheet_path)?;
    parse_xlsx_rows(&sheet)
}

fn parse_xlsx_rows(source: &str) -> Result<Vec<Vec<usize>>, MarkoffError> {
    let mut reader = Reader::from_str(source);
    let mut rows = Vec::new();
    let mut in_sheet_data = false;
    let mut row_style = 0usize;
    let mut current_row = None;

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "sheetData" => in_sheet_data = true,
                "row" if in_sheet_data => {
                    row_style = attribute_value(&tag, "s")?
                        .map(|value| parse_xml_index(&value))
                        .transpose()?
                        .unwrap_or(0);
                    current_row = Some(Vec::new());
                }
                "c" if current_row.is_some() => {
                    current_row
                        .as_mut()
                        .unwrap()
                        .push(cell_style_id(&tag, row_style)?);
                }
                _ => {}
            },
            Event::Empty(tag) => match tag.local_name().as_ref() {
                "c" if current_row.is_some() => {
                    current_row
                        .as_mut()
                        .unwrap()
                        .push(cell_style_id(&tag, row_style)?);
                }
                _ => {}
            },
            Event::End(tag) => match tag.local_name().as_ref() {
                "row" => {
                    if let Some(row) = current_row.take()
                        && !row.is_empty()
                    {
                        rows.push(row);
                        if rows.len() >= 6 {
                            break;
                        }
                    }
                }
                "sheetData" => in_sheet_data = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(rows)
}

fn cell_style_id(tag: &BytesStart<'_>, row_style: usize) -> Result<usize, MarkoffError> {
    attribute_value(tag, "s")?.map_or(Ok(row_style), |value| parse_xml_index(&value))
}

fn parse_xml_index(value: &str) -> Result<usize, MarkoffError> {
    value.parse().map_err(|error| invalid_data(error).into())
}

fn xml_index_attribute(tag: &BytesStart<'_>, name: &str) -> Result<usize, MarkoffError> {
    attribute_value(tag, name)?.map_or(Ok(0), |value| parse_xml_index(&value))
}

fn extract_xlsx_theme(input: &Path) -> Result<StyleThemePreview, MarkoffError> {
    let file = std::fs::File::open(input)?;
    let mut archive = zip::ZipArchive::new(file).map_err(invalid_data)?;
    let theme_colors = read_optional_zip_text_part(&mut archive, "xl/theme/theme1.xml")?
        .map(|source| parse_xlsx_theme_colors(&source))
        .transpose()?
        .unwrap_or([None; 12]);
    let style_source = read_optional_zip_text_part(&mut archive, "xl/styles.xml")?;
    let styles = match style_source.as_deref() {
        Some(source) => parse_xlsx_styles(source, theme_colors)?,
        None => XlsxStyles::with_defaults(),
    };
    let rows = read_first_xlsx_rows(&mut archive)?;
    let mut theme = StyleThemePreview::default();
    apply_xlsx_cell_styles(&styles, &rows, &mut theme)?;
    Ok(theme)
}

fn style_counts(rows: &[Vec<usize>]) -> BTreeMap<usize, usize> {
    let mut counts = BTreeMap::new();
    for style in rows.iter().flatten() {
        *counts.entry(*style).or_default() += 1;
    }
    counts
}

fn dominant_style(counts: &BTreeMap<usize, usize>) -> Option<usize> {
    counts
        .iter()
        .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
        .map(|(style, _)| *style)
}

fn apply_xlsx_cell_styles(
    styles: &XlsxStyles,
    rows: &[Vec<usize>],
    theme: &mut StyleThemePreview,
) -> Result<(), MarkoffError> {
    let body_style_id = if rows.len() > 1 {
        dominant_style(&style_counts(&rows[1..])).unwrap_or(0)
    } else {
        0
    };
    let body = effective_xlsx_style(styles, body_style_id)?;
    if let Some(font) = body.font.font_family {
        theme.font_family = font;
    }
    if let Some(size) = body.font.font_size_pt {
        theme.font_size_pt = size;
        theme.table_font_size_pt = size;
    }
    if let Some(color) = body.font.color {
        theme.text_color = color;
    }
    if let Some(align) = body.text_align {
        theme.text_align = align;
    }
    if let Some(color) = body.border.color {
        theme.table_border_color = color;
    }
    if let Some(width) = body.border.width_pt {
        theme.table_border_width_pt = width;
    }

    let header_style_id = rows.first().and_then(|row| {
        dominant_style(&row.iter().fold(BTreeMap::new(), |mut counts, style| {
            *counts.entry(*style).or_default() += 1;
            counts
        }))
    });
    if let Some(header_style_id) = header_style_id
        && header_style_id != body_style_id
    {
        let header = effective_xlsx_style(styles, header_style_id)?;
        if let Some(color) = header.fill.color {
            theme.table_header_background = color;
        }
        if let Some(color) = header.font.color {
            theme.table_header_color = color;
        }
        if body.border.color.is_none()
            && let Some(color) = header.border.color
        {
            theme.table_border_color = color;
        }
        if body.border.width_pt.is_none()
            && let Some(width) = header.border.width_pt
        {
            theme.table_border_width_pt = width;
        }
    }

    let body_fill = body.fill.color;
    for row in rows.iter().skip(1) {
        let Some(style_id) =
            dominant_style(&row.iter().fold(BTreeMap::new(), |mut counts, style| {
                *counts.entry(*style).or_default() += 1;
                counts
            }))
        else {
            continue;
        };
        if style_id == body_style_id {
            continue;
        }
        if let Some(color) = effective_xlsx_style(styles, style_id)?.fill.color
            && Some(color) != body_fill
        {
            theme.table_stripe_background = Some(color);
            break;
        }
    }
    Ok(())
}

#[derive(Clone, Default)]
struct OdfCellStyle {
    parent: Option<String>,
    font_family: Option<String>,
    font_size_pt: Option<f32>,
    text_color: Option<StyleColor>,
    background: Option<StyleColor>,
    border_color: Option<StyleColor>,
    border_width_pt: Option<f32>,
    text_align: Option<StyleTextAlign>,
}

#[derive(Default)]
struct OdfStyles {
    default: OdfCellStyle,
    named: BTreeMap<String, OdfCellStyle>,
}

#[derive(Clone, Copy)]
enum OdfPropertyScope {
    None,
    Text,
    Cell,
    Paragraph,
}

fn parse_ods_style_definitions(source: &str, styles: &mut OdfStyles) -> Result<(), MarkoffError> {
    let mut reader = Reader::from_str(source);
    let mut current_style: Option<(Option<String>, OdfCellStyle)> = None;
    let mut scope = OdfPropertyScope::None;

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => {
                let name = local_name(&tag);
                match name.as_str() {
                    "style"
                        if attribute_value(&tag, "family")?.as_deref() == Some("table-cell") =>
                    {
                        current_style = Some((
                            attribute_value(&tag, "name")?,
                            OdfCellStyle {
                                parent: attribute_value(&tag, "parent-style-name")?,
                                ..OdfCellStyle::default()
                            },
                        ));
                        scope = OdfPropertyScope::None;
                    }
                    "default-style"
                        if attribute_value(&tag, "family")?.as_deref() == Some("table-cell") =>
                    {
                        current_style = Some((None, OdfCellStyle::default()));
                        scope = OdfPropertyScope::None;
                    }
                    "text-properties" => {
                        scope = OdfPropertyScope::Text;
                        apply_odf_properties(&tag, current_style.as_mut(), scope)?;
                    }
                    "table-cell-properties" => {
                        scope = OdfPropertyScope::Cell;
                        apply_odf_properties(&tag, current_style.as_mut(), scope)?;
                    }
                    "paragraph-properties" => {
                        scope = OdfPropertyScope::Paragraph;
                        apply_odf_properties(&tag, current_style.as_mut(), scope)?;
                    }
                    _ => apply_odf_properties(&tag, current_style.as_mut(), scope)?,
                }
            }
            Event::Empty(tag) => {
                let name = local_name(&tag);
                match name.as_str() {
                    "style"
                        if attribute_value(&tag, "family")?.as_deref() == Some("table-cell") =>
                    {
                        let name = attribute_value(&tag, "name")?;
                        if let Some(name) = name {
                            styles.named.entry(name).or_default().parent =
                                attribute_value(&tag, "parent-style-name")?;
                        }
                    }
                    "default-style"
                        if attribute_value(&tag, "family")?.as_deref() == Some("table-cell") => {}
                    "text-properties" => {
                        apply_odf_properties(&tag, current_style.as_mut(), OdfPropertyScope::Text)?;
                    }
                    "table-cell-properties" => {
                        apply_odf_properties(&tag, current_style.as_mut(), OdfPropertyScope::Cell)?;
                    }
                    "paragraph-properties" => {
                        apply_odf_properties(
                            &tag,
                            current_style.as_mut(),
                            OdfPropertyScope::Paragraph,
                        )?;
                    }
                    _ => apply_odf_properties(&tag, current_style.as_mut(), scope)?,
                }
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                "text-properties" | "table-cell-properties" | "paragraph-properties" => {
                    scope = OdfPropertyScope::None;
                }
                "style" | "default-style" => {
                    if let Some((name, style)) = current_style.take() {
                        match name {
                            Some(name) => {
                                merge_odf_style(styles.named.entry(name).or_default(), style)
                            }
                            None => merge_odf_style(&mut styles.default, style),
                        }
                    }
                    scope = OdfPropertyScope::None;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn apply_odf_properties(
    tag: &BytesStart<'_>,
    current_style: Option<&mut (Option<String>, OdfCellStyle)>,
    scope: OdfPropertyScope,
) -> Result<(), MarkoffError> {
    let Some((_, style)) = current_style else {
        return Ok(());
    };
    match scope {
        OdfPropertyScope::Text => {
            style.font_family =
                attribute_value(tag, "font-name")?.filter(|font| !font.trim().is_empty());
            style.font_size_pt =
                attribute_value(tag, "font-size")?.and_then(|value| parse_points(&value));
            style.text_color = attribute_value(tag, "color")?.and_then(|value| parse_rgb(&value));
        }
        OdfPropertyScope::Cell => {
            style.background =
                attribute_value(tag, "background-color")?.and_then(|value| parse_rgb(&value));
            for border_name in [
                "border",
                "border-left",
                "border-right",
                "border-top",
                "border-bottom",
            ] {
                if let Some(border) = attribute_value(tag, border_name)? {
                    let (color, width) = parse_odf_border(&border);
                    if style.border_color.is_none() {
                        style.border_color = color;
                    }
                    if style.border_width_pt.is_none() {
                        style.border_width_pt = width;
                    }
                }
            }
        }
        OdfPropertyScope::Paragraph => {
            style.text_align =
                attribute_value(tag, "text-align")?.and_then(|value| parse_odf_alignment(&value));
        }
        OdfPropertyScope::None => {}
    }
    Ok(())
}

fn merge_odf_style(target: &mut OdfCellStyle, source: OdfCellStyle) {
    if source.parent.is_some() {
        target.parent.clone_from(&source.parent);
    }
    if source.font_family.is_some() {
        target.font_family.clone_from(&source.font_family);
    }
    if source.font_size_pt.is_some() {
        target.font_size_pt = source.font_size_pt;
    }
    if source.text_color.is_some() {
        target.text_color = source.text_color;
    }
    if source.background.is_some() {
        target.background = source.background;
    }
    if source.border_color.is_some() {
        target.border_color = source.border_color;
    }
    if source.border_width_pt.is_some() {
        target.border_width_pt = source.border_width_pt;
    }
    if source.text_align.is_some() {
        target.text_align = source.text_align;
    }
}

fn resolve_odf_style(styles: &OdfStyles, name: &str) -> Result<OdfCellStyle, MarkoffError> {
    fn resolve(
        styles: &OdfStyles,
        name: &str,
        visited: &mut HashSet<String>,
    ) -> Result<OdfCellStyle, MarkoffError> {
        if !visited.insert(name.to_string()) {
            return Err(invalid_data(std::io::Error::other(
                "cyclic OpenDocument cell style inheritance",
            ))
            .into());
        }
        let mut resolved = styles.default.clone();
        if let Some(style) = styles.named.get(name) {
            if let Some(parent) = style.parent.as_deref() {
                resolved = resolve(styles, parent, visited)?;
            }
            merge_odf_style(&mut resolved, style.clone());
        }
        visited.remove(name);
        Ok(resolved)
    }

    resolve(styles, name, &mut HashSet::new())
}

fn parse_ods_rows(source: &str) -> Result<Vec<BTreeMap<String, usize>>, MarkoffError> {
    let mut reader = Reader::from_str(source);
    let mut rows = Vec::new();
    let mut in_table = false;
    let mut current_row = None;

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "table" if !in_table => in_table = true,
                "table-row" if in_table => current_row = Some(BTreeMap::new()),
                "table-cell" | "covered-table-cell" if current_row.is_some() => {
                    add_ods_cell_style(&tag, current_row.as_mut().unwrap())?;
                }
                _ => {}
            },
            Event::Empty(tag) => match tag.local_name().as_ref() {
                "table-cell" | "covered-table-cell" if current_row.is_some() => {
                    add_ods_cell_style(&tag, current_row.as_mut().unwrap())?;
                }
                _ => {}
            },
            Event::End(tag) => match tag.local_name().as_ref() {
                "table-row" => {
                    if let Some(row) = current_row.take()
                        && !row.is_empty()
                    {
                        rows.push(row);
                        if rows.len() >= 6 {
                            break;
                        }
                    }
                }
                "table" if in_table => in_table = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(rows)
}

fn add_ods_cell_style(
    tag: &BytesStart<'_>,
    row: &mut BTreeMap<String, usize>,
) -> Result<(), MarkoffError> {
    let style = attribute_value(tag, "style-name")?.unwrap_or_default();
    let repeat = attribute_value(tag, "number-columns-repeated")?
        .map(|value| parse_xml_index(&value))
        .transpose()?
        .unwrap_or(1)
        .clamp(1, 16_384);
    *row.entry(style).or_default() += repeat;
    Ok(())
}

fn dominant_odf_style(rows: &[BTreeMap<String, usize>]) -> Option<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for row in rows {
        for (style, count) in row {
            *counts.entry(style.clone()).or_default() += count;
        }
    }
    counts
        .iter()
        .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
        .map(|(style, _)| style.clone())
}

fn extract_ods_theme(input: &Path) -> Result<StyleThemePreview, MarkoffError> {
    let file = std::fs::File::open(input)?;
    let mut archive = zip::ZipArchive::new(file).map_err(invalid_data)?;
    let content = read_zip_text_part(&mut archive, "content.xml")?;
    let styles_source = read_optional_zip_text_part(&mut archive, "styles.xml")?;
    let mut styles = OdfStyles::default();
    if let Some(source) = styles_source {
        parse_ods_style_definitions(&source, &mut styles)?;
    }
    parse_ods_style_definitions(&content, &mut styles)?;
    let rows = parse_ods_rows(&content)?;
    let mut theme = StyleThemePreview::default();

    let body_style_name = if rows.len() > 1 {
        dominant_odf_style(&rows[1..])
    } else {
        None
    };
    let body = body_style_name
        .as_deref()
        .map(|name| resolve_odf_style(&styles, name))
        .transpose()?
        .unwrap_or_else(|| styles.default.clone());
    apply_odf_body_style(&body, &mut theme);

    let header_style_name = rows.first().and_then(|row| {
        let counts = row
            .iter()
            .map(|(name, count)| (name.clone(), *count))
            .collect::<BTreeMap<_, _>>();
        dominant_odf_style(&[counts])
    });
    if let Some(header_style_name) = header_style_name
        && Some(&header_style_name) != body_style_name.as_ref()
    {
        let header = resolve_odf_style(&styles, &header_style_name)?;
        if let Some(color) = header.background {
            theme.table_header_background = color;
        }
        if let Some(color) = header.text_color {
            theme.table_header_color = color;
        }
        if body.border_color.is_none()
            && let Some(color) = header.border_color
        {
            theme.table_border_color = color;
        }
        if body.border_width_pt.is_none()
            && let Some(width) = header.border_width_pt
        {
            theme.table_border_width_pt = width;
        }
    }

    let body_background = body.background;
    for row in rows.iter().skip(1) {
        let Some(style_name) = dominant_odf_style(std::slice::from_ref(row)) else {
            continue;
        };
        if Some(&style_name) == body_style_name.as_ref() {
            continue;
        }
        if let Some(color) = resolve_odf_style(&styles, &style_name)?.background
            && Some(color) != body_background
        {
            theme.table_stripe_background = Some(color);
            break;
        }
    }
    Ok(theme)
}

fn extract_pptx_theme(input: &Path) -> Result<StyleThemePreview, MarkoffError> {
    let file = std::fs::File::open(input)?;
    let mut archive = zip::ZipArchive::new(file).map_err(invalid_data)?;
    let Some(source) = read_optional_zip_text_part(&mut archive, "ppt/theme/theme1.xml")? else {
        return Ok(StyleThemePreview::default());
    };
    let colors = parse_xlsx_theme_colors(&source)?;
    let fonts = parse_word_theme_fonts(&source)?;
    let mut theme = StyleThemePreview::default();
    if let Some(font) = fonts.minor {
        theme.font_family = font;
    }
    theme.heading_font_family = fonts.major.unwrap_or_else(|| theme.font_family.clone());
    if let Some(color) = colors[1] {
        theme.text_color = color;
    }
    if let Some(color) = colors[4] {
        theme.heading_color = color;
        theme.table_header_background = color;
    }
    theme.heading_colors =
        std::array::from_fn(|index| colors[index + 4].unwrap_or(theme.heading_color));
    if let Some(color) = colors[0] {
        theme.table_header_color = color;
    }
    if let Some(color) = colors[10] {
        theme.link_color = color;
    }
    Ok(theme)
}

fn apply_odf_body_style(style: &OdfCellStyle, theme: &mut StyleThemePreview) {
    if let Some(font) = style.font_family.clone() {
        theme.font_family = font;
    }
    if let Some(size) = style.font_size_pt {
        theme.font_size_pt = size;
        theme.table_font_size_pt = size;
    }
    if let Some(color) = style.text_color {
        theme.text_color = color;
    }
    if let Some(align) = style.text_align {
        theme.text_align = align;
    }
    if let Some(color) = style.border_color {
        theme.table_border_color = color;
    }
    if let Some(width) = style.border_width_pt {
        theme.table_border_width_pt = width;
    }
}

fn parse_odf_border(value: &str) -> (Option<StyleColor>, Option<f32>) {
    let color = value.split_whitespace().find_map(parse_rgb);
    let width = value.split_whitespace().find_map(parse_points);
    (color, width)
}

fn parse_odf_alignment(value: &str) -> Option<StyleTextAlign> {
    match value.to_ascii_lowercase().as_str() {
        "start" | "left" => Some(StyleTextAlign::Left),
        "center" => Some(StyleTextAlign::Center),
        "end" | "right" => Some(StyleTextAlign::Right),
        "justify" => Some(StyleTextAlign::Justify),
        _ => None,
    }
}

fn parse_points(value: &str) -> Option<f32> {
    let value = value.trim().strip_suffix("pt").unwrap_or(value.trim());
    parse_positive(value)
}

fn parse_positive(value: &str) -> Option<f32> {
    let value = value.parse::<f32>().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

fn parse_non_negative(value: &str) -> Option<f32> {
    let value = value.parse::<f32>().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn parse_rgb(value: &str) -> Option<StyleColor> {
    let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
    let value = match value.len() {
        6 => value,
        8 => &value[2..],
        _ => return None,
    };
    if !value.is_ascii() {
        return None;
    }
    Some(StyleColor {
        red: u8::from_str_radix(&value[0..2], 16).ok()?,
        green: u8::from_str_radix(&value[2..4], 16).ok()?,
        blue: u8::from_str_radix(&value[4..6], 16).ok()?,
    })
}

fn local_name(tag: &BytesStart<'_>) -> String {
    tag.local_name().as_ref().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    fn temp_path(name: &str, extension: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "markoff_style_import_{}_{}_{}.{}",
            std::process::id(),
            name,
            stamp,
            extension
        ))
    }

    fn write_zip(path: &Path, parts: &[(&str, &str)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        for (name, contents) in parts {
            archive
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents.as_bytes()).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn imports_docx_paragraph_heading_and_page_styles() {
        let input = temp_path("word", "docx");
        let output = temp_path("word_theme", "toml");
        let styles = r##"<?xml version="1.0"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi"/><w:sz w:val="22"/><w:color w:val="112233"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:pPr><w:spacing w:before="40" w:after="180"/><w:jc w:val="both"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="360" w:after="120"/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorAscii"/><w:sz w:val="36"/><w:color w:val="445566"/><w:b/></w:rPr></w:style></w:styles>"##;
        let word_theme = r##"<?xml version="1.0"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Aptos Display"/></a:majorFont><a:minorFont><a:latin typeface="Aptos"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"##;
        let document = r##"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Example</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="720" w:right="1080" w:bottom="720" w:left="1080"/></w:sectPr></w:body></w:document>"##;
        write_zip(
            &input,
            &[
                ("word/document.xml", document),
                ("word/styles.xml", styles),
                ("word/theme/theme1.xml", word_theme),
            ],
        );

        write_style_theme_from_document(&input, &output, false).unwrap();
        let theme = crate::load_style_theme_preview(&output).unwrap();
        assert_eq!(theme.font_family, "Aptos");
        assert_eq!(theme.font_size_pt, 11.0);
        assert_eq!(theme.text_color, parse_rgb("112233").unwrap());
        assert_eq!(theme.paragraph_spacing_before_pt, 2.0);
        assert_eq!(theme.paragraph_spacing_after_pt, 9.0);
        assert_eq!(theme.line_height, 1.15);
        assert_eq!(theme.text_align, StyleTextAlign::Justify);
        assert_eq!(theme.heading_font_family, "Aptos Display");
        assert_eq!(theme.heading_size_for(1), 18.0);
        assert_eq!(theme.heading_color_for(1), parse_rgb("445566").unwrap());
        assert_eq!(theme.page_size, StylePageSize::Letter);
        assert_eq!(theme.margin_top_pt, 36.0);
        assert_eq!(theme.margin_left_pt, 54.0);

        std::fs::remove_file(input).ok();
        std::fs::remove_file(output).ok();
    }

    #[test]
    fn imports_xlsx_body_and_header_cell_styles() {
        let input = temp_path("excel", "xlsx");
        let output = temp_path("excel_theme", "toml");
        let styles = r##"<?xml version="1.0"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><color rgb="FF000000"/><name val="Calibri"/></font><font><b/><sz val="10"/><color theme="0"/><name val="Aptos"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="solid"><fgColor theme="4"/></patternFill></fill></fills><borders count="2"><border/><border><left style="thin"><color rgb="FF445566"/></left></border></borders><cellXfs count="2"><xf fontId="0" fillId="0" borderId="0"/><xf fontId="1" fillId="1" borderId="1"/></cellXfs></styleSheet>"##;
        let theme = r##"<?xml version="1.0"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F497D"/></a:dk2><a:lt2><a:srgbClr val="EEECE1"/></a:lt2><a:accent1><a:srgbClr val="1F4E78"/></a:accent1></a:clrScheme></a:themeElements></a:theme>"##;
        let workbook = r##"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"##;
        let relationships = r##"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="worksheet" Target="worksheets/sheet1.xml"/></Relationships>"##;
        let sheet = r##"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" s="1"/><c r="B1" s="1"/></row><row r="2"><c r="A2"/><c r="B2"/></row></sheetData></worksheet>"##;
        write_zip(
            &input,
            &[
                ("xl/styles.xml", styles),
                ("xl/theme/theme1.xml", theme),
                ("xl/workbook.xml", workbook),
                ("xl/_rels/workbook.xml.rels", relationships),
                ("xl/worksheets/sheet1.xml", sheet),
            ],
        );

        write_style_theme_from_document(&input, &output, false).unwrap();
        let theme = crate::load_style_theme_preview(&output).unwrap();
        assert_eq!(theme.font_family, "Calibri");
        assert_eq!(theme.table_font_size_pt, 11.0);
        assert_eq!(theme.table_header_background, parse_rgb("1F4E78").unwrap());
        assert_eq!(theme.table_header_color, parse_rgb("FFFFFF").unwrap());
        assert_eq!(theme.table_border_color, parse_rgb("445566").unwrap());
        assert_eq!(theme.table_border_width_pt, 0.5);

        std::fs::remove_file(input).ok();
        std::fs::remove_file(output).ok();
    }

    #[test]
    fn imports_xlsx_without_a_styles_part_using_theme_defaults() {
        let input = temp_path("unstyled_excel", "xlsx");
        let output = temp_path("unstyled_excel_theme", "toml");
        let workbook = r##"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"##;
        let relationships = r##"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="worksheet" Target="worksheets/sheet1.xml"/></Relationships>"##;
        let sheet = r##"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"/><c r="B1"/></row><row r="2"><c r="A2"/><c r="B2"/></row></sheetData></worksheet>"##;
        write_zip(
            &input,
            &[
                ("xl/workbook.xml", workbook),
                ("xl/_rels/workbook.xml.rels", relationships),
                ("xl/worksheets/sheet1.xml", sheet),
            ],
        );

        write_style_theme_from_document(&input, &output, false).unwrap();
        let theme = crate::load_style_theme_preview(&output).unwrap();
        assert_eq!(theme.font_family, StyleThemePreview::default().font_family);

        std::fs::remove_file(input).ok();
        std::fs::remove_file(output).ok();
    }

    #[test]
    fn imports_ods_cell_styles() {
        let input = temp_path("open_document", "ods");
        let output = temp_path("open_document_theme", "toml");
        let styles = r##"<?xml version="1.0"?><office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"><office:styles><style:default-style style:family="table-cell"><style:text-properties style:font-name="Liberation Sans" fo:font-size="10pt" fo:color="#202020"/></style:default-style><style:style style:name="Header" style:family="table-cell"><style:text-properties fo:font-weight="bold" fo:color="#FFFFFF"/><style:table-cell-properties fo:background-color="#1F4E78" fo:border="0.5pt solid #445566"/></style:style><style:style style:name="Body" style:family="table-cell"/></office:styles></office:document-styles>"##;
        let content = r##"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:automatic-styles/><office:body><office:spreadsheet><table:table table:name="Data"><table:table-row><table:table-cell table:style-name="Header"/><table:table-cell table:style-name="Header"/></table:table-row><table:table-row><table:table-cell table:style-name="Body"/><table:table-cell table:style-name="Body"/></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"##;
        write_zip(&input, &[("styles.xml", styles), ("content.xml", content)]);

        write_style_theme_from_document(&input, &output, false).unwrap();
        let theme = crate::load_style_theme_preview(&output).unwrap();
        assert_eq!(theme.font_family, "Liberation Sans");
        assert_eq!(theme.font_size_pt, 10.0);
        assert_eq!(theme.text_color, parse_rgb("202020").unwrap());
        assert_eq!(theme.table_header_background, parse_rgb("1F4E78").unwrap());
        assert_eq!(theme.table_header_color, parse_rgb("FFFFFF").unwrap());
        assert_eq!(theme.table_border_color, parse_rgb("445566").unwrap());

        std::fs::remove_file(input).ok();
        std::fs::remove_file(output).ok();
    }

    #[test]
    fn imports_pptx_theme_fonts_and_colors() {
        let input = temp_path("presentation", "pptx");
        let output = temp_path("presentation_theme", "toml");
        let theme_xml = r##"<?xml version="1.0"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:srgbClr val="111111"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="222222"/></a:dk2><a:lt2><a:srgbClr val="EEEEEE"/></a:lt2><a:accent1><a:srgbClr val="334455"/></a:accent1><a:accent2><a:srgbClr val="556677"/></a:accent2><a:accent3><a:srgbClr val="778899"/></a:accent3><a:accent4><a:srgbClr val="99AABB"/></a:accent4><a:accent5><a:srgbClr val="AABBCC"/></a:accent5><a:accent6><a:srgbClr val="BBCCDD"/></a:accent6><a:hlink><a:srgbClr val="0000FF"/></a:hlink><a:folHlink><a:srgbClr val="800080"/></a:folHlink></a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Aptos Display"/></a:majorFont><a:minorFont><a:latin typeface="Aptos"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"##;
        assert_eq!(
            parse_xlsx_theme_colors(theme_xml).unwrap()[9],
            Some(parse_rgb("BBCCDD").unwrap())
        );
        write_zip(&input, &[("ppt/theme/theme1.xml", theme_xml)]);

        write_style_theme_from_document(&input, &output, false).unwrap();
        let theme = crate::load_style_theme_preview(&output).unwrap();
        assert_eq!(theme.font_family, "Aptos");
        assert_eq!(theme.heading_font_family, "Aptos Display");
        assert_eq!(theme.text_color, parse_rgb("111111").unwrap());
        assert_eq!(theme.heading_color_for(1), parse_rgb("334455").unwrap());
        assert_eq!(theme.heading_color_for(6), parse_rgb("BBCCDD").unwrap());
        assert_eq!(theme.link_color, parse_rgb("0000FF").unwrap());

        std::fs::remove_file(input).ok();
        std::fs::remove_file(output).ok();
    }

    #[test]
    fn does_not_replace_the_source_document_with_a_theme() {
        let input = temp_path("same_path", "docx");
        write_zip(
            &input,
            &[(
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#,
            )],
        );
        let error = write_style_theme_from_document(&input, &input, true).unwrap_err();
        assert!(error.to_string().contains("same file"));
        std::fs::remove_file(input).ok();
    }
}
