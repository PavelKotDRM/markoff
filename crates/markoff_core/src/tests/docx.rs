use super::unique_temp_path;
use crate::{Format, convert_file};
use std::fs;

#[test]
fn round_trips_markdown_through_docx() {
    let markdown = unique_temp_path("markdown_docx_input");
    let document = unique_temp_path("markdown_docx_document");
    let restored = unique_temp_path("markdown_docx_output");
    fs::write(&markdown, "# Project\n\nA **bold** and *italic* note.\n").unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();
    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&restored).unwrap();
    assert!(rendered.contains("# Project"));
    assert!(rendered.contains("**bold**"));
    assert!(rendered.contains("*italic*"));

    fs::remove_file(markdown).ok();
    fs::remove_file(document).ok();
    fs::remove_file(restored).ok();
}

#[test]
fn round_trips_markdown_lists_through_docx() {
    let markdown = unique_temp_path("markdown_docx_lists_input");
    let document = unique_temp_path("markdown_docx_lists_document");
    let restored = unique_temp_path("markdown_docx_lists_output");
    fs::write(
        &markdown,
        "- First task\n- Second task\n\n1. First step\n2. Second step\n",
    )
    .unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();
    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&restored).unwrap();
    assert!(rendered.contains("- First task"));
    assert!(rendered.contains("- Second task"));
    assert!(rendered.contains("1. First step"));
    assert!(rendered.contains("2. Second step"));

    fs::remove_file(markdown).ok();
    fs::remove_file(document).ok();
    fs::remove_file(restored).ok();
}

#[test]
fn converts_pageref_fields_to_markdown_links() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let document = unique_temp_path("pageref_input");
    let markdown = unique_temp_path("pageref_output");
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:bookmarkStart w:id="0" w:name="_TocTarget"/><w:r><w:t>Target heading</w:t></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGEREF _TocTarget \h </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>3</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#;
    let file = fs::File::create(&document).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("word/document.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(xml.as_bytes()).unwrap();
    archive.finish().unwrap();

    convert_file(&document, &markdown, Format::Docx, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&markdown).unwrap();
    assert!(rendered.contains("<a id=\"_TocTarget\"></a>\n# Target heading"));
    assert!(rendered.contains("[3](#_TocTarget)"));
    assert!(!rendered.contains("PAGEREF"));

    fs::remove_file(document).ok();
    fs::remove_file(markdown).ok();
}
