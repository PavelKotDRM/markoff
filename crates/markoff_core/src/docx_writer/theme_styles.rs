//! Word styles generated from an explicitly selected style theme.

use crate::style::{DocumentTheme, StyleColor, StyleTextAlign};
use crate::xml_utils::xml_attribute_escape;

pub(super) fn points_to_twips(value: f32) -> u32 {
    (value * 20.0).round() as u32
}

pub(super) fn points_to_half_points(value: f32) -> u32 {
    (value * 2.0).round() as u32
}

/// Converts a line width in points to Word's eighth-point border units.
pub(super) fn points_to_border_eighths(value: f32) -> u32 {
    ((value * 8.0).round() as u32).clamp(2, 96)
}

fn docx_alignment(align: StyleTextAlign) -> &'static str {
    match align {
        StyleTextAlign::Left => "left",
        StyleTextAlign::Center => "center",
        StyleTextAlign::Right => "right",
        StyleTextAlign::Justify => "both",
    }
}

fn run_font(font: &str) -> String {
    let font = xml_attribute_escape(font);
    format!("<w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:cs=\"{font}\"/>")
}

fn run_size(size_pt: f32) -> String {
    let size = points_to_half_points(size_pt);
    format!("<w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/>")
}

fn shading(color: Option<StyleColor>) -> String {
    color
        .map(|color| {
            format!(
                "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>",
                color.hex()
            )
        })
        .unwrap_or_default()
}

const CONSOLAS_RUN_FONTS: &str =
    "<w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/>";

/// Replaces the fixed code font of generated runs with a theme character style.
///
/// Runs that already reference a character style (hyperlinks) keep the fixed
/// font because a run can reference only one character style.
pub(super) fn apply_code_character_style(xml: &str, style_id: &str) -> String {
    let mut output = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(position) = rest.find(CONSOLAS_RUN_FONTS) {
        let before = &rest[..position];
        let after = &rest[position + CONSOLAS_RUN_FONTS.len()..];
        let properties_end = after.find("</w:rPr>").unwrap_or(after.len());
        let linked = after[..properties_end].contains("<w:rStyle ");
        match before.rfind("<w:rPr>") {
            Some(start) if !linked => {
                let start = start + "<w:rPr>".len();
                output.push_str(&before[..start]);
                output.push_str(&format!("<w:rStyle w:val=\"{style_id}\"/>"));
                output.push_str(&before[start..]);
            }
            _ => {
                output.push_str(before);
                output.push_str(CONSOLAS_RUN_FONTS);
            }
        }
        rest = after;
    }
    output.push_str(rest);
    output
}

pub(super) fn themed_styles_xml(theme: &DocumentTheme) -> String {
    let justification = docx_alignment(theme.text_align);
    let headings = (1..=6u8)
        .map(|level| {
            format!(
                "<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"{before}\" w:after=\"{after}\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"left\"/><w:outlineLvl w:val=\"{outline}\"/></w:pPr><w:rPr>{font}{bold}{italic}<w:color w:val=\"{color}\"/>{size}</w:rPr></w:style>",
                before = points_to_twips(theme.heading_spacing_before_pt),
                after = points_to_twips(theme.heading_spacing_after_pt),
                outline = level - 1,
                font = run_font(&theme.heading_font_family),
                bold = if theme.heading_bold {
                    "<w:b/>"
                } else {
                    "<w:b w:val=\"0\"/>"
                },
                italic = if theme.heading_italic { "<w:i/>" } else { "" },
                color = theme.heading_color_for(level).hex(),
                size = run_size(theme.heading_size_for(level)),
            )
        })
        .collect::<String>();
    let quote = format!(
        "<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:pBdr><w:left w:val=\"single\" w:sz=\"{border}\" w:space=\"{space}\" w:color=\"{border_color}\"/></w:pBdr>{background}<w:ind w:left=\"{indent}\" w:firstLine=\"0\"/></w:pPr><w:rPr>{italic}<w:color w:val=\"{color}\"/></w:rPr></w:style>",
        border = points_to_border_eighths(theme.quote_border_width_pt),
        space = (theme.quote_indent_pt / 2.0).round().clamp(0.0, 31.0) as u32,
        border_color = theme.quote_border_color.hex(),
        background = shading(theme.quote_background),
        indent = points_to_twips(theme.quote_indent_pt),
        italic = if theme.quote_italic { "<w:i/>" } else { "" },
        color = theme.quote_text_color.hex(),
    );
    let code_run = format!(
        "{}<w:color w:val=\"{}\"/>{}",
        run_font(&theme.code_font_family),
        theme.code_text_color.hex(),
        run_size(theme.code_font_size_pt)
    );
    let code_padding = points_to_twips(theme.code_padding_pt);
    let code = format!(
        "<w:style w:type=\"paragraph\" w:styleId=\"CodeBlock\"><w:name w:val=\"Code Block\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr>{background}<w:spacing w:line=\"240\" w:lineRule=\"auto\"/><w:ind w:left=\"{code_padding}\" w:right=\"{code_padding}\" w:firstLine=\"0\"/><w:jc w:val=\"left\"/></w:pPr><w:rPr>{code_run}</w:rPr></w:style><w:style w:type=\"character\" w:styleId=\"CodeChar\"><w:name w:val=\"Code Char\"/><w:rPr>{code_run}{inline_background}</w:rPr></w:style><w:style w:type=\"character\" w:styleId=\"CodeBlockChar\"><w:name w:val=\"Code Block Char\"/><w:rPr>{code_run}</w:rPr></w:style>",
        background = shading(Some(theme.code_background)),
        inline_background = shading(theme.code_inline_background),
    );
    let hyperlink = format!(
        "<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"{}\"/><w:u w:val=\"{}\"/></w:rPr></w:style>",
        theme.link_color.hex(),
        if theme.link_underline {
            "single"
        } else {
            "none"
        }
    );
    let table = format!(
        "<w:style w:type=\"paragraph\" w:styleId=\"TableText\"><w:name w:val=\"Table Text\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"left\"/></w:pPr><w:rPr>{size}</w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"TableHeader\"><w:name w:val=\"Table Header\"/><w:basedOn w:val=\"TableText\"/><w:rPr><w:b/><w:color w:val=\"{header_color}\"/></w:rPr></w:style>",
        size = run_size(theme.table_font_size_pt),
        header_color = theme.table_header_color.hex(),
    );
    let page_text = run_size(theme.header_footer_font_size_pt);
    let auxiliary = format!(
        "<w:style w:type=\"paragraph\" w:styleId=\"FootnoteText\"><w:name w:val=\"footnote text\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:ind w:firstLine=\"0\"/></w:pPr><w:rPr>{footnote}</w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Header\"><w:name w:val=\"header\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr>{page_text}</w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Footer\"><w:name w:val=\"footer\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr>{page_text}</w:rPr></w:style>",
        footnote = run_size(theme.footnote_font_size_pt),
    );
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr>{body_font}<w:color w:val=\"{body_color}\"/>{body_size}</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before=\"{paragraph_before}\" w:after=\"{paragraph_after}\" w:line=\"{line_height}\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/><w:pPr><w:ind w:firstLine=\"{first_line}\"/><w:jc w:val=\"{justification}\"/></w:pPr></w:style>{headings}{quote}{code}{hyperlink}{table}{auxiliary}</w:styles>",
        body_font = run_font(&theme.font_family),
        body_color = theme.text_color.hex(),
        body_size = run_size(theme.font_size_pt),
        paragraph_before = points_to_twips(theme.paragraph_spacing_before_pt),
        paragraph_after = points_to_twips(theme.paragraph_spacing_after_pt),
        line_height = (theme.line_height * 240.0).round() as u32,
        first_line = points_to_twips(theme.first_line_indent_pt),
    )
}

/// Builds a header or footer part whose `{page}`/`{pages}` placeholders become Word fields.
pub(super) fn page_text_part(root: &str, style_id: &str, text: &str) -> String {
    let runs = crate::style::page_field_segments(text)
        .into_iter()
        .map(|segment| match segment {
            crate::style::PageTextSegment::Text(text) => format!(
                "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                crate::xml_utils::xml_escape(text)
            ),
            crate::style::PageTextSegment::Page => {
                "<w:fldSimple w:instr=\" PAGE \"><w:r><w:t>1</w:t></w:r></w:fldSimple>".to_string()
            }
            crate::style::PageTextSegment::Pages => {
                "<w:fldSimple w:instr=\" NUMPAGES \"><w:r><w:t>1</w:t></w:r></w:fldSimple>"
                    .to_string()
            }
        })
        .collect::<String>();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:{root} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:p><w:pPr><w:pStyle w:val=\"{style_id}\"/></w:pPr>{runs}</w:p></w:{root}>"
    )
}
