use std::collections::HashMap;

use super::footnotes::markdown_inline_to_docx_runs_with_footnotes;
use super::hyperlinks::{Bookmark, HyperlinkAllocator};
use super::lists::ListParagraph;
use crate::MarkoffError;
use crate::docx_inline::{markdown_code_block_to_docx_runs, markdown_list_item};
use crate::xml_utils::xml_attribute_escape;

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

pub(super) fn styles_xml() -> &'static str {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:cs=\"Calibri\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"160\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"240\" w:after=\"120\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"1F4E79\"/><w:sz w:val=\"32\"/><w:szCs w:val=\"32\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"200\" w:after=\"100\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"2F5496\"/><w:sz w:val=\"28\"/><w:szCs w:val=\"28\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"160\" w:after=\"80\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"5B9BD5\"/><w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading4\"><w:name w:val=\"heading 4\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"120\" w:after=\"60\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"5B9BD5\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading5\"><w:name w:val=\"heading 5\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"100\" w:after=\"50\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"5B9BD5\"/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading6\"><w:name w:val=\"heading 6\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"80\" w:after=\"40\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"5B9BD5\"/><w:sz w:val=\"18\"/><w:szCs w:val=\"18\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:ind w:left=\"720\" w:right=\"720\"/></w:pPr><w:rPr><w:i/><w:color w:val=\"666666\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"CodeBlock\"><w:name w:val=\"Code Block\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:spacing w:before=\"80\" w:after=\"80\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style><w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:uiPriority w:val=\"99\"/><w:unhideWhenUsed/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style></w:styles>"
}

pub(super) fn docx_paragraph(
    line: &str,
    footnote_ids: &HashMap<&str, usize>,
    list_item: Option<ListParagraph>,
    bookmark: Option<&Bookmark>,
    hyperlinks: &mut HyperlinkAllocator,
) -> Result<String, MarkoffError> {
    let heading_level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    let (style, content) = if line.trim() == "---" {
        ("<w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"auto\"/></w:pBdr></w:pPr>".to_string(), "")
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

pub(super) fn docx_code_block(value: &str) -> String {
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"CodeBlock\"/><w:shd w:val=\"clear\" w:fill=\"F2F2F2\"/><w:ind w:left=\"360\"/></w:pPr>{}</w:p>",
        markdown_code_block_to_docx_runs(value)
    )
}

pub(super) fn docx_table(
    rows: &[Vec<String>],
    footnote_ids: &HashMap<&str, usize>,
    hyperlinks: &mut HyperlinkAllocator,
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
                    let shading = if row_index == 0 {
                        "<w:shd w:val=\"clear\" w:fill=\"D9EAF7\"/>"
                    } else {
                        ""
                    };
                    format!(
                        "<w:tc><w:tcPr><w:tcW w:w=\"2400\" w:type=\"dxa\"/>{shading}</w:tcPr><w:p>{}</w:p></w:tc>",
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
    format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblLayout w:type=\"autofit\"/><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/></w:tblBorders></w:tblPr><w:tblGrid>{grid}</w:tblGrid>{rows}</w:tbl>"
    )
}
