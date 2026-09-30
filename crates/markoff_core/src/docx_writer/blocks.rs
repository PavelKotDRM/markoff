use std::collections::HashMap;

use super::footnotes::markdown_inline_to_docx_runs_with_footnotes;
use super::hyperlinks::{Bookmark, HyperlinkAllocator};
use super::lists::ListParagraph;
use crate::MarkoffError;
use crate::docx_inline::{markdown_code_block_to_docx_runs, markdown_list_item};
use crate::style::DocumentTheme;
use crate::xml_utils::xml_attribute_escape;

use super::theme_styles::{
    apply_code_character_style, points_to_border_eighths, points_to_half_points, points_to_twips,
    themed_styles_xml,
};

pub(super) fn is_markdown_table_row(line: &str) -> bool {
    let line = line.trim();
    line.matches('|').count() >= 1
}

pub(super) fn is_markdown_table_start(lines: &[&str], index: usize) -> bool {
    is_markdown_table_row(lines[index])
        && lines
            .get(index + 1)
            .is_some_and(|line| is_markdown_table_row(line) && line.contains("---"))
}

pub(super) fn is_fenced_code_block_start(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

pub(super) fn styles_xml(theme: &DocumentTheme) -> String {
    if theme.enabled {
        return themed_styles_xml(theme);
    }
    let headings = theme
        .heading_sizes_pt
        .iter()
        .enumerate()
        .map(|(index, configured_size)| {
            let default_sizes = [16.0, 14.0, 12.0, 11.0, 10.0, 9.0];
            let default_before = [12.0, 10.0, 8.0, 6.0, 5.0, 4.0];
            let default_after = [6.0, 5.0, 4.0, 3.0, 2.5, 2.0];
            let default_colors = ["1F4E79", "2F5496", "5B9BD5", "5B9BD5", "5B9BD5", "5B9BD5"];
            let size = if theme.enabled {
                *configured_size
            } else {
                default_sizes[index]
            };
            format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"{before}\" w:after=\"{after}\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:cs=\"{font}\"/><w:b/><w:color w:val=\"{color}\"/><w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/></w:rPr></w:style>",
            level = index + 1,
            before = points_to_twips(if theme.enabled { theme.heading_spacing_before_pt } else { default_before[index] }),
            after = points_to_twips(if theme.enabled { theme.heading_spacing_after_pt } else { default_after[index] }),
            font = xml_attribute_escape(&theme.heading_font_family),
            color = if theme.enabled { theme.heading_color.hex() } else { default_colors[index].to_string() },
            size = points_to_half_points(size),
        )
        })
        .collect::<String>();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{body_font}\" w:hAnsi=\"{body_font}\" w:cs=\"{body_font}\"/><w:color w:val=\"{body_color}\"/><w:sz w:val=\"{body_size}\"/><w:szCs w:val=\"{body_size}\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"{paragraph_after}\" w:line=\"{line_height}\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>{headings}<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:ind w:left=\"720\" w:right=\"720\"/></w:pPr><w:rPr><w:i/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"CodeBlock\"><w:name w:val=\"Code Block\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:rPr><w:rFonts w:ascii=\"{code_font}\" w:hAnsi=\"{code_font}\" w:cs=\"{code_font}\"/></w:rPr></w:style><w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style></w:styles>",
        body_font = xml_attribute_escape(&theme.font_family),
        body_color = theme.text_color.hex(),
        body_size = points_to_half_points(theme.font_size_pt),
        paragraph_after = points_to_twips(theme.paragraph_spacing_after_pt),
        line_height = (theme.line_height * 240.0).round() as u32,
        code_font = xml_attribute_escape(&theme.code_font_family),
    )
}

pub(super) fn docx_paragraph(
    line: &str,
    footnote_ids: &HashMap<&str, usize>,
    list_item: Option<ListParagraph>,
    bookmark: Option<&Bookmark>,
    hyperlinks: &mut HyperlinkAllocator,
    theme: &DocumentTheme,
) -> Result<String, MarkoffError> {
    let heading_level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    let (style, content) = if line.trim() == "---" && theme.enabled {
        (
            format!(
                "<w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"{}\" w:space=\"1\" w:color=\"{}\"/></w:pBdr></w:pPr>",
                points_to_border_eighths(theme.rule_width_pt),
                theme.rule_color.hex()
            ),
            "",
        )
    } else if line.trim() == "---" {
        ("<w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"auto\"/></w:pBdr></w:pPr>".to_string(), "")
    } else if let Some(content) = line.strip_prefix("> ")
        && theme.enabled
    {
        (
            "<w:pPr><w:pStyle w:val=\"Quote\"/></w:pPr>".to_string(),
            content,
        )
    } else if let Some(content) = line.strip_prefix("> ") {
        (
            "<w:pPr><w:pStyle w:val=\"Quote\"/><w:ind w:left=\"720\"/></w:pPr>".to_string(),
            content,
        )
    } else if heading_level > 0
        && heading_level <= 6
        && line.as_bytes().get(heading_level) == Some(&b' ')
    {
        (
            format!("<w:pPr><w:pStyle w:val=\"Heading{heading_level}\"/></w:pPr>"),
            &line[heading_level + 1..],
        )
    } else if let Some(list_item) = list_item {
        let Some((_, _, content)) = markdown_list_item(line) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "parsed Markdown list item has no list marker",
            )
            .into());
        };
        (
            format!(
                "<w:pPr><w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{}\"/></w:numPr></w:pPr>",
                list_item.level, list_item.num_id
            ),
            content,
        )
    } else {
        (String::new(), line)
    };
    let runs = markdown_inline_to_docx_runs_with_footnotes(content, footnote_ids, hyperlinks);
    let content = if let Some(bookmark) = bookmark {
        format!(
            "<w:bookmarkStart w:id=\"{}\" w:name=\"{}\"/>{runs}<w:bookmarkEnd w:id=\"{}\"/>",
            bookmark.id,
            xml_attribute_escape(&bookmark.name),
            bookmark.id
        )
    } else {
        runs
    };
    Ok(format!("<w:p>{style}{content}</w:p>"))
}

pub(super) fn docx_code_block(value: &str, theme: &DocumentTheme) -> String {
    if theme.enabled {
        return format!(
            "<w:p><w:pPr><w:pStyle w:val=\"CodeBlock\"/></w:pPr>{}</w:p>",
            apply_code_character_style(&markdown_code_block_to_docx_runs(value), "CodeBlockChar")
        );
    }
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"CodeBlock\"/><w:shd w:val=\"clear\" w:fill=\"{}\"/><w:ind w:left=\"360\"/></w:pPr>{}</w:p>",
        if theme.enabled {
            theme.code_background.hex()
        } else {
            "F2F2F2".to_string()
        },
        markdown_code_block_to_docx_runs(value)
    )
}

pub(super) fn docx_table(
    rows: &[Vec<String>],
    footnote_ids: &HashMap<&str, usize>,
    hyperlinks: &mut HyperlinkAllocator,
    theme: &DocumentTheme,
) -> String {
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if column_count == 0 {
        return String::new();
    }
    let grid = (0..column_count)
        .map(|_| "<w:gridCol w:w=\"2400\"/>")
        .collect::<String>();
    let rows = rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let cells = (0..column_count)
                .map(|column_index| {
                    let cell = row.get(column_index).map_or("", String::as_str);
                    let stripe = theme
                        .table_stripe_background
                        .filter(|_| theme.enabled && row_index >= 2 && row_index % 2 == 0);
                    let shading = if let Some(stripe) = stripe {
                        format!("<w:shd w:val=\"clear\" w:fill=\"{}\"/>", stripe.hex())
                    } else if row_index == 0 {
                        format!(
                            "<w:shd w:val=\"clear\" w:fill=\"{}\"/>",
                            if theme.enabled {
                                theme.table_header_background.hex()
                            } else {
                                "D9EAF7".to_string()
                            }
                        )
                    } else {
                        String::new()
                    };
                    let header_text = if row_index == 0 && theme.enabled {
                        format!(
                            "<w:pPr><w:pStyle w:val=\"TableHeader\"/><w:rPr><w:b/><w:color w:val=\"{}\"/></w:rPr></w:pPr>",
                            theme.table_header_color.hex()
                        )
                    } else if theme.enabled {
                        "<w:pPr><w:pStyle w:val=\"TableText\"/></w:pPr>".to_string()
                    } else {
                        String::new()
                    };
                    format!(
                        "<w:tc><w:tcPr><w:tcW w:w=\"2400\" w:type=\"dxa\"/>{shading}</w:tcPr><w:p>{header_text}{}</w:p></w:tc>",
                        markdown_inline_to_docx_runs_with_footnotes(
                            cell,
                            footnote_ids,
                            hyperlinks
                        )
                    )
                })
                .collect::<String>();
            let header = if row_index == 0 {
                "<w:trPr><w:tblHeader/></w:trPr>"
            } else {
                ""
            };
            format!("<w:tr>{header}{cells}</w:tr>")
        })
        .collect::<String>();
    let (border_size, cell_margins) = if theme.enabled {
        let padding = points_to_twips(theme.table_cell_padding_pt);
        (
            points_to_border_eighths(theme.table_border_width_pt),
            format!(
                "<w:tblCellMar><w:top w:w=\"{padding}\" w:type=\"dxa\"/><w:left w:w=\"{padding}\" w:type=\"dxa\"/><w:bottom w:w=\"{padding}\" w:type=\"dxa\"/><w:right w:w=\"{padding}\" w:type=\"dxa\"/></w:tblCellMar>"
            ),
        )
    } else {
        (4, String::new())
    };
    format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblLayout w:type=\"autofit\"/><w:tblBorders><w:top w:val=\"single\" w:sz=\"{border_size}\" w:space=\"0\" w:color=\"{border}\"/><w:left w:val=\"single\" w:sz=\"{border_size}\" w:space=\"0\" w:color=\"{border}\"/><w:bottom w:val=\"single\" w:sz=\"{border_size}\" w:space=\"0\" w:color=\"{border}\"/><w:right w:val=\"single\" w:sz=\"{border_size}\" w:space=\"0\" w:color=\"{border}\"/><w:insideH w:val=\"single\" w:sz=\"{border_size}\" w:space=\"0\" w:color=\"{border}\"/><w:insideV w:val=\"single\" w:sz=\"{border_size}\" w:space=\"0\" w:color=\"{border}\"/></w:tblBorders>{cell_margins}</w:tblPr><w:tblGrid>{grid}</w:tblGrid>{rows}</w:tbl>",
        border = if theme.enabled {
            theme.table_border_color.hex()
        } else {
            "B7C9D6".to_string()
        },
    )
}
