use std::collections::HashMap;
use std::path::Path;

use crate::MarkoffError;
use crate::docx_inline::{
    markdown_code_block_to_docx_runs, markdown_inline_to_docx_runs, markdown_list_item,
};
use crate::error::invalid_data;
use crate::tables::parse_markdown_table;

pub(crate) fn convert_markdown_to_docx(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let source = std::fs::read_to_string(input)?;
    let (source, footnotes) = extract_footnotes(&source);
    let footnote_ids = footnotes
        .iter()
        .map(|footnote| (footnote.label.as_str(), footnote.id))
        .collect::<HashMap<_, _>>();
    let lines = source.lines().collect::<Vec<_>>();
    let mut body = String::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if is_markdown_table_start(&lines, index) {
            let start = index;
            while index < lines.len() && is_markdown_table_row(lines[index]) {
                index += 1;
            }
            let rows = parse_markdown_table(&lines[start..index].join("\n"));
            body.push_str(&docx_table(&rows, &footnote_ids));
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
                body.push_str(&docx_paragraph(line, &footnote_ids));
            }
            index += 1;
        }
    }
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr/></w:body></w:document>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    let mut content_types = content_types.replace(
        "</Types>",
        "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/></Types>",
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
    let list_levels = (0..=8)
        .map(|level| format!("<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"•\"/></w:lvl>"))
        .collect::<String>();
    let ordered_list_levels = (0..=8)
        .map(|level| format!("<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%{level_plus_one}.\"/></w:lvl>", level_plus_one = level + 1))
        .collect::<String>();
    let numbering = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:abstractNum w:abstractNumId=\"0\">{list_levels}</w:abstractNum><w:abstractNum w:abstractNumId=\"1\">{ordered_list_levels}</w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num></w:numbering>"
    );

    let file = std::fs::File::create(output)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    archive
        .start_file("[Content_Types].xml", options)
        .map_err(invalid_data)?;
    archive.write_all(content_types.as_bytes())?;
    archive
        .start_file("_rels/.rels", options)
        .map_err(invalid_data)?;
    archive.write_all(relationships.as_bytes())?;
    archive
        .start_file("word/_rels/document.xml.rels", options)
        .map_err(invalid_data)?;
    archive.write_all(document_relationships.as_bytes())?;
    archive
        .start_file("word/document.xml", options)
        .map_err(invalid_data)?;
    archive.write_all(document.as_bytes())?;
    archive
        .start_file("word/numbering.xml", options)
        .map_err(invalid_data)?;
    archive.write_all(numbering.as_bytes())?;
    if !footnotes.is_empty() {
        archive
            .start_file("word/footnotes.xml", options)
            .map_err(invalid_data)?;
        archive.write_all(render_footnotes(&footnotes).as_bytes())?;
    }
    archive.finish().map_err(invalid_data)?;
    Ok(())
}

fn is_markdown_table_row(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('|') && line.ends_with('|')
}

fn is_markdown_table_start(lines: &[&str], index: usize) -> bool {
    is_markdown_table_row(lines[index])
        && lines
            .get(index + 1)
            .is_some_and(|line| is_markdown_table_row(line) && line.contains("---"))
}

fn is_fenced_code_block_start(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

fn docx_paragraph(line: &str, footnote_ids: &HashMap<&str, usize>) -> String {
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
    } else if let Some((numbering_id, list_level, content)) = markdown_list_item(line) {
        (
            format!(
                "<w:pPr><w:numPr><w:ilvl w:val=\"{list_level}\"/><w:numId w:val=\"{numbering_id}\"/></w:numPr></w:pPr>"
            ),
            content,
        )
    } else {
        (String::new(), line)
    };
    format!(
        "<w:p>{style}{}</w:p>",
        markdown_inline_to_docx_runs_with_footnotes(content, footnote_ids)
    )
}

fn docx_code_block(value: &str) -> String {
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"CodeBlock\"/><w:shd w:val=\"clear\" w:fill=\"F2F2F2\"/><w:ind w:left=\"360\"/></w:pPr>{}</w:p>",
        markdown_code_block_to_docx_runs(value)
    )
}

fn docx_table(rows: &[Vec<String>], footnote_ids: &HashMap<&str, usize>) -> String {
    let rows = rows
        .iter()
        .map(|row| {
            let cells = row
                .iter()
                .map(|cell| {
                    format!(
                        "<w:tc><w:p>{}</w:p></w:tc>",
                        markdown_inline_to_docx_runs_with_footnotes(cell, footnote_ids)
                    )
                })
                .collect::<String>();
            format!("<w:tr>{cells}</w:tr>")
        })
        .collect::<String>();
    format!("<w:tbl><w:tblPr/><w:tblGrid/>{rows}</w:tbl>")
}

struct Footnote {
    id: usize,
    label: String,
    paragraphs: Vec<String>,
}

fn extract_footnotes(source: &str) -> (String, Vec<Footnote>) {
    let lines = source.lines().collect::<Vec<_>>();
    let mut footnotes = Vec::new();
    let mut body = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        if let Some((label, content)) = footnote_definition(lines[index]) {
            let mut paragraphs = vec![content.to_string()];
            index += 1;
            while index < lines.len() {
                if let Some(content) = lines[index]
                    .strip_prefix("    ")
                    .or_else(|| lines[index].strip_prefix('\t'))
                {
                    paragraphs.push(content.to_string());
                    index += 1;
                } else if lines[index].trim().is_empty()
                    && lines
                        .get(index + 1)
                        .is_some_and(|line| line.starts_with("    ") || line.starts_with('\t'))
                {
                    paragraphs.push(String::new());
                    index += 1;
                } else {
                    break;
                }
            }
            footnotes.push(Footnote {
                id: footnotes.len() + 1,
                label: label.to_string(),
                paragraphs,
            });
            continue;
        }

        body.push(lines[index].to_string());
        index += 1;
    }

    let mut inline_sequence = 1;
    for line in &mut body {
        *line = replace_inline_footnotes(line, &mut footnotes, &mut inline_sequence);
    }
    (body.join("\n"), footnotes)
}

fn footnote_definition(line: &str) -> Option<(&str, &str)> {
    let remainder = line.strip_prefix("[^")?;
    let (label, content) = remainder.split_once("]:")?;
    (!label.is_empty()).then_some((label, content.trim_start()))
}

fn replace_inline_footnotes(
    line: &str,
    footnotes: &mut Vec<Footnote>,
    inline_sequence: &mut usize,
) -> String {
    let mut rendered = String::new();
    let mut remaining = line;
    while let Some(start) = remaining.find("^[") {
        rendered.push_str(&remaining[..start]);
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find(']') else {
            rendered.push_str(&remaining[start..]);
            return rendered;
        };
        let label = format!("inline-{}", *inline_sequence);
        *inline_sequence += 1;
        footnotes.push(Footnote {
            id: footnotes.len() + 1,
            label: label.clone(),
            paragraphs: vec![after_start[..end].to_string()],
        });
        rendered.push_str("[^");
        rendered.push_str(&label);
        rendered.push(']');
        remaining = &after_start[end + 1..];
    }
    rendered.push_str(remaining);
    rendered
}

fn markdown_inline_to_docx_runs_with_footnotes(
    value: &str,
    footnote_ids: &HashMap<&str, usize>,
) -> String {
    let mut runs = String::new();
    let mut remaining = value;
    while let Some(start) = remaining.find("[^") {
        let before = &remaining[..start];
        runs.push_str(&markdown_inline_to_docx_runs(before));
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find(']') else {
            runs.push_str(&markdown_inline_to_docx_runs(&remaining[start..]));
            return runs;
        };
        let label = &after_start[..end];
        if let Some(id) = footnote_ids.get(label) {
            runs.push_str(&format!("<w:r><w:footnoteReference w:id=\"{id}\"/></w:r>"));
        } else {
            runs.push_str(&markdown_inline_to_docx_runs(
                &remaining[start..start + end + 3],
            ));
        }
        remaining = &after_start[end + 1..];
    }
    runs.push_str(&markdown_inline_to_docx_runs(remaining));
    runs
}

fn render_footnotes(footnotes: &[Footnote]) -> String {
    let entries = footnotes
        .iter()
        .map(|footnote| {
            let paragraphs = footnote
                .paragraphs
                .split(|paragraph| paragraph.is_empty())
                .filter(|paragraph| !paragraph.is_empty())
                .map(|paragraph| {
                    format!(
                        "<w:p>{}</w:p>",
                        markdown_inline_to_docx_runs(&paragraph.join("\n"))
                    )
                })
                .collect::<String>();
            format!(
                "<w:footnote w:id=\"{}\">{paragraphs}</w:footnote>",
                footnote.id
            )
        })
        .collect::<String>();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:footnotes xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>{entries}</w:footnotes>"
    )
}
