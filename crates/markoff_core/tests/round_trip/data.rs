use super::{remove_files, temporary_path};
use markoff_core::{ConversionRequest, Format, convert_document, convert_file};
use proptest::prelude::*;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn read_csv_records(path: &PathBuf) -> Vec<Vec<String>> {
    let mut reader = csv::Reader::from_path(path).unwrap();
    reader
        .records()
        .map(|record| record.unwrap().iter().map(str::to_string).collect())
        .collect()
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
        style: None,
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
        style: None,
    })
    .unwrap();
    assert_eq!(
        fs::read_to_string(&restored_csv).unwrap(),
        fs::read_to_string(&csv).unwrap()
    );

    remove_files(&[&csv, &markdown, &restored_csv]);
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
fn structured_data_ods_round_trip_preserves_cell_types() {
    let source = temporary_path("structured_ods_input", "json");
    let workbook = temporary_path("structured_workbook", "ods");
    let restored = temporary_path("structured_ods_output", "json");
    let records = json!([
        {"name": "Ada", "score": 42, "active": true},
        {"name": "Grace", "score": -7, "active": false}
    ]);
    fs::write(&source, serde_json::to_string(&records).unwrap()).unwrap();

    convert_file(&source, &workbook, Format::Json, Format::Ods).unwrap();
    convert_file(&workbook, &restored, Format::Ods, Format::Json).unwrap();

    let converted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&restored).unwrap()).unwrap();
    assert_eq!(converted, records);

    remove_files(&[&source, &workbook, &restored]);
}

#[test]
fn edited_ods_markdown_updates_typed_cell_values() {
    let source = temporary_path("edited_ods_input", "json");
    let workbook = temporary_path("edited_ods_workbook", "ods");
    let markdown = temporary_path("edited_ods_markdown", "md");
    let edited_workbook = temporary_path("edited_ods_result", "ods");
    let restored = temporary_path("edited_ods_output", "json");
    let records = json!([{"name": "Ada", "score": 42, "active": true}]);
    fs::write(&source, serde_json::to_string(&records).unwrap()).unwrap();

    convert_file(&source, &workbook, Format::Json, Format::Ods).unwrap();
    convert_file(&workbook, &markdown, Format::Ods, Format::Markdown).unwrap();
    let edited = fs::read_to_string(&markdown)
        .unwrap()
        .replacen("42", "43", 1)
        .replacen("true", "false", 1);
    fs::write(&markdown, edited).unwrap();
    convert_file(&markdown, &edited_workbook, Format::Markdown, Format::Ods).unwrap();
    convert_file(&edited_workbook, &restored, Format::Ods, Format::Json).unwrap();

    let converted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&restored).unwrap()).unwrap();
    assert_eq!(
        converted,
        json!([{"name": "Ada", "score": 43, "active": false}])
    );

    remove_files(&[&source, &workbook, &markdown, &edited_workbook, &restored]);
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
