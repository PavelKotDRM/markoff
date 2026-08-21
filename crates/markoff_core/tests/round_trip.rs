use markoff_core::{Format, convert_file};
use proptest::prelude::*;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temporary_path(name: &str, extension: &str) -> PathBuf {
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "markoff_{name}_{}_{}.{}",
        std::process::id(),
        sequence,
        extension
    ))
}

fn remove_files(paths: &[&PathBuf]) {
    for path in paths {
        fs::remove_file(path).ok();
    }
}

#[test]
fn golden_document_round_trip_preserves_core_markdown_content() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("golden/Golden_Word_Test_Document_v2.docx");
    let markdown = temporary_path("golden_document", "md");
    let restored_document = temporary_path("golden_document", "docx");
    let restored_markdown = temporary_path("golden_document_restored", "md");

    convert_file(&source, &markdown, Format::Docx, Format::Markdown).unwrap();
    convert_file(
        &markdown,
        &restored_document,
        Format::Markdown,
        Format::Docx,
    )
    .unwrap();
    convert_file(
        &restored_document,
        &restored_markdown,
        Format::Docx,
        Format::Markdown,
    )
    .unwrap();

    let source_markdown = fs::read_to_string(&markdown).unwrap();
    assert!(
        source_markdown.contains("1. **Heading Level 1 / Заголовок уровня 1 3**"),
        "tab between TOC title and page number should become a space, not merge digits: {source_markdown:?}"
    );
    for expected in [
        "Текст со сноской номер 1.[^1]",
        "Текст со второй сноской.[^2]",
        "Текст с концевой сноской.[^i]",
        "[^1]: Сноска 1: Это тестовая сноска.",
        "[^2]: Сноска 2: Вторая тестовая сноска с ссылкой на https://example.com",
        "[^i]: Концевая сноска 1: Это тестовая концевая сноска.",
        "Встроенная формула: $E = mc^{2}$",
        "Формула в отдельной строке:  \n$$x = (-b \\pm \\sqrt{b2 - 4ac}) / 2a$$",
        "Матрица:  \n$$\\begin{matrix} 1 & 2 \\\\ 3 & 4 \\end{matrix}$$",
        "Дробь:  \n$$\\frac{a + b}{c + d}$$",
        "Интеграл:  \n$$\\int0\\infty e-x dx = 1$$",
    ] {
        assert!(
            source_markdown.contains(expected),
            "missing {expected:?} in golden docx-to-markdown output: {source_markdown:?}"
        );
    }

    let rendered = fs::read_to_string(&restored_markdown).unwrap();
    for expected in [
        "# 1. Heading Level 1 / Заголовок уровня 1",
        "1. **Heading Level 1 / Заголовок уровня 1 3**",
        "**полужирный текст,** *курсив,* ***полужирный курсив,*** <u>подчеркнутый текст,</u> ~~зачеркнутый текст,~~ верхний индекс x$^{2}$, нижний индекс H$_{2}$O",
        "Встроенная формула: $E = mc^{2}$",
        "$$\\begin{matrix} 1 & 2 \\\\ 3 & 4 \\end{matrix}$$",
        "**Полужирный текст.**",
        "***Полужирный курсивный текст.***",
        "И пример имени файла: report_final_v2.docx",
        "- Элемент 1",
        "1. Первый",
        "1. Шаг первый",
        "2. Шаг второй",
        "Inline code: `const answer = 42;`",
        "```\nfunction greet(name) {\n    console.log(\"Hello, \" + name);",
        "Строка с ручным разрывом строки.  \nСледующая строка после soft line break.",
        "| ID | Name | Role | Active |",
        "| Q4 | 160 | 110 | 50 |",
        "Текст со сноской номер 1.[^1]",
        "[^1]: Сноска 1: Это тестовая сноска.",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in golden round trip"
        );
    }

    remove_files(&[&markdown, &restored_document, &restored_markdown]);
}

#[test]
fn markdown_docx_round_trip_preserves_supported_elements() {
    let markdown = temporary_path("docx_input", "md");
    let document = temporary_path("docx_document", "docx");
    let restored = temporary_path("docx_output", "md");
    fs::write(
        &markdown,
        "# Project\n\nA **bold** and *italic* note.\n\n- First task\n- Second task\n\n1. First step\n2. Second step\n",
    )
    .unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();
    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&restored).unwrap();
    for expected in [
        "# Project",
        "**bold**",
        "*italic*",
        "- First task",
        "1. First step",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }

    remove_files(&[&markdown, &document, &restored]);
}

#[test]
fn markdown_docx_round_trip_preserves_all_heading_levels() {
    use std::io::Read;
    use zip::ZipArchive;

    let markdown = temporary_path("heading_levels_input", "md");
    let document = temporary_path("heading_levels_document", "docx");
    let restored = temporary_path("heading_levels_output", "md");
    let source = "# Level 1\n\n## Level 2\n\n### Level 3\n\n#### Level 4\n\n##### Level 5\n\n###### Level 6\n";
    fs::write(&markdown, source).unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();

    let file = fs::File::open(&document).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut document_xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut document_xml)
        .unwrap();
    for level in 1..=6 {
        assert!(
            document_xml.contains(&format!("<w:pStyle w:val=\"Heading{level}\"/>")),
            "missing Heading{level} style in {document_xml:?}"
        );
    }

    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();
    assert_eq!(fs::read_to_string(&restored).unwrap(), source);

    remove_files(&[&markdown, &document, &restored]);
}

#[test]
fn markdown_docx_round_trip_preserves_code_quotes_and_horizontal_rules() {
    let markdown = temporary_path("markdown_blocks_input", "md");
    let document = temporary_path("markdown_blocks_document", "docx");
    let restored = temporary_path("markdown_blocks_output", "md");
    let source = "Use `inline code`.\n\n> Quoted text.\n\n---\n\n```\nlet answer = 42;\nprintln!(\"{answer}\");\n```\n";
    fs::write(&markdown, source).unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();
    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(fs::read_to_string(&restored).unwrap(), source);

    remove_files(&[&markdown, &document, &restored]);
}

#[test]
fn markdown_docx_round_trip_preserves_tables() {
    let markdown = temporary_path("table_docx_input", "md");
    let document = temporary_path("table_docx_document", "docx");
    let restored = temporary_path("table_docx_output", "md");
    fs::write(
        &markdown,
        "| Name | Score |\n| --- | --- |\n| Ada | 42 |\n| Grace | 99 |\n",
    )
    .unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();
    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(
        fs::read_to_string(&restored).unwrap(),
        "| Name | Score |\n| --- | --- |\n| Ada | 42 |\n| Grace | 99 |\n"
    );

    remove_files(&[&markdown, &document, &restored]);
}

#[test]
fn docx_tables_convert_to_all_tabular_formats() {
    let markdown = temporary_path("table_formats_input", "md");
    let document = temporary_path("table_formats_document", "docx");
    let csv = temporary_path("table_formats", "csv");
    let workbook = temporary_path("table_formats", "xlsx");
    let json = temporary_path("table_formats", "json");
    let yaml = temporary_path("table_formats", "yaml");
    let toml = temporary_path("table_formats", "toml");
    fs::write(&markdown, "| Name | Score |\n| --- | --- |\n| Ada | 42 |\n").unwrap();
    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();

    convert_file(&document, &csv, Format::Docx, Format::Csv).unwrap();
    convert_file(&document, &workbook, Format::Docx, Format::Xlsx).unwrap();
    convert_file(&document, &json, Format::Docx, Format::Json).unwrap();
    convert_file(&document, &yaml, Format::Docx, Format::Yaml).unwrap();
    convert_file(&document, &toml, Format::Docx, Format::Toml).unwrap();

    assert!(fs::read_to_string(&csv).unwrap().contains("Ada,42"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&json).unwrap()).unwrap(),
        json!({
            "blocks": [
                {
                    "type": "table",
                    "rows": [["Name", "Score"], ["Ada", "42"]]
                }
            ]
        })
    );
    for output in [&yaml, &toml] {
        let rendered = fs::read_to_string(output).unwrap();
        assert!(
            rendered.contains("Ada"),
            "missing table row in {rendered:?}"
        );
    }

    let restored = temporary_path("table_formats_restored", "md");
    convert_file(&workbook, &restored, Format::Xlsx, Format::Markdown).unwrap();
    assert!(
        fs::read_to_string(&restored)
            .unwrap()
            .contains("| Ada | 42 |")
    );

    remove_files(&[
        &markdown, &document, &csv, &workbook, &json, &yaml, &toml, &restored,
    ]);
}

#[test]
fn markdown_docx_round_trip_preserves_emphasis_and_nested_lists() {
    let markdown = temporary_path("formatted_docx_input", "md");
    let document = temporary_path("formatted_docx_document", "docx");
    let restored = temporary_path("formatted_docx_output", "md");
    fs::write(
        &markdown,
        "__bold__, _italic_, ~~deleted~~, and **bold with *italic***.\n\n- Parent\n    - Child\n\n1. First\n    1. Nested\n",
    )
    .unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();
    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&restored).unwrap();
    for expected in [
        "**bold**",
        "*italic*",
        "~~deleted~~",
        "***italic***",
        "- Parent",
        "    - Child",
        "1. First",
        "    1. Nested",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }

    remove_files(&[&markdown, &document, &restored]);
}

#[test]
fn markdown_docx_round_trip_normalizes_adjacent_bold_runs() {
    let markdown = temporary_path("adjacent_bold_input", "md");
    let document = temporary_path("adjacent_bold_document", "docx");
    let restored = temporary_path("adjacent_bold_output", "md");
    fs::write(&markdown, "**ПО ****РАБОТ****Е**** В СИСТЕМЕ БИТРИКС24**\n").unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();
    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(
        fs::read_to_string(&restored).unwrap(),
        "**ПО РАБОТЕ В СИСТЕМЕ БИТРИКС24**\n"
    );

    remove_files(&[&markdown, &document, &restored]);
}

#[test]
fn markdown_docx_round_trip_preserves_footnotes() {
    let markdown = temporary_path("footnotes_input", "md");
    let document = temporary_path("footnotes_document", "docx");
    let restored = temporary_path("footnotes_output", "md");
    fs::write(
        &markdown,
        "Footnote 1 link[^first].\n\nFootnote 2 link[^second].\n\nInline footnote^[Text of inline footnote] definition.\n\nDuplicated footnote reference[^second].\n\n[^first]: Footnote **can have markup**\n\n    and multiple paragraphs.\n\n[^second]: Footnote text.\n",
    )
    .unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();
    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&restored).unwrap();
    for expected in [
        "Footnote 1 link[^1].",
        "Footnote 2 link[^2].",
        "Inline footnote[^3] definition.",
        "Duplicated footnote reference[^2].",
        "[^1]: Footnote **can have markup**\n\n    and multiple paragraphs.",
        "[^2]: Footnote text.",
        "[^3]: Text of inline footnote",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }

    remove_files(&[&markdown, &document, &restored]);
}

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

#[test]
fn structured_data_xlsx_round_trip_preserves_cell_types() {
    let source = temporary_path("structured_input", "json");
    let workbook = temporary_path("structured_workbook", "xlsx");
    let restored = temporary_path("structured_output", "json");
    let records = json!([
        {"name": "Ada", "score": 42, "active": true},
        {"name": "Grace", "score": -7, "active": false}
    ]);
    fs::write(&source, serde_json::to_string(&records).unwrap()).unwrap();

    convert_file(&source, &workbook, Format::Json, Format::Xlsx).unwrap();
    convert_file(&workbook, &restored, Format::Xlsx, Format::Json).unwrap();

    let converted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&restored).unwrap()).unwrap();
    assert_eq!(converted, records);

    remove_files(&[&source, &workbook, &restored]);
}

#[test]
fn json_xlsx_round_trip_preserves_string_values_that_look_like_scalars() {
    let source = temporary_path("string_scalars_input", "json");
    let workbook = temporary_path("string_scalars_workbook", "xlsx");
    let restored = temporary_path("string_scalars_output", "json");
    let records = json!([{"identifier": "042", "enabled": "true", "score": "3.14"}]);
    fs::write(&source, serde_json::to_string(&records).unwrap()).unwrap();

    convert_file(&source, &workbook, Format::Json, Format::Xlsx).unwrap();
    convert_file(&workbook, &restored, Format::Xlsx, Format::Json).unwrap();

    let converted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&restored).unwrap()).unwrap();
    assert_eq!(converted, records);

    remove_files(&[&source, &workbook, &restored]);
}

#[test]
fn markdown_csv_conversion_escapes_delimited_fields() {
    let markdown = temporary_path("csv_escaping_input", "md");
    let csv = temporary_path("csv_escaping_output", "csv");
    fs::write(
        &markdown,
        "| name | note |\n| --- | --- |\n| Ada | Hello, world |\n",
    )
    .unwrap();

    convert_file(&markdown, &csv, Format::Markdown, Format::Csv).unwrap();

    let mut reader = csv::Reader::from_path(&csv).unwrap();
    let records = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(records[0].get(1), Some("Hello, world"));

    remove_files(&[&markdown, &csv]);
}

proptest! {
    #[test]
    fn json_xlsx_round_trip_preserves_generated_integer_and_boolean_cells(
        rows in prop::collection::vec((any::<i32>(), any::<bool>()), 1..20),
    ) {
        let source = temporary_path("generated_input", "json");
        let workbook = temporary_path("generated_workbook", "xlsx");
        let restored = temporary_path("generated_output", "json");
        let records = serde_json::Value::Array(rows.into_iter().map(|(score, active)| {
            json!({"score": score, "active": active})
        }).collect());
        fs::write(&source, serde_json::to_string(&records).unwrap()).unwrap();

        convert_file(&source, &workbook, Format::Json, Format::Xlsx).unwrap();
        convert_file(&workbook, &restored, Format::Xlsx, Format::Json).unwrap();

        let converted: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&restored).unwrap()).unwrap();
        remove_files(&[&source, &workbook, &restored]);
        prop_assert_eq!(converted, records);
    }
}
