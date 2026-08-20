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
        "**_*italic*_**",
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
