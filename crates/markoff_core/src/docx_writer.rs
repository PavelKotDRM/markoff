use std::path::Path;

use crate::MarkoffError;
use crate::docx_inline::{markdown_inline_to_docx_runs, markdown_list_item};
use crate::error::invalid_data;

pub(crate) fn convert_markdown_to_docx(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let source = std::fs::read_to_string(input)?;
    let body = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let heading_level = line
                .chars()
                .take_while(|character| *character == '#')
                .count();
            let (style, content) = if heading_level > 0
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
            format!("<w:p>{style}{}</w:p>", markdown_inline_to_docx_runs(content))
        })
        .collect::<String>();
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr/></w:body></w:document>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    let content_types = content_types.replace(
        "</Types>",
        "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/></Types>",
    );
    let relationships = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    let document_relationships = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/></Relationships>";
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
    archive.finish().map_err(invalid_data)?;
    Ok(())
}
