use super::{remove_files, temporary_path};
use markoff_core::{Format, convert_file};
use serde_json::json;
use std::fs;

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
fn markdown_docx_round_trip_preserves_soft_and_hard_break_semantics() {
    let markdown = temporary_path("docx_line_breaks_input", "md");
    let document = temporary_path("docx_line_breaks_document", "docx");
    let restored = temporary_path("docx_line_breaks_output", "md");
    fs::write(
        &markdown,
        "A soft-wrapped\nparagraph stays together.\n\nA hard break  \nstays inside the paragraph.\n",
    )
    .unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();
    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();

    assert_eq!(
        fs::read_to_string(&restored).unwrap(),
        "A soft-wrapped paragraph stays together.\n\nA hard break  \nstays inside the paragraph.\n"
    );

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
fn markdown_docx_renders_links_heading_and_table_structure() {
    use std::io::Read;
    use zip::ZipArchive;

    let markdown = temporary_path("docx_structure_input", "md");
    let document = temporary_path("docx_structure_document", "docx");
    let restored = temporary_path("docx_structure_output", "md");
    let source = "# Report\n\nSee [website](https://example.com?a=1&b=2) or [section](#report).\n\n| Name | Value |\n| --- | --- |\n| Ada | 42 |\n";
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
    let mut relationships = String::new();
    archive
        .by_name("word/_rels/document.xml.rels")
        .unwrap()
        .read_to_string(&mut relationships)
        .unwrap();
    let mut styles = String::new();
    archive
        .by_name("word/styles.xml")
        .unwrap()
        .read_to_string(&mut styles)
        .unwrap();

    assert!(document_xml.contains(
        "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\""
    ));
    assert!(document_xml.contains("<w:pStyle w:val=\"Heading1\"/>"));
    assert!(document_xml.contains("<w:bookmarkStart w:id=\"1\" w:name=\"report\"/>"));
    assert!(document_xml.contains("<w:hyperlink r:id=\"rId4\">"));
    assert!(document_xml.contains("<w:hyperlink w:anchor=\"report\">"));
    assert!(document_xml.contains("<w:tblBorders>"));
    assert!(document_xml.contains("<w:gridCol w:w=\"2400\"/>"));
    assert!(document_xml.contains("<w:tblHeader/>"));
    assert!(document_xml.contains("<w:shd w:val=\"clear\" w:fill=\"D9EAF7\"/>"));
    assert!(relationships.contains(
        "Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://example.com?a=1&amp;b=2\" TargetMode=\"External\""
    ));
    assert!(relationships.contains(
        "Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\""
    ));
    assert!(styles.contains("<w:style w:type=\"paragraph\" w:styleId=\"Heading1\">"));
    assert!(styles.contains("<w:b/><w:color w:val=\"1F4E79\"/>"));

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
fn markdown_docx_accepts_tables_without_outer_pipes() {
    use std::io::Read;
    use zip::ZipArchive;

    let markdown = temporary_path("table_without_outer_pipes_input", "md");
    let document = temporary_path("table_without_outer_pipes_document", "docx");
    let restored = temporary_path("table_without_outer_pipes_output", "md");
    fs::write(&markdown, "Name | Score\n--- | ---\nAda | 42\n").unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();

    let file = fs::File::open(&document).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut document_xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut document_xml)
        .unwrap();
    assert!(document_xml.contains("<w:tbl>"));

    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();
    assert_eq!(
        fs::read_to_string(&restored).unwrap(),
        "| Name | Score |\n| --- | --- |\n| Ada | 42 |\n"
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
                    "cells": [
                        [
                            [{"type": "text", "text": "Name"}],
                            [{"type": "text", "text": "Score"}]
                        ],
                        [
                            [{"type": "text", "text": "Ada"}],
                            [{"type": "text", "text": "42"}]
                        ]
                    ]
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
fn markdown_docx_rejects_list_nesting_beyond_word_limit() {
    use std::io::Read;
    use zip::ZipArchive;

    let markdown = temporary_path("deep_list_input", "md");
    let document = temporary_path("deep_list_document", "docx");
    let source = (0..9)
        .map(|level| format!("{}- Level {level}", "    ".repeat(level)))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
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
    assert!(document_xml.contains("<w:ilvl w:val=\"8\"/>"));
    drop(archive);
    remove_files(&[&markdown, &document]);

    let markdown = temporary_path("too_deep_list_input", "md");
    let document = temporary_path("too_deep_list_document", "docx");
    let source = (0..10)
        .map(|level| format!("{}- Level {level}", "    ".repeat(level)))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&markdown, source).unwrap();

    let error = convert_file(&markdown, &document, Format::Markdown, Format::Docx)
        .expect_err("nesting beyond Word's nine levels must be rejected");
    assert!(matches!(
        error,
        markoff_core::MarkoffError::Io(ref error)
            if error.kind() == std::io::ErrorKind::InvalidInput
    ));
    assert!(!document.exists());

    remove_files(&[&markdown, &document]);
}

#[test]
fn markdown_docx_preserves_list_boundaries_and_start_numbers() {
    use std::io::Read;
    use zip::ZipArchive;

    let markdown = temporary_path("list_boundaries_input", "md");
    let document = temporary_path("list_boundaries_document", "docx");
    let restored = temporary_path("list_boundaries_output", "md");
    let source = "1. First group\n\n2. Second group\n\nA paragraph separator.\n\n1. Restarted group\n2. Second restarted item\n\n- Bullet group\n- Second bullet\n\n4) List starting at four\n5) Next item\n\n8. Mixed list first item\n\n- Mixed bullet first item\n\n- Mixed bullet second item\n\n9. Mixed list second item\n";
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
    let mut numbering_xml = String::new();
    archive
        .by_name("word/numbering.xml")
        .unwrap()
        .read_to_string(&mut numbering_xml)
        .unwrap();

    let list_ids = document_xml
        .split("<w:numId w:val=\"")
        .skip(1)
        .map(|entry| entry.split('"').next().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        list_ids,
        ["1", "1", "2", "2", "3", "3", "4", "4", "5", "6", "6", "7"]
    );
    assert!(
        numbering_xml.contains(
            "<w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"multilevel\"/>"
        )
    );
    assert!(
        numbering_xml.contains(
            "<w:abstractNum w:abstractNumId=\"1\"><w:multiLevelType w:val=\"multilevel\"/>"
        )
    );
    assert!(numbering_xml.contains(
        "<w:num w:numId=\"4\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"4\"/></w:lvlOverride></w:num>"
    ));
    assert!(numbering_xml.contains(
        "<w:num w:numId=\"5\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"8\"/></w:lvlOverride></w:num>"
    ));
    assert!(numbering_xml.contains(
        "<w:num w:numId=\"7\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"9\"/></w:lvlOverride></w:num>"
    ));

    convert_file(&document, &restored, Format::Docx, Format::Markdown).unwrap();
    let rendered = fs::read_to_string(&restored).unwrap();
    assert!(rendered.contains("4. List starting at four"));
    assert!(rendered.contains("5. Next item"));
    assert!(rendered.contains("8. Mixed list first item"));
    assert!(rendered.contains("9. Mixed list second item"));

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
fn markdown_docx_does_not_leak_unmatched_bold_markers() {
    use std::io::Read;
    use zip::ZipArchive;

    let markdown = temporary_path("unmatched_bold_input", "md");
    let document = temporary_path("unmatched_bold_document", "docx");
    fs::write(&markdown, "**bold** plain **unclosed\n").unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();

    let file = fs::File::open(&document).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut document_xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut document_xml)
        .unwrap();
    assert_eq!(document_xml.matches("<w:b/>").count(), 1);
    assert!(document_xml.contains("plain **unclosed"));

    remove_files(&[&markdown, &document]);
}

#[test]
fn markdown_docx_round_trip_preserves_footnotes() {
    let markdown = temporary_path("footnotes_input", "md");
    let document = temporary_path("footnotes_document", "docx");
    let restored = temporary_path("footnotes_output", "md");
    fs::write(
        &markdown,
        "Footnote 1 link[^first].\n\nFootnote 2 link[^second].\n\nInline footnote^[Text of inline footnote] definition.\n\nDuplicated footnote reference[^second].\n\n[^first]: Footnote **can have markup** with [docs](https://example.com).\n\n    and multiple paragraphs.\n\n[^second]: Footnote text.\n",
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
        "[^1]: Footnote **can have markup** with [docs](https://example.com).\n\n    and multiple paragraphs.",
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
