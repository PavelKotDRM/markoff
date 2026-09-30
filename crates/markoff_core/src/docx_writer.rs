use std::collections::HashMap;
use std::path::Path;

use crate::MarkoffError;
use crate::error::invalid_data;
use crate::tables::parse_markdown_table;
use crate::zip_utils::write_zip_part;

mod blocks;
mod footnotes;
mod hyperlinks;
mod lists;

use blocks::{
    docx_code_block, docx_paragraph, docx_table, is_fenced_code_block_start, is_markdown_table_row,
    is_markdown_table_start, styles_xml,
};
use footnotes::{extract_footnotes, render_footnotes};
use hyperlinks::{HyperlinkAllocator, build_heading_bookmarks};
use lists::{NumberingStyle, parse_markdown_lists};

pub(crate) fn convert_markdown_to_docx(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    use zip::write::SimpleFileOptions;

    let source = std::fs::read_to_string(input)?;
    let (source, footnotes) = extract_footnotes(&source);
    let footnote_ids = footnotes
        .iter()
        .map(|footnote| (footnote.label.as_str(), footnote.id))
        .collect::<HashMap<_, _>>();
    let lines = source.lines().collect::<Vec<_>>();
    let (heading_bookmarks, heading_anchors) = build_heading_bookmarks(&lines);
    let lists = parse_markdown_lists(&source);
    if lists.definitions.iter().any(|list| list.level > 8) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Markdown list nesting exceeds DOCX's nine-level limit",
        )
        .into());
    }
    let mut body = String::new();
    let mut index = 0;
    let mut document_hyperlinks =
        HyperlinkAllocator::new(4, lines.len().saturating_add(1), &heading_anchors);
    while index < lines.len() {
        let line = lines[index];
        if is_markdown_table_start(&lines, index) {
            let start = index;
            while index < lines.len() && is_markdown_table_row(lines[index]) {
                index += 1;
            }
            let rows = parse_markdown_table(&lines[start..index].join("\n"))?;
            body.push_str(&docx_table(&rows, &footnote_ids, &mut document_hyperlinks));
        } else if is_fenced_code_block_start(line) {
            let start = index + 1;
            index += 1;
            while index < lines.len() && !is_fenced_code_block_start(lines[index]) {
                index += 1;
            }
            body.push_str(&docx_code_block(&lines[start..index].join("\n")));
            if index < lines.len() {
                index += 1;
            }
        } else {
            if !line.trim().is_empty() {
                let paragraph_start = index;
                let mut paragraph = line.to_string();
                let list_item = lists.items_by_line.get(paragraph_start).copied().flatten();
                if list_item.is_none() && !is_standalone_block_line(line) {
                    while let Some(next_line) = lines.get(index + 1) {
                        let next_index = index + 1;
                        if next_line.trim().is_empty()
                            || lists
                                .items_by_line
                                .get(next_index)
                                .is_some_and(Option::is_some)
                            || is_standalone_block_line(next_line)
                            || is_markdown_table_start(&lines, next_index)
                        {
                            break;
                        }
                        let hard_break = paragraph.ends_with("  ");
                        if hard_break {
                            paragraph.truncate(paragraph.len() - 2);
                        }
                        paragraph.push(if hard_break { '\n' } else { ' ' });
                        paragraph.push_str(next_line.trim_start());
                        index = next_index;
                    }
                }
                body.push_str(&docx_paragraph(
                    &paragraph,
                    &footnote_ids,
                    list_item,
                    heading_bookmarks
                        .get(paragraph_start)
                        .and_then(Option::as_ref),
                    &mut document_hyperlinks,
                )?);
            }
            index += 1;
        }
    }
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>{body}<w:sectPr/></w:body></w:document>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    let mut content_types = content_types.replace(
        "</Types>",
        "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/></Types>",
    );
    if !footnotes.is_empty() {
        content_types = content_types.replace(
            "</Types>",
            "<Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/></Types>",
        );
    }
    let relationships = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    let mut document_relationships = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/></Relationships>".to_string();
    if !footnotes.is_empty() {
        document_relationships = document_relationships.replace(
            "</Relationships>",
            "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/></Relationships>",
        );
    }
    document_relationships = document_relationships.replace(
        "</Relationships>",
        "<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>",
    );
    document_relationships = document_relationships.replace(
        "</Relationships>",
        &format!(
            "{}</Relationships>",
            document_hyperlinks.relationship_entries()
        ),
    );
    let list_levels = (0..=8)
        .map(|level| format!("<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"•\"/></w:lvl>"))
        .collect::<String>();
    let ordered_list_levels = (0..=8)
        .map(|level| format!("<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%{level_plus_one}.\"/></w:lvl>", level_plus_one = level + 1))
        .collect::<String>();
    let num_entries = lists
        .definitions
        .iter()
        .map(|list| {
            let abstract_id = match list.style {
                NumberingStyle::Bullet => 0,
                NumberingStyle::Decimal => 1,
            };
            let start_override = if list.style == NumberingStyle::Decimal && list.start != 1 {
                format!(
                    "<w:lvlOverride w:ilvl=\"{}\"><w:startOverride w:val=\"{}\"/></w:lvlOverride>",
                    list.level, list.start
                )
            } else {
                String::new()
            };
            format!(
                "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"{abstract_id}\"/>{start_override}</w:num>",
                list.num_id
            )
        })
        .collect::<String>();
    let numbering = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"multilevel\"/>{list_levels}</w:abstractNum><w:abstractNum w:abstractNumId=\"1\"><w:multiLevelType w:val=\"multilevel\"/>{ordered_list_levels}</w:abstractNum>{num_entries}</w:numbering>"
    );
    let styles = styles_xml();

    let file = std::fs::File::create(output)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    let mut footnote_hyperlinks =
        HyperlinkAllocator::new(1, lines.len().saturating_add(1), &heading_anchors);
    let footnotes_xml =
        (!footnotes.is_empty()).then(|| render_footnotes(&footnotes, &mut footnote_hyperlinks));
    let footnote_relationships = footnote_hyperlinks.has_relationships().then(|| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{}</Relationships>",
            footnote_hyperlinks.relationship_entries()
        )
    });
    let mut parts = vec![
        ("[Content_Types].xml", content_types.as_str()),
        ("_rels/.rels", relationships),
        (
            "word/_rels/document.xml.rels",
            document_relationships.as_str(),
        ),
        ("word/document.xml", document.as_str()),
        ("word/numbering.xml", numbering.as_str()),
        ("word/styles.xml", styles),
    ];
    if let Some(footnotes_xml) = &footnotes_xml {
        parts.push(("word/footnotes.xml", footnotes_xml.as_str()));
    }
    if let Some(footnote_relationships) = &footnote_relationships {
        parts.push((
            "word/_rels/footnotes.xml.rels",
            footnote_relationships.as_str(),
        ));
    }
    for (name, content) in parts {
        write_zip_part(&mut archive, options, name, content)?;
    }
    archive.finish().map_err(invalid_data)?;
    Ok(())
}

fn is_standalone_block_line(line: &str) -> bool {
    let line = line.trim_start();
    let heading_level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    (heading_level > 0 && line.as_bytes().get(heading_level) == Some(&b' '))
        || line.starts_with('>')
        || line == "---"
        || is_fenced_code_block_start(line)
}
