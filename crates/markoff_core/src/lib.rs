#![warn(missing_docs)]

//! Core conversion engine for the `markoff` document converter.
//!
//! This crate exposes the shared domain model, format detection helpers,
//! and conversion orchestration used by the CLI and GUI layers.
//!
//! # Examples
//!
//! ```rust
//! use markoff_core::{detect_format, Format};
//! assert!(matches!(detect_format("report.md"), Ok(Format::Markdown)));
//! ```

use std::path::Path;

mod csv_format;
mod data;
mod docx_inline;
mod docx_reader;
mod docx_writer;
mod error;
mod model;
mod tables;
mod text;
mod xlsx;

pub use model::{ConversionRequest, Format, MarkoffError, detect_format};

use csv_format::{convert_csv_to_markdown, convert_markdown_to_csv};
use data::{convert_data_to_xlsx, convert_xlsx_to_data};
use docx_reader::convert_docx_to_markdown;
use docx_writer::convert_markdown_to_docx;
use xlsx::{convert_markdown_to_xlsx, convert_xlsx_to_markdown};

/// Converts a document using a conversion request.
///
/// # Errors
///
/// Returns a typed conversion error when validation or conversion fails.
///
/// # Panics
///
/// This function does not panic under normal use.
pub fn convert_document(request: &ConversionRequest) -> Result<(), MarkoffError> {
    if !request.input.exists() {
        return Err(MarkoffError::InvalidInput {
            path: request.input.to_string_lossy().to_string(),
        });
    }

    if let Some(parent) = request.output.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|_| MarkoffError::OutputDirectory {
            path: parent.to_string_lossy().to_string(),
        })?;
    }

    match (request.from, request.to) {
        (Format::Docx, Format::Markdown) => {
            convert_docx_to_markdown(&request.input, &request.output)
        }
        (Format::Markdown, Format::Docx) => {
            convert_markdown_to_docx(&request.input, &request.output)
        }
        (Format::Json, Format::Markdown) => {
            text::convert_json_to_markdown(&request.input, &request.output)
        }
        (Format::Markdown, Format::Json) => {
            text::convert_markdown_to_json(&request.input, &request.output)
        }
        (Format::Csv, Format::Markdown) => convert_csv_to_markdown(&request.input, &request.output),
        (Format::Markdown, Format::Csv) => convert_markdown_to_csv(&request.input, &request.output),
        (Format::Yaml, Format::Markdown) => {
            text::convert_yaml_to_markdown(&request.input, &request.output)
        }
        (Format::Toml, Format::Markdown) => {
            text::convert_toml_to_markdown(&request.input, &request.output)
        }
        (Format::Xlsx, Format::Markdown) => {
            convert_xlsx_to_markdown(&request.input, &request.output)
        }
        (Format::Markdown, Format::Xlsx) => {
            convert_markdown_to_xlsx(&request.input, &request.output)
        }
        (Format::Json | Format::Csv | Format::Yaml | Format::Toml, Format::Xlsx) => {
            convert_data_to_xlsx(&request.input, &request.output, request.from)
        }
        (Format::Xlsx, Format::Json | Format::Csv | Format::Yaml | Format::Toml) => {
            convert_xlsx_to_data(&request.input, &request.output, request.to)
        }
        _ => Err(MarkoffError::NotImplemented {
            from: request.from,
            to: request.to,
        }),
    }
}

/// Convenience wrapper for converting a single file using the input and output paths.
pub fn convert_file<P, Q>(input: P, output: Q, from: Format, to: Format) -> Result<(), MarkoffError>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let request = ConversionRequest {
        input: input.as_ref().to_path_buf(),
        output: output.as_ref().to_path_buf(),
        from,
        to,
    };

    convert_document(&request)
}

#[cfg(test)]
mod tests {
    use super::{Format, convert_file, detect_format};
    use crate::xlsx::{read_xlsx_sheets, write_xlsx_sheets};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("markoff_{name}_{nanos}.tmp"))
    }

    #[test]
    fn detects_known_formats() {
        assert!(matches!(detect_format("report.md"), Ok(Format::Markdown)));
        assert!(matches!(detect_format("sheet.xlsx"), Ok(Format::Xlsx)));
        assert!(matches!(detect_format("records.csv"), Ok(Format::Csv)));
    }

    #[test]
    fn rejects_unknown_formats() {
        assert!(detect_format("archive.bin").is_err());
    }

    #[test]
    fn converts_json_to_markdown() {
        let input = unique_temp_path("json_to_markdown_input");
        let output = unique_temp_path("json_to_markdown_output");
        fs::write(&input, r#"{"name":"Ada","count":42}"#).unwrap();

        convert_file(&input, &output, Format::Json, Format::Markdown).unwrap();

        let rendered = fs::read_to_string(&output).unwrap();
        assert!(rendered.contains("```json"));
        assert!(rendered.contains("Ada"));
        assert!(rendered.contains("42"));

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn converts_markdown_to_json() {
        let input = unique_temp_path("markdown_to_json_input");
        let output = unique_temp_path("markdown_to_json_output");
        fs::write(&input, "# Example\n\nThis is a markdown note.").unwrap();

        convert_file(&input, &output, Format::Markdown, Format::Json).unwrap();

        let rendered = fs::read_to_string(&output).unwrap();
        assert!(rendered.contains("Example"));
        assert!(rendered.contains("markdown note"));

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn converts_csv_to_markdown() {
        let input = unique_temp_path("csv_to_markdown_input");
        let output = unique_temp_path("csv_to_markdown_output");
        fs::write(&input, "name,age\nAda,42\nBob,30\n").unwrap();

        convert_file(&input, &output, Format::Csv, Format::Markdown).unwrap();

        let rendered = fs::read_to_string(&output).unwrap();
        assert!(rendered.contains("| name | age |"));
        assert!(rendered.contains("| Ada | 42 |"));

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn converts_markdown_to_csv() {
        let input = unique_temp_path("markdown_to_csv_input");
        let output = unique_temp_path("markdown_to_csv_output");
        fs::write(
            &input,
            "| name | age |\n| --- | --- |\n| Ada | 42 |\n| Bob | 30 |\n",
        )
        .unwrap();

        convert_file(&input, &output, Format::Markdown, Format::Csv).unwrap();

        let rendered = fs::read_to_string(&output).unwrap();
        assert!(rendered.contains("name,age"));
        assert!(rendered.contains("Ada,42"));

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn converts_yaml_to_markdown() {
        let input = unique_temp_path("yaml_to_markdown_input");
        let output = unique_temp_path("yaml_to_markdown_output");
        fs::write(&input, "name: Ada\ncount: 42\n").unwrap();

        convert_file(&input, &output, Format::Yaml, Format::Markdown).unwrap();

        let rendered = fs::read_to_string(&output).unwrap();
        assert!(rendered.contains("```yaml"));
        assert!(rendered.contains("name: Ada"));

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn converts_toml_to_markdown() {
        let input = unique_temp_path("toml_to_markdown_input");
        let output = unique_temp_path("toml_to_markdown_output");
        fs::write(&input, "name = \"Ada\"\ncount = 42\n").unwrap();

        convert_file(&input, &output, Format::Toml, Format::Markdown).unwrap();

        let rendered = fs::read_to_string(&output).unwrap();
        assert!(rendered.contains("```toml"));
        assert!(rendered.contains("name = \"Ada\""));

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

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
        assert!(rendered.contains("1. Second step"));

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
}
