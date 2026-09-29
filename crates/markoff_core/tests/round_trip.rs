use markoff_core::{ConversionRequest, Format, convert_document, convert_file};
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
        "8. Шаг первый",
        "9. Шаг второй",
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
fn golden_pdf_extracts_embedded_images_alongside_markdown() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("golden/golden_test_document.pdf");
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "markoff_golden_pdf_images_{}_{}",
        std::process::id(),
        sequence
    ));
    fs::create_dir_all(&directory).unwrap();
    let markdown = directory.join("golden.md");

    convert_file(&source, &markdown, Format::Pdf, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&markdown).unwrap();
    assert!(
        rendered
            .lines()
            .any(|line| !line.trim().is_empty() && !line.starts_with("![](")),
        "missing extracted text in {rendered:?}"
    );
    assert!(
        rendered.contains("![](image/image1.png)"),
        "missing extracted image reference in {rendered:?}"
    );

    let image_bytes = fs::read(directory.join("image").join("image1.png")).unwrap();
    assert_eq!(
        &image_bytes[..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "extracted file is not a valid PNG"
    );

    fs::remove_dir_all(directory).ok();
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
fn csv_round_trip_preserves_embedded_newlines_and_backslashes() {
    let csv = temporary_path("csv_special_input", "csv");
    let markdown = temporary_path("csv_special", "md");
    let restored_csv = temporary_path("csv_special_restored", "csv");
    let document = temporary_path("csv_special", "docx");
    let restored_docx_csv = temporary_path("csv_special_docx_restored", "csv");
    fs::write(&csv, "id,name,tags\n1,\"Multi\nline note\",\"a|b\\c\"\n").unwrap();
    let expected = vec![vec![
        "1".to_string(),
        "Multi\nline note".to_string(),
        "a|b\\c".to_string(),
    ]];

    convert_file(&csv, &markdown, Format::Csv, Format::Markdown).unwrap();
    convert_file(&markdown, &restored_csv, Format::Markdown, Format::Csv).unwrap();
    assert_eq!(
        read_csv_records(&restored_csv),
        expected,
        "Markdown round trip should preserve embedded newlines and backslashes"
    );

    convert_file(&csv, &document, Format::Csv, Format::Docx).unwrap();
    convert_file(&document, &restored_docx_csv, Format::Docx, Format::Csv).unwrap();
    assert_eq!(
        read_csv_records(&restored_docx_csv),
        expected,
        "DOCX round trip should preserve embedded newlines and backslashes"
    );

    remove_files(&[
        &csv,
        &markdown,
        &restored_csv,
        &document,
        &restored_docx_csv,
    ]);
}

#[test]
fn csv_supports_a_custom_delimiter() {
    let csv = temporary_path("csv_semicolon_input", "csv");
    let markdown = temporary_path("csv_semicolon", "md");
    let restored_csv = temporary_path("csv_semicolon_restored", "csv");
    fs::write(&csv, "id;name;amount\n1;Ada;42\n2;Grace;99\n").unwrap();

    convert_document(&ConversionRequest {
        input: csv.clone(),
        output: markdown.clone(),
        from: Format::Csv,
        to: Format::Markdown,
        overwrite: true,
        csv_delimiter: b';',
        tables_only: false,
    })
    .unwrap();
    assert_eq!(
        fs::read_to_string(&markdown).unwrap(),
        "| id | name | amount |\n| --- | --- | --- |\n| 1 | Ada | 42 |\n| 2 | Grace | 99 |"
    );

    convert_document(&ConversionRequest {
        input: markdown.clone(),
        output: restored_csv.clone(),
        from: Format::Markdown,
        to: Format::Csv,
        overwrite: true,
        csv_delimiter: b';',
        tables_only: false,
    })
    .unwrap();
    assert_eq!(
        fs::read_to_string(&restored_csv).unwrap(),
        fs::read_to_string(&csv).unwrap()
    );

    remove_files(&[&csv, &markdown, &restored_csv]);
}

fn read_csv_records(path: &PathBuf) -> Vec<Vec<String>> {
    let mut reader = csv::Reader::from_path(path).unwrap();
    reader
        .records()
        .map(|record| record.unwrap().iter().map(str::to_string).collect())
        .collect()
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
fn markdown_json_yaml_toml_docx_html_chain_preserves_document_elements() {
    let markdown = temporary_path("format_chain_input", "md");
    let json = temporary_path("format_chain", "json");
    let yaml = temporary_path("format_chain", "yaml");
    let toml = temporary_path("format_chain", "toml");
    let docx = temporary_path("format_chain", "docx");
    let html = temporary_path("format_chain", "html");
    let restored_markdown = temporary_path("format_chain_restored", "md");
    fs::write(
        &markdown,
        "# Report\n\nA paragraph.\n\n- First item\n- Second item\n\n| Name | Score |\n| --- | --- |\n| Ada | 42 |\n",
    )
    .unwrap();

    convert_file(&markdown, &json, Format::Markdown, Format::Json).unwrap();
    convert_file(&json, &yaml, Format::Json, Format::Yaml).unwrap();
    convert_file(&yaml, &toml, Format::Yaml, Format::Toml).unwrap();

    let json_document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    let yaml_document: serde_json::Value =
        serde_yaml::from_str(&fs::read_to_string(&yaml).unwrap()).unwrap();
    let toml_document: serde_json::Value =
        toml::from_str(&fs::read_to_string(&toml).unwrap()).unwrap();
    assert_eq!(yaml_document, json_document);
    assert_eq!(toml_document, json_document);

    convert_file(&toml, &docx, Format::Toml, Format::Docx).unwrap();
    convert_file(&docx, &html, Format::Docx, Format::Html).unwrap();

    let rendered = fs::read_to_string(&html).unwrap();
    for expected in [
        "<h1>Report</h1>",
        "<p>A paragraph.</p>",
        "<ul>",
        "<p>First item</p>",
        "<p>Second item</p>",
        "<table>",
        "<th>Name</th>",
        "<td>Ada</td>",
        "<td>42</td>",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in final HTML: {rendered:?}"
        );
    }

    convert_file(&html, &restored_markdown, Format::Html, Format::Markdown).unwrap();
    let restored = fs::read_to_string(&restored_markdown).unwrap();
    assert_eq!(restored, fs::read_to_string(&markdown).unwrap());

    remove_files(&[
        &markdown,
        &json,
        &yaml,
        &toml,
        &docx,
        &html,
        &restored_markdown,
    ]);
}

#[test]
fn markdown_structured_formats_preserve_inline_semantics_and_footnotes() {
    let workspace = temporary_path("rich_schema_workspace", "dir");
    let markdown = workspace.join("input.md");
    let json = workspace.join("document.json");
    let yaml = workspace.join("document.yaml");
    let toml = workspace.join("document.toml");
    let restored_json = workspace.join("restored_json.md");
    let restored_yaml = workspace.join("restored_yaml.md");
    let restored_toml = workspace.join("restored_toml.md");
    let html = workspace.join("document.html");
    let restored_html = workspace.join("restored_html.md");
    let docx = workspace.join("document.docx");
    let restored_docx = workspace.join("restored_docx.md");
    let docx_html = workspace.join("document_from_docx.html");
    let restored_docx_html = workspace.join("restored_docx_html.md");
    let source = "# Report\n\nText **bold**, *italic*, ~~strike~~, <u>underlined</u>, `code`, [site](https://example.com \"title\"), x$^{2}$ and H$_{2}$O[^note]. See <a id=\"spot\"></a>here and ![graph](image/picture.png).\n\n- [x] Done\n    - Nested\n- [ ] Pending\n\n8. First\n9. Second\n\n| **Name** | Score |\n| :--- | ---: |\n| Ada | 42 |\n\n```rust\nfn main() {}\n```\n\n> Quoted *text*.\n\n---\n\n$$\nE = mc^2\n$$\n\nInline footnote^[Inline **note**].\n\n[^note]: Footnote *body*.\n\n    Additional **paragraph**.\n";
    fs::create_dir_all(workspace.join("image")).unwrap();
    let image_bytes = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    fs::write(workspace.join("image").join("picture.png"), image_bytes).unwrap();
    fs::write(&markdown, source).unwrap();

    convert_file(&markdown, &json, Format::Markdown, Format::Json).unwrap();
    convert_file(&json, &yaml, Format::Json, Format::Yaml).unwrap();
    convert_file(&yaml, &toml, Format::Yaml, Format::Toml).unwrap();

    let json_document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    let yaml_document: serde_json::Value =
        serde_yaml::from_str(&fs::read_to_string(&yaml).unwrap()).unwrap();
    let toml_document: serde_json::Value =
        toml::from_str(&fs::read_to_string(&toml).unwrap()).unwrap();
    assert_eq!(yaml_document, json_document);
    assert_eq!(toml_document, json_document);

    fn has_type(value: &serde_json::Value, expected: &str) -> bool {
        match value {
            serde_json::Value::Array(values) => {
                values.iter().any(|value| has_type(value, expected))
            }
            serde_json::Value::Object(fields) => {
                fields.get("type").and_then(|value| value.as_str()) == Some(expected)
                    || fields.values().any(|value| has_type(value, expected))
            }
            _ => false,
        }
    }

    assert!(has_type(&json_document, "strong"));
    assert!(has_type(&json_document, "emphasis"));
    assert!(has_type(&json_document, "strikethrough"));
    assert!(has_type(&json_document, "underline"));
    assert!(has_type(&json_document, "code"));
    assert!(has_type(&json_document, "math"));
    assert!(has_type(&json_document, "superscript"));
    assert!(has_type(&json_document, "subscript"));
    assert!(has_type(&json_document, "link"));
    assert!(has_type(&json_document, "image"));
    assert!(has_type(&json_document, "bookmark"));
    assert!(has_type(&json_document, "footnote_reference"));
    assert!(has_type(&json_document, "footnote"));
    assert!(has_type(&json_document, "footnote_definition"));
    assert!(has_type(&json_document, "quote"));
    assert!(has_type(&json_document, "horizontal_rule"));
    assert!(has_type(&json_document, "math"));

    let blocks = json_document["blocks"].as_array().unwrap();
    let checked_list = blocks.iter().find(|block| block["type"] == "list").unwrap();
    assert_eq!(
        checked_list["items"][0]["blocks"][0]["content"][0]["checked"],
        true
    );
    assert_eq!(
        checked_list["items"][1]["blocks"][0]["content"][0]["checked"],
        false
    );
    let ordered_list = blocks
        .iter()
        .find(|block| block["type"] == "list" && block["ordered"] == true)
        .unwrap();
    assert_eq!(ordered_list["start"], 8);
    let table = blocks
        .iter()
        .find(|block| block["type"] == "table")
        .unwrap();
    assert_eq!(table["alignments"], serde_json::json!(["left", "right"]));
    assert!(
        blocks
            .iter()
            .any(|block| { block["type"] == "code_block" && block["info"] == "rust" })
    );
    assert!(
        json_document
            .to_string()
            .contains("\"destination\":\"https://example.com\"")
    );
    assert!(json_document.to_string().contains("\"label\":\"note\""));
    convert_file(&json, &html, Format::Json, Format::Html).unwrap();
    assert!(
        fs::read_to_string(&html)
            .unwrap()
            .contains("src=\"data:image/png;base64,iVBORw0KGgo=\"")
    );
    convert_file(&html, &restored_html, Format::Html, Format::Markdown).unwrap();
    let html_markdown = fs::read_to_string(&restored_html).unwrap();
    for expected in [
        "Text **bold**",
        "[site](https://example.com \"title\")",
        "H$_{2}$O",
        "E = mc^2",
        "<a id=\"spot\"></a>",
        "```rust",
        "[^note]",
        "[^note]: Footnote *body*.",
        "Additional **paragraph**.",
        "Additional **paragraph**.",
        "[^markoff-inline-0]: Inline **note**",
        "- [x] Done",
        "    - Nested",
        "- [ ] Pending",
        "8. First",
        "9. Second",
        "| :--- | ---: |",
    ] {
        assert!(
            html_markdown.contains(expected),
            "missing {expected:?} after HTML round trip: {html_markdown:?}"
        );
    }

    for (input, format, output) in [
        (&json, Format::Json, &restored_json),
        (&yaml, Format::Yaml, &restored_yaml),
        (&toml, Format::Toml, &restored_toml),
    ] {
        convert_file(input, output, format, Format::Markdown).unwrap();
        assert_eq!(
            fs::read_to_string(output).unwrap(),
            source.replace("image/picture.png", "image/image1.png")
        );
        assert_eq!(
            fs::read(workspace.join("image").join("image1.png")).unwrap(),
            image_bytes
        );
    }

    convert_file(&toml, &docx, Format::Toml, Format::Docx).unwrap();
    convert_file(&docx, &restored_docx, Format::Docx, Format::Markdown).unwrap();
    let restored = fs::read_to_string(&restored_docx).unwrap();
    assert!(restored.contains("[^1]"), "{restored:?}");
    assert!(restored.contains("Footnote"), "{restored:?}");
    assert!(restored.contains("Additional"), "{restored:?}");
    assert!(restored.contains("<a id=\"spot\"></a>"), "{restored:?}");

    convert_file(&docx, &docx_html, Format::Docx, Format::Html).unwrap();
    convert_file(
        &docx_html,
        &restored_docx_html,
        Format::Html,
        Format::Markdown,
    )
    .unwrap();
    let restored_docx_html = fs::read_to_string(&restored_docx_html).unwrap();
    assert!(
        restored_docx_html.contains("Footnote"),
        "{restored_docx_html:?}"
    );
    assert!(
        restored_docx_html.contains("Additional"),
        "{restored_docx_html:?}"
    );
    assert!(
        restored_docx_html.contains("[^1]"),
        "{restored_docx_html:?}"
    );
    assert!(
        restored_docx_html.contains("<a id=\"spot\"></a>"),
        "{restored_docx_html:?}"
    );

    fs::remove_dir_all(workspace).ok();
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

#[test]
fn html_round_trip_preserves_headings_lists_and_links() {
    let markdown = temporary_path("html_roundtrip_input", "md");
    let html = temporary_path("html_roundtrip", "html");
    let restored = temporary_path("html_roundtrip_output", "md");
    fs::write(
        &markdown,
        "# Report\n\nA **bold** claim with a [link](https://example.com).\n\n- Alpha\n- Beta\n",
    )
    .unwrap();

    convert_file(&markdown, &html, Format::Markdown, Format::Html).unwrap();
    convert_file(&html, &restored, Format::Html, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&restored).unwrap();
    for expected in [
        "# Report",
        "**bold**",
        "[link](https://example.com)",
        "- Alpha",
        "- Beta",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }

    remove_files(&[&markdown, &html, &restored]);
}

#[test]
fn pptx_round_trip_preserves_slide_titles_and_bullets() {
    let markdown = temporary_path("pptx_roundtrip_input", "md");
    let pptx = temporary_path("pptx_roundtrip", "pptx");
    let restored = temporary_path("pptx_roundtrip_output", "md");
    fs::write(
        &markdown,
        "# Introduction\n\nWelcome note.\n\n## Agenda\n\n- Topic one\n- Topic two\n",
    )
    .unwrap();

    convert_file(&markdown, &pptx, Format::Markdown, Format::Pptx).unwrap();
    convert_file(&pptx, &restored, Format::Pptx, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&restored).unwrap();
    for expected in [
        "## Introduction",
        "Welcome note.",
        "## Agenda",
        "- Topic one",
        "- Topic two",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }

    remove_files(&[&markdown, &pptx, &restored]);
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
