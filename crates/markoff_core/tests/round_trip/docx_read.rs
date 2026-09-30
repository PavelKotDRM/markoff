use super::{remove_files, temporary_path};
use markoff_core::{Format, convert_file};
use std::fs;

#[test]
fn docx_to_markdown_merges_adjacent_runs_with_the_same_formatting() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let document = temporary_path("adjacent_docx_runs", "docx");
    let markdown = temporary_path("adjacent_docx_runs", "md");
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>ПО </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>РАБОТ</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>Е</w:t></w:r></w:p></w:body></w:document>"#;
    let file = fs::File::create(&document).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("word/document.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(xml.as_bytes()).unwrap();
    archive.finish().unwrap();

    convert_file(&document, &markdown, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(fs::read_to_string(&markdown).unwrap(), "**ПО РАБОТЕ**\n");

    remove_files(&[&document, &markdown]);
}

#[test]
fn docx_to_markdown_preserves_tabs_between_a_number_and_text() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let document = temporary_path("docx_tab_separator", "docx");
    let markdown = temporary_path("docx_tab_separator", "md");
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>1.</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>Section title</w:t></w:r></w:p></w:body></w:document>"#;
    let file = fs::File::create(&document).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("word/document.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(xml.as_bytes()).unwrap();
    archive.finish().unwrap();

    convert_file(&document, &markdown, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(fs::read_to_string(&markdown).unwrap(), "1. Section title\n");

    remove_files(&[&document, &markdown]);
}

#[test]
fn docx_to_markdown_converts_multilevel_textual_numbering_to_nested_lists() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let document = temporary_path("docx_textual_nested_numbering", "docx");
    let markdown = temporary_path("docx_textual_nested_numbering", "md");
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>1. Root</w:t></w:r></w:p><w:p><w:r><w:t>1.2. Child</w:t></w:r></w:p><w:p><w:r><w:t>1.2.3. Grandchild</w:t></w:r></w:p><w:p><w:r><w:t>1.2.3.4. Great-grandchild</w:t></w:r></w:p></w:body></w:document>"#;
    let file = fs::File::create(&document).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("word/document.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(xml.as_bytes()).unwrap();
    archive.finish().unwrap();

    convert_file(&document, &markdown, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(
        fs::read_to_string(&markdown).unwrap(),
        "1. Root\n\n    2. Child\n\n        3. Grandchild\n\n            4. Great-grandchild\n"
    );

    remove_files(&[&document, &markdown]);
}

#[test]
fn docx_to_markdown_converts_italic_parenthesized_numbering_to_a_list() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let document = temporary_path("docx_italic_parenthesized_numbering", "docx");
    let markdown = temporary_path("docx_italic_parenthesized_numbering", "md");
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:i/></w:rPr><w:t>1) Site</w:t></w:r></w:p><w:p><w:r><w:rPr><w:i/></w:rPr><w:t>2) Production</w:t></w:r></w:p><w:p><w:r><w:rPr><w:i/></w:rPr><w:t>3) Recruitment</w:t></w:r></w:p></w:body></w:document>"#;
    let file = fs::File::create(&document).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("word/document.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(xml.as_bytes()).unwrap();
    archive.finish().unwrap();

    convert_file(&document, &markdown, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(
        fs::read_to_string(&markdown).unwrap(),
        "1. *Site*\n\n2. *Production*\n\n3. *Recruitment*\n"
    );

    remove_files(&[&document, &markdown]);
}

#[test]
fn docx_to_markdown_keeps_bookmarks_inside_numbered_list_items() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let document = temporary_path("docx_bookmark_in_list", "docx");
    let markdown = temporary_path("docx_bookmark_in_list", "md");
    let document_xml = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="9"/></w:numPr></w:pPr><w:r><w:t>First item</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="9"/></w:numPr></w:pPr><w:bookmarkStart w:id="0" w:name="SecondItem"/><w:r><w:t>Second item</w:t></w:r></w:p></w:body></w:document>"#;
    let numbering_xml = r#"<?xml version="1.0" encoding="UTF-8"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="9"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#;
    let file = fs::File::create(&document).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("word/document.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(document_xml.as_bytes()).unwrap();
    archive
        .start_file("word/numbering.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(numbering_xml.as_bytes()).unwrap();
    archive.finish().unwrap();

    convert_file(&document, &markdown, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(
        fs::read_to_string(&markdown).unwrap(),
        "1. First item\n\n2. <a id=\"SecondItem\"></a>Second item\n"
    );

    remove_files(&[&document, &markdown]);
}

#[test]
fn docx_to_markdown_converts_embedded_tables() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let document = temporary_path("embedded_docx_table", "docx");
    let markdown = temporary_path("embedded_docx_table", "md");
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Score</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>Ada</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>42</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
    let file = fs::File::create(&document).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("word/document.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(xml.as_bytes()).unwrap();
    archive.finish().unwrap();

    convert_file(&document, &markdown, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(
        fs::read_to_string(&markdown).unwrap(),
        "| Name | Score |\n| --- | --- |\n| Ada | 42 |\n"
    );

    remove_files(&[&document, &markdown]);
}

#[test]
fn docx_to_markdown_preserves_list_markers_and_numbering() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let document = temporary_path("docx_numbering", "docx");
    let markdown = temporary_path("docx_numbering", "md");
    let document_xml = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="42"/></w:numPr></w:pPr><w:r><w:t>Bullet item</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="99"/></w:numPr></w:pPr><w:r><w:t>Third item</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="99"/></w:numPr></w:pPr><w:r><w:t>Fourth item</w:t></w:r></w:p></w:body></w:document>"#;
    let numbering_xml = r#"<?xml version="1.0" encoding="UTF-8"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="10"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="20"><w:lvl w:ilvl="0"><w:start w:val="3"/><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="42"><w:abstractNumId w:val="10"/></w:num><w:num w:numId="99"><w:abstractNumId w:val="20"/></w:num></w:numbering>"#;
    let file = fs::File::create(&document).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("word/document.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(document_xml.as_bytes()).unwrap();
    archive
        .start_file("word/numbering.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(numbering_xml.as_bytes()).unwrap();
    archive.finish().unwrap();

    convert_file(&document, &markdown, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(
        fs::read_to_string(&markdown).unwrap(),
        "- Bullet item\n\n3. Third item\n\n4. Fourth item\n"
    );

    remove_files(&[&document, &markdown]);
}
