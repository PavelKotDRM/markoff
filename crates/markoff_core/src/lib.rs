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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

mod csv_format;
mod data;
mod document;
mod docx_inline;
mod docx_reader;
mod docx_writer;
mod error;
mod html;
mod model;
mod pdf;
mod pptx;
mod tables;
mod xlsx;
mod xml_utils;
mod zip_utils;

pub use model::{ConversionRequest, Format, MarkoffError, detect_format};

use csv_format::{convert_csv_to_markdown, convert_markdown_to_csv};
use data::{convert_data_to_xlsx, convert_xlsx_to_data};
use document::{convert_document_to_markdown, convert_markdown_to_document};
use docx_reader::convert_docx_to_markdown;
use docx_writer::convert_markdown_to_docx;
use html::{convert_html_to_markdown, convert_markdown_to_html};
use pdf::convert_pdf_to_markdown;
use pptx::{convert_markdown_to_pptx, convert_pptx_to_markdown};
use xlsx::{convert_markdown_to_xlsx, convert_xlsx_to_markdown};

static INTERMEDIATE_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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

    if !request.overwrite && request.output.exists() {
        return Err(MarkoffError::OutputExists {
            path: request.output.to_string_lossy().to_string(),
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
        (Format::Docx, Format::Csv | Format::Xlsx) => convert_via_markdown_intermediate(
            |markdown| convert_docx_to_markdown(&request.input, markdown),
            |markdown| match request.to {
                Format::Csv => {
                    convert_markdown_to_csv(markdown, &request.output, request.csv_delimiter)
                }
                Format::Xlsx => convert_markdown_to_xlsx(markdown, &request.output),
                _ => unreachable!("only CSV and XLSX reach this branch"),
            },
        ),
        (Format::Docx, Format::Json | Format::Yaml | Format::Toml) => {
            convert_via_markdown_intermediate(
                |markdown| convert_docx_to_markdown(&request.input, markdown),
                |markdown| convert_markdown_to_document(markdown, &request.output, request.to),
            )
        }
        (Format::Csv | Format::Xlsx, Format::Docx) => convert_via_markdown_intermediate(
            |markdown| match request.from {
                Format::Csv => {
                    convert_csv_to_markdown(&request.input, markdown, request.csv_delimiter)
                }
                Format::Xlsx => convert_xlsx_to_markdown(&request.input, markdown),
                _ => unreachable!("only CSV and XLSX reach this branch"),
            },
            |markdown| convert_markdown_to_docx(markdown, &request.output),
        ),
        (Format::Json | Format::Yaml | Format::Toml, Format::Docx) => {
            convert_via_markdown_intermediate(
                |markdown| convert_document_to_markdown(&request.input, markdown, request.from),
                |markdown| convert_markdown_to_docx(markdown, &request.output),
            )
        }
        (Format::Pdf, Format::Markdown) => convert_pdf_to_markdown(&request.input, &request.output),
        (Format::Json | Format::Yaml | Format::Toml, Format::Markdown) => {
            convert_document_to_markdown(&request.input, &request.output, request.from)
        }
        (Format::Markdown, Format::Json | Format::Yaml | Format::Toml) => {
            convert_markdown_to_document(&request.input, &request.output, request.to)
        }
        (Format::Csv, Format::Markdown) => {
            convert_csv_to_markdown(&request.input, &request.output, request.csv_delimiter)
        }
        (Format::Markdown, Format::Csv) => {
            convert_markdown_to_csv(&request.input, &request.output, request.csv_delimiter)
        }
        (Format::Xlsx, Format::Markdown) => {
            convert_xlsx_to_markdown(&request.input, &request.output)
        }
        (Format::Markdown, Format::Xlsx) => {
            convert_markdown_to_xlsx(&request.input, &request.output)
        }
        (Format::Json | Format::Csv | Format::Yaml | Format::Toml, Format::Xlsx) => {
            convert_data_to_xlsx(
                &request.input,
                &request.output,
                request.from,
                request.csv_delimiter,
            )
        }
        (Format::Xlsx, Format::Json | Format::Csv | Format::Yaml | Format::Toml) => {
            convert_xlsx_to_data(
                &request.input,
                &request.output,
                request.to,
                request.csv_delimiter,
            )
        }
        (Format::Pptx, Format::Markdown) => {
            convert_pptx_to_markdown(&request.input, &request.output)
        }
        (Format::Markdown, Format::Pptx) => {
            convert_markdown_to_pptx(&request.input, &request.output)
        }
        (Format::Html, Format::Markdown) => {
            convert_html_to_markdown(&request.input, &request.output)
        }
        (Format::Markdown, Format::Html) => {
            convert_markdown_to_html(&request.input, &request.output)
        }
        _ => Err(MarkoffError::NotImplemented {
            from: request.from,
            to: request.to,
        }),
    }
}

/// Runs a conversion that must pass through an intermediate Markdown file:
/// `to_markdown` renders the source into a fresh temporary Markdown path,
/// then `from_markdown` renders that same path into the final output. The
/// temporary file (and any side files it created, such as an `image/`
/// folder) is always cleaned up, even when either step fails.
fn convert_via_markdown_intermediate(
    to_markdown: impl FnOnce(&Path) -> Result<(), MarkoffError>,
    from_markdown: impl FnOnce(&Path) -> Result<(), MarkoffError>,
) -> Result<(), MarkoffError> {
    let markdown = intermediate_path("md");
    let result = to_markdown(&markdown).and_then(|()| from_markdown(&markdown));
    remove_intermediate_markdown(&markdown);
    result
}

fn intermediate_path(extension: &str) -> PathBuf {
    let sequence = INTERMEDIATE_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "markoff_intermediate_{}_{}",
        std::process::id(),
        sequence
    ));
    std::fs::create_dir_all(&directory).ok();
    directory.join(format!("intermediate.{extension}"))
}

/// Removes an intermediate file's own temp directory, including any `image`
/// folder that DOCX image extraction may have created beside it. Each
/// intermediate path lives in its own directory (see `intermediate_path`), so
/// this cannot affect unrelated concurrent conversions.
fn remove_intermediate_markdown(markdown: &Path) {
    match markdown.parent() {
        Some(parent) => {
            std::fs::remove_dir_all(parent).ok();
        }
        None => {
            std::fs::remove_file(markdown).ok();
        }
    }
}

/// Convenience wrapper for converting a single file using the input and output paths.
///
/// Always overwrites an existing file at `output`; use `convert_document` with
/// `ConversionRequest.overwrite` set to `false` to require the destination to
/// be absent.
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
        overwrite: true,
        csv_delimiter: b',',
    };

    convert_document(&request)
}

/// Test-only helper shared by unit tests across modules, avoiding duplicate
/// `unique_temp_path`-style helpers in every file that needs one.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub(crate) fn unique_temp_path(name: &str, extension: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "markoff_{name}_{}_{nanos}.{extension}",
            std::process::id()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConversionRequest, Format, MarkoffError, convert_document, convert_file, detect_format,
    };
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
        assert!(matches!(detect_format("report.pdf"), Ok(Format::Pdf)));
        assert!(matches!(detect_format("sheet.xlsx"), Ok(Format::Xlsx)));
        assert!(matches!(detect_format("records.csv"), Ok(Format::Csv)));
    }

    #[test]
    fn rejects_unknown_formats() {
        assert!(detect_format("archive.bin").is_err());
    }

    #[test]
    fn rejects_existing_output_without_overwrite() {
        let input = unique_temp_path("overwrite_input");
        let output = unique_temp_path("overwrite_output");
        fs::write(&input, r#"{"name":"Ada"}"#).unwrap();
        fs::write(&output, "pre-existing content").unwrap();

        let request = ConversionRequest {
            input: input.clone(),
            output: output.clone(),
            from: Format::Json,
            to: Format::Markdown,
            overwrite: false,
            csv_delimiter: b',',
        };

        assert!(matches!(
            convert_document(&request),
            Err(MarkoffError::OutputExists { .. })
        ));
        assert_eq!(fs::read_to_string(&output).unwrap(), "pre-existing content");

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn overwrites_existing_output_when_requested() {
        let input = unique_temp_path("overwrite_allowed_input");
        let output = unique_temp_path("overwrite_allowed_output");
        fs::write(&input, r#"{"blocks":[{"type":"paragraph","text":"Ada"}]}"#).unwrap();
        fs::write(&output, "pre-existing content").unwrap();

        let request = ConversionRequest {
            input: input.clone(),
            output: output.clone(),
            from: Format::Json,
            to: Format::Markdown,
            overwrite: true,
            csv_delimiter: b',',
        };

        convert_document(&request).unwrap();
        assert!(fs::read_to_string(&output).unwrap().contains("Ada"));

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn converts_json_to_markdown() {
        let input = unique_temp_path("json_to_markdown_input");
        let output = unique_temp_path("json_to_markdown_output");
        fs::write(
            &input,
            r#"{"blocks":[{"type":"heading","level":1,"text":"Ada"},{"type":"paragraph","text":"count 42"}]}"#,
        )
        .unwrap();

        convert_file(&input, &output, Format::Json, Format::Markdown).unwrap();

        let rendered = fs::read_to_string(&output).unwrap();
        assert!(rendered.contains("# Ada"));
        assert!(rendered.contains("count 42"));

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn converts_pdf_to_markdown() {
        let input = unique_temp_path("pdf_to_markdown_input");
        let output = unique_temp_path("pdf_to_markdown_output");
        let content = b"BT /F1 12 Tf 72 720 Td (Hello from PDF) Tj ET";
        let objects = [
            b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".as_slice(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".as_slice(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".as_slice(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"5 0 obj\n<< /Length ");
        pdf.extend_from_slice(content.len().to_string().as_bytes());
        pdf.extend_from_slice(b" >>\nstream\n");
        pdf.extend_from_slice(content);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        let xref_offset = pdf.len();
        pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
                .as_bytes(),
        );
        fs::write(&input, pdf).unwrap();

        convert_file(&input, &output, Format::Pdf, Format::Markdown).unwrap();

        assert!(
            fs::read_to_string(&output)
                .unwrap()
                .contains("Hello from PDF")
        );

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
        fs::write(
            &input,
            "blocks:\n  - type: paragraph\n    text: Ada count 42\n",
        )
        .unwrap();

        convert_file(&input, &output, Format::Yaml, Format::Markdown).unwrap();

        let rendered = fs::read_to_string(&output).unwrap();
        assert!(rendered.contains("Ada count 42"));

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn converts_toml_to_markdown() {
        let input = unique_temp_path("toml_to_markdown_input");
        let output = unique_temp_path("toml_to_markdown_output");
        fs::write(
            &input,
            "[[blocks]]\ntype = \"paragraph\"\ntext = \"Ada count 42\"\n",
        )
        .unwrap();

        convert_file(&input, &output, Format::Toml, Format::Markdown).unwrap();

        let rendered = fs::read_to_string(&output).unwrap();
        assert!(rendered.contains("Ada count 42"));

        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }

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
}
