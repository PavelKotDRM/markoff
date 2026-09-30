use super::unique_temp_path;
use crate::{ConversionRequest, Format, convert_document, convert_file};
use std::fs;
use std::path::PathBuf;

#[test]
fn round_trips_markdown_document_through_json_yaml_toml() {
    for format in [Format::Json, Format::Yaml, Format::Toml] {
        let markdown = unique_temp_path("document_round_trip_markdown");
        let structured = unique_temp_path("document_round_trip_structured");
        let restored = unique_temp_path("document_round_trip_restored");
        fs::write(
            &markdown,
            "# Title\n\nA paragraph.\n\n- First\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n",
        )
        .unwrap();

        convert_file(&markdown, &structured, Format::Markdown, format).unwrap();
        convert_file(&structured, &restored, format, Format::Markdown).unwrap();

        assert_eq!(
            fs::read_to_string(&markdown).unwrap(),
            fs::read_to_string(&restored).unwrap()
        );

        fs::remove_file(markdown).ok();
        fs::remove_file(structured).ok();
        fs::remove_file(restored).ok();
    }
}

#[test]
fn round_trips_docx_document_through_json() {
    let markdown = unique_temp_path("docx_document_json_markdown");
    let document = unique_temp_path("docx_document_json_document");
    let json = unique_temp_path("docx_document_json");
    let restored_document = unique_temp_path("docx_document_json_restored_document");
    let restored_markdown = unique_temp_path("docx_document_json_restored_markdown");
    fs::write(
        &markdown,
        "# Title\n\n| Name | Score |\n| --- | --- |\n| Ada | 42 |\n",
    )
    .unwrap();

    convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();
    convert_file(&document, &json, Format::Docx, Format::Json).unwrap();
    let rendered = fs::read_to_string(&json).unwrap();
    assert!(rendered.contains("\"type\": \"heading\""));
    assert!(rendered.contains("\"type\": \"table\""));

    convert_file(&json, &restored_document, Format::Json, Format::Docx).unwrap();
    convert_file(
        &restored_document,
        &restored_markdown,
        Format::Docx,
        Format::Markdown,
    )
    .unwrap();
    let restored = fs::read_to_string(&restored_markdown).unwrap();
    assert!(restored.contains("# Title"));
    assert!(restored.contains("| Ada | 42 |"));

    fs::remove_file(markdown).ok();
    fs::remove_file(document).ok();
    fs::remove_file(json).ok();
    fs::remove_file(restored_document).ok();
    fs::remove_file(restored_markdown).ok();
}

#[test]
fn tables_only_keeps_table_blocks_when_converting_to_and_from_structured_data() {
    let markdown = unique_temp_path("tables_only_markdown");
    let json = unique_temp_path("tables_only_json");
    let table_only_json = unique_temp_path("tables_only_filtered_json");
    let document = unique_temp_path("tables_only_docx");
    let restored_markdown = unique_temp_path("tables_only_restored_markdown");
    fs::write(
        &markdown,
        "# Report\n\nSummary text.\n\n| Name | Score |\n| --- | --- |\n| Ada | 42 |\n",
    )
    .unwrap();

    convert_file(&markdown, &json, Format::Markdown, Format::Json).unwrap();
    convert_document(&ConversionRequest {
        input: markdown.clone(),
        output: table_only_json.clone(),
        from: Format::Markdown,
        to: Format::Json,
        overwrite: true,
        csv_delimiter: b',',
        tables_only: true,
    })
    .unwrap();

    let full_document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(full_document["blocks"].as_array().unwrap().len(), 3);
    let table_only: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&table_only_json).unwrap()).unwrap();
    let blocks = table_only["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["type"], "table");
    assert_eq!(blocks[0]["cells"][1][0][0]["text"], "Ada");

    convert_document(&ConversionRequest {
        input: json.clone(),
        output: document.clone(),
        from: Format::Json,
        to: Format::Docx,
        overwrite: true,
        csv_delimiter: b',',
        tables_only: true,
    })
    .unwrap();
    convert_file(
        &document,
        &restored_markdown,
        Format::Docx,
        Format::Markdown,
    )
    .unwrap();
    let restored = fs::read_to_string(&restored_markdown).unwrap();
    assert!(restored.contains("| Ada | 42 |"));
    assert!(!restored.contains("Report"));
    assert!(!restored.contains("Summary text"));

    for path in [markdown, json, table_only_json, document, restored_markdown] {
        fs::remove_file(path).ok();
    }
}

#[test]
fn converts_html_and_pptx_to_and_from_structured_document_formats() {
    let markdown = unique_temp_path("structured_format_markdown");
    let html = unique_temp_path("structured_format_html");
    let json = unique_temp_path("structured_format_json");
    let restored_html = unique_temp_path("structured_format_restored_html");
    let pptx = unique_temp_path("structured_format_pptx");
    let yaml = unique_temp_path("structured_format_yaml");
    let restored_pptx = unique_temp_path("structured_format_restored_pptx");
    let restored_markdown = unique_temp_path("structured_format_restored_markdown");
    fs::write(
        &markdown,
        "# Introduction\n\nA paragraph.\n\n- First item\n- Second item\n",
    )
    .unwrap();

    convert_file(&markdown, &html, Format::Markdown, Format::Html).unwrap();
    convert_file(&html, &json, Format::Html, Format::Json).unwrap();
    let html_document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    assert!(
        html_document["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|block| block["type"] == "heading")
    );
    convert_file(&json, &restored_html, Format::Json, Format::Html).unwrap();

    convert_file(&markdown, &pptx, Format::Markdown, Format::Pptx).unwrap();
    convert_file(&pptx, &yaml, Format::Pptx, Format::Yaml).unwrap();
    let pptx_document: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&yaml).unwrap()).unwrap();
    assert!(
        pptx_document["blocks"]
            .as_sequence()
            .unwrap()
            .iter()
            .any(|block| block["type"] == "heading")
    );
    convert_file(&yaml, &restored_pptx, Format::Yaml, Format::Pptx).unwrap();
    convert_file(
        &restored_pptx,
        &restored_markdown,
        Format::Pptx,
        Format::Markdown,
    )
    .unwrap();
    let rendered = fs::read_to_string(&restored_markdown).unwrap();
    assert!(rendered.contains("Introduction"));
    assert!(rendered.contains("First item"));

    for path in [
        markdown,
        html,
        json,
        restored_html,
        pptx,
        yaml,
        restored_pptx,
        restored_markdown,
    ] {
        fs::remove_file(path).ok();
    }
}

#[test]
fn converts_pdf_text_to_a_structured_document() {
    let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("golden/golden_test_document.pdf");
    let json = unique_temp_path("pdf_structured_json");

    convert_file(&pdf, &json, Format::Pdf, Format::Json).unwrap();

    let document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    let blocks = document["blocks"].as_array().unwrap();
    assert!(!blocks.is_empty());
    assert!(
        blocks
            .iter()
            .any(|block| block["type"] == "paragraph" || block["type"] == "image")
    );

    fs::remove_file(json).ok();
}

#[test]
fn converts_between_json_yaml_and_toml_values() {
    let json = unique_temp_path("structured_value_json");
    let yaml = unique_temp_path("structured_value_yaml");
    let restored_json = unique_temp_path("structured_value_restored_json");
    let toml = unique_temp_path("structured_value_toml");
    let restored_toml_json = unique_temp_path("structured_value_toml_restored_json");
    let value = serde_json::json!({
        "blocks": [
            {"type": "heading", "level": 1, "text": "Title"},
            {"type": "table", "rows": [["Name"], ["Ada"]]}
        ]
    });
    fs::write(&json, serde_json::to_string(&value).unwrap()).unwrap();

    convert_file(&json, &yaml, Format::Json, Format::Yaml).unwrap();
    convert_file(&yaml, &restored_json, Format::Yaml, Format::Json).unwrap();

    let restored: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&restored_json).unwrap()).unwrap();
    assert_eq!(restored, value);

    convert_file(&json, &toml, Format::Json, Format::Toml).unwrap();
    convert_file(&toml, &restored_toml_json, Format::Toml, Format::Json).unwrap();
    let restored_from_toml: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&restored_toml_json).unwrap()).unwrap();
    assert_eq!(restored_from_toml, value);

    for path in [json, yaml, restored_json, toml, restored_toml_json] {
        fs::remove_file(path).ok();
    }
}
