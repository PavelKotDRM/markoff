use super::unique_temp_path;
use crate::xlsx::{read_xlsx_sheets, write_xlsx_sheets};
use crate::{Format, convert_file};
use std::collections::BTreeMap;
use std::fs;

#[test]
fn round_trips_markdown_table_through_xlsx() {
    let markdown = unique_temp_path("markdown_xlsx_input");
    let workbook = unique_temp_path("markdown_xlsx_workbook");
    let restored = unique_temp_path("markdown_xlsx_output");
    fs::write(&markdown, "| name | score |\n| --- | --- |\n| Ada | 42 |\n").unwrap();

    convert_file(&markdown, &workbook, Format::Markdown, Format::Xlsx).unwrap();
    convert_file(&workbook, &restored, Format::Xlsx, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&restored).unwrap();
    assert!(rendered.contains("| name | score |"));
    assert!(rendered.contains("| Ada | 42 |"));

    fs::remove_file(markdown).ok();
    fs::remove_file(workbook).ok();
    fs::remove_file(restored).ok();
}

#[test]
fn round_trips_multiple_xlsx_sheets_through_markdown() {
    let workbook = unique_temp_path("multiple_sheets_input");
    let markdown = unique_temp_path("multiple_sheets_markdown");
    let restored = unique_temp_path("multiple_sheets_output");
    let mut sheets = BTreeMap::new();
    sheets.insert(
        "People".to_string(),
        vec![vec!["name".to_string()], vec!["Ada".to_string()]],
    );
    sheets.insert(
        "Scores".to_string(),
        vec![vec!["score".to_string()], vec!["42".to_string()]],
    );
    write_xlsx_sheets(&workbook, &sheets).unwrap();

    convert_file(&workbook, &markdown, Format::Xlsx, Format::Markdown).unwrap();
    convert_file(&markdown, &restored, Format::Markdown, Format::Xlsx).unwrap();

    let restored_sheets = read_xlsx_sheets(&restored).unwrap();
    assert_eq!(restored_sheets["People"][1][0], "Ada");
    assert_eq!(restored_sheets["Scores"][1][0], "42");

    fs::remove_file(workbook).ok();
    fs::remove_file(markdown).ok();
    fs::remove_file(restored).ok();
}

#[test]
fn round_trips_json_rows_through_xlsx() {
    let json = unique_temp_path("json_xlsx_input");
    let workbook = unique_temp_path("json_xlsx_workbook");
    let restored = unique_temp_path("json_xlsx_output");
    fs::write(&json, r#"[{"name":"Ada","active":true,"score":42}]"#).unwrap();

    convert_file(&json, &workbook, Format::Json, Format::Xlsx).unwrap();
    convert_file(&workbook, &restored, Format::Xlsx, Format::Json).unwrap();

    let rendered: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&restored).unwrap()).unwrap();
    assert_eq!(rendered[0]["name"], "Ada");
    assert_eq!(rendered[0]["active"], true);
    assert_eq!(rendered[0]["score"], 42);

    fs::remove_file(json).ok();
    fs::remove_file(workbook).ok();
    fs::remove_file(restored).ok();
}
