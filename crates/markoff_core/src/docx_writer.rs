use std::collections::HashMap;
use std::path::Path;

use crate::MarkoffError;
use crate::docx_inline::{
    DocxHyperlink, markdown_code_block_to_docx_runs, markdown_inline_to_docx_runs_with_links,
    markdown_list_item,
};
use crate::docx_markdown::heading_anchor;
use crate::error::invalid_data;
use crate::tables::parse_markdown_table;
use crate::xml_utils::xml_attribute_escape;
use crate::zip_utils::write_zip_part;

#[derive(Default)]
struct HyperlinkAllocator {
    next_id: u32,
    relationships: Vec<(String, String)>,
    anchors: HashMap<String, String>,
}

impl HyperlinkAllocator {
    fn new(first_id: u32, anchors: &HashMap<String, String>) -> Self {
        Self {
            next_id: first_id,
            relationships: Vec::new(),
            anchors: anchors.clone(),
        }
    }

    fn resolve(&mut self, destination: &str) -> Option<DocxHyperlink> {
        if destination.is_empty() {
            return None;
        }
        if let Some(anchor) = destination.strip_prefix('#') {
            return self
                .anchors
                .get(anchor)
                .cloned()
                .or_else(|| (!anchor.is_empty()).then(|| anchor.to_string()))
                .map(DocxHyperlink::Anchor);
        }

        let id = format!("rId{}", self.next_id);
        self.next_id += 1;
        self.relationships
            .push((id.clone(), destination.to_string()));
        Some(DocxHyperlink::Relationship(id))
    }

    fn relationship_entries(&self) -> String {
        self.relationships
            .iter()
            .map(|(id, target)| {
                format!(
                    "<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
                    xml_attribute_escape(id),
                    xml_attribute_escape(target)
                )
            })
            .collect()
    }

    fn has_relationships(&self) -> bool {
        !self.relationships.is_empty()
    }
}

struct Bookmark {
    id: usize,
    name: String,
}

fn build_heading_bookmarks(lines: &[&str]) -> (Vec<Option<Bookmark>>, HashMap<String, String>) {
    let mut used_anchors = HashMap::new();
    let mut bookmarks = Vec::with_capacity(lines.len());
    let mut anchors = HashMap::new();

    for (line_index, line) in lines.iter().enumerate() {
        let Some((_level, content)) = markdown_heading(line) else {
            bookmarks.push(None);
            continue;
        };
        let Some(base_anchor) = heading_anchor(content) else {
            bookmarks.push(None);
            continue;
        };
        let occurrence = used_anchors.entry(base_anchor.clone()).or_insert(0usize);
        let anchor = if *occurrence == 0 {
            base_anchor
        } else {
            format!("{base_anchor}-{}", *occurrence)
        };
        *occurrence += 1;
        let name = if is_word_bookmark_name(&anchor) {
            anchor.clone()
        } else {
            format!("_markoff_{}", line_index + 1)
        };
        anchors.insert(anchor, name.clone());
        bookmarks.push(Some(Bookmark {
            id: line_index + 1,
            name,
        }));
    }

    (bookmarks, anchors)
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&level) || line.as_bytes().get(level) != Some(&b' ') {
        return None;
    }
    Some((level, line[level + 1..].trim()))
}

fn is_word_bookmark_name(name: &str) -> bool {
    name.len() <= 40
        && name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

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
    let mut body = String::new();
    let mut index = 0;
    let mut list_ids = ListIdAllocator::default();
    let mut document_hyperlinks = HyperlinkAllocator::new(4, &heading_anchors);
    while index < lines.len() {
        let line = lines[index];
        if is_markdown_table_start(&lines, index) {
            let start = index;
            while index < lines.len() && is_markdown_table_row(lines[index]) {
                index += 1;
            }
            let rows = parse_markdown_table(&lines[start..index].join("\n"));
            body.push_str(&docx_table(&rows, &footnote_ids, &mut document_hyperlinks));
            list_ids.reset();
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
            list_ids.reset();
        } else {
            if !line.trim().is_empty() {
                let paragraph_start = index;
                let mut paragraph = line.to_string();
                while paragraph.ends_with("  ")
                    && lines
                        .get(index + 1)
                        .is_some_and(|next_line| !next_line.trim().is_empty())
                {
                    paragraph.truncate(paragraph.len() - 2);
                    index += 1;
                    paragraph.push('\n');
                    paragraph.push_str(lines[index]);
                }
                let list_num_id = list_ids.resolve(&paragraph);
                body.push_str(&docx_paragraph(
                    &paragraph,
                    &footnote_ids,
                    list_num_id,
                    heading_bookmarks
                        .get(paragraph_start)
                        .and_then(Option::as_ref),
                    &mut document_hyperlinks,
                ));
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
    let num_entries = list_ids
        .allocated()
        .iter()
        .map(|(num_id, kind)| {
            let abstract_id = match kind {
                NumberingStyle::Bullet => 0,
                NumberingStyle::Decimal => 1,
            };
            format!(
                "<w:num w:numId=\"{num_id}\"><w:abstractNumId w:val=\"{abstract_id}\"/></w:num>"
            )
        })
        .collect::<String>();
    let numbering = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:abstractNum w:abstractNumId=\"0\">{list_levels}</w:abstractNum><w:abstractNum w:abstractNumId=\"1\">{ordered_list_levels}</w:abstractNum>{num_entries}</w:numbering>"
    );
    let styles = styles_xml();

    let file = std::fs::File::create(output)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    let mut footnote_hyperlinks = HyperlinkAllocator::new(1, &heading_anchors);
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

fn is_markdown_table_row(line: &str) -> bool {
    let line = line.trim();
    line.matches('|').count() >= 1
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

fn styles_xml() -> &'static str {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:cs=\"Calibri\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"160\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"240\" w:after=\"120\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"1F4E79\"/><w:sz w:val=\"32\"/><w:szCs w:val=\"32\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"200\" w:after=\"100\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"2F5496\"/><w:sz w:val=\"28\"/><w:szCs w:val=\"28\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"160\" w:after=\"80\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"5B9BD5\"/><w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading4\"><w:name w:val=\"heading 4\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"120\" w:after=\"60\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"5B9BD5\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading5\"><w:name w:val=\"heading 5\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"100\" w:after=\"50\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"5B9BD5\"/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading6\"><w:name w:val=\"heading 6\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"80\" w:after=\"40\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"5B9BD5\"/><w:sz w:val=\"18\"/><w:szCs w:val=\"18\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:ind w:left=\"720\" w:right=\"720\"/></w:pPr><w:rPr><w:i/><w:color w:val=\"666666\"/></w:rPr></w:style><w:style w:type=\"paragraph\" w:styleId=\"CodeBlock\"><w:name w:val=\"Code Block\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:spacing w:before=\"80\" w:after=\"80\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style><w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:uiPriority w:val=\"99\"/><w:unhideWhenUsed/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style></w:styles>"
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NumberingStyle {
    Bullet,
    Decimal,
}

/// Assigns each independent run of list-item lines its own DOCX `numId`, so
/// separate Markdown lists do not share one continuously incrementing Word
/// numbering counter.
#[derive(Default)]
struct ListIdAllocator {
    next_id: u32,
    active_bullet: Option<u32>,
    active_decimal: Option<u32>,
    allocated: Vec<(u32, NumberingStyle)>,
}

impl ListIdAllocator {
    fn resolve(&mut self, line: &str) -> Option<u32> {
        let Some((kind, ..)) = markdown_list_item(line) else {
            self.reset();
            return None;
        };
        if self.next_id == 0 {
            self.next_id = 1;
        }
        let (kind, active) = match kind {
            1 => (NumberingStyle::Bullet, &mut self.active_bullet),
            _ => (NumberingStyle::Decimal, &mut self.active_decimal),
        };
        if let Some(id) = active {
            return Some(*id);
        }
        let id = self.next_id;
        self.next_id += 1;
        *active = Some(id);
        self.allocated.push((id, kind));
        Some(id)
    }

    fn reset(&mut self) {
        self.active_bullet = None;
        self.active_decimal = None;
    }

    fn allocated(&self) -> &[(u32, NumberingStyle)] {
        &self.allocated
    }
}

fn docx_paragraph(
    line: &str,
    footnote_ids: &HashMap<&str, usize>,
    list_num_id: Option<u32>,
    bookmark: Option<&Bookmark>,
    hyperlinks: &mut HyperlinkAllocator,
) -> String {
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
    } else if let Some((_, list_level, content)) = markdown_list_item(line) {
        let numbering_id =
            list_num_id.expect("list item lines are resolved to a numId before rendering");
        (
            format!(
                "<w:pPr><w:numPr><w:ilvl w:val=\"{list_level}\"/><w:numId w:val=\"{numbering_id}\"/></w:numPr></w:pPr>"
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
    format!("<w:p>{style}{content}</w:p>")
}

fn docx_code_block(value: &str) -> String {
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"CodeBlock\"/><w:shd w:val=\"clear\" w:fill=\"F2F2F2\"/><w:ind w:left=\"360\"/></w:pPr>{}</w:p>",
        markdown_code_block_to_docx_runs(value)
    )
}

fn docx_table(
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
                    let shading = (row_index == 0)
                        .then_some("<w:shd w:val=\"clear\" w:fill=\"D9EAF7\"/>")
                        .unwrap_or("");
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
            let header = (row_index == 0)
                .then_some("<w:trPr><w:tblHeader/></w:trPr>")
                .unwrap_or("");
            format!("<w:tr>{header}{cells}</w:tr>")
        })
        .collect::<String>();
    format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblLayout w:type=\"autofit\"/><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B7C9D6\"/></w:tblBorders></w:tblPr><w:tblGrid>{grid}</w:tblGrid>{rows}</w:tbl>"
    )
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
    hyperlinks: &mut HyperlinkAllocator,
) -> String {
    value
        .split('\n')
        .map(|line| markdown_inline_to_docx_runs_without_breaks(line, footnote_ids, hyperlinks))
        .collect::<Vec<_>>()
        .join("<w:r><w:br/></w:r>")
}

fn markdown_inline_to_docx_runs_without_breaks(
    value: &str,
    footnote_ids: &HashMap<&str, usize>,
    hyperlinks: &mut HyperlinkAllocator,
) -> String {
    let mut runs = String::new();
    let mut remaining = value;
    while let Some(start) = remaining.find("[^") {
        let before = &remaining[..start];
        let mut resolve = |destination: &str| hyperlinks.resolve(destination);
        runs.push_str(&markdown_inline_to_docx_runs_with_links(
            before,
            &mut resolve,
        ));
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find(']') else {
            let mut resolve = |destination: &str| hyperlinks.resolve(destination);
            runs.push_str(&markdown_inline_to_docx_runs_with_links(
                &remaining[start..],
                &mut resolve,
            ));
            return runs;
        };
        let label = &after_start[..end];
        if let Some(id) = footnote_ids.get(label) {
            runs.push_str(&format!("<w:r><w:footnoteReference w:id=\"{id}\"/></w:r>"));
        } else {
            let mut resolve = |destination: &str| hyperlinks.resolve(destination);
            runs.push_str(&markdown_inline_to_docx_runs_with_links(
                &remaining[start..start + end + 3],
                &mut resolve,
            ));
        }
        remaining = &after_start[end + 1..];
    }
    let mut resolve = |destination: &str| hyperlinks.resolve(destination);
    runs.push_str(&markdown_inline_to_docx_runs_with_links(
        remaining,
        &mut resolve,
    ));
    runs
}

fn render_footnotes(footnotes: &[Footnote], hyperlinks: &mut HyperlinkAllocator) -> String {
    let entries = footnotes
        .iter()
        .map(|footnote| {
            let paragraphs = footnote
                .paragraphs
                .split(|paragraph| paragraph.is_empty())
                .filter(|paragraph| !paragraph.is_empty())
                .map(|paragraph| {
                    let mut resolve = |destination: &str| hyperlinks.resolve(destination);
                    format!(
                        "<w:p>{}</w:p>",
                        markdown_inline_to_docx_runs_with_links(
                            &paragraph.join("\n"),
                            &mut resolve
                        )
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
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:footnotes xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>{entries}</w:footnotes>"
    )
}
