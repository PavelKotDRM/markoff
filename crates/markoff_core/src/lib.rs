#![warn(missing_docs)]

//! Core conversion engine for the `markoff` document converter.
//!
//! The crate provides a shared [`Format`] model, typed conversion
//! options ([`ConversionRequest`]), format detection, and the conversion
//! entry points used by the CLI and GUI.
//!
//! # Supported formats
//!
//! Conversions include Markdown, DOCX, PDF, PPTX, HTML, XLSX, CSV, JSON, YAML,
//! and TOML. Not every format pair is available; an unsupported pair returns
//! [`MarkoffError::NotImplemented`]. Office and PDF conversions may preserve
//! less layout information than text-based conversions.
//!
//! # Choosing a conversion API
//!
//! Use [`convert_file`] for a concise one-off conversion. It overwrites an
//! existing destination. Use [`convert_document`] with a [`ConversionRequest`]
//! to control overwrite behavior, the CSV delimiter, and table-only
//! conversion.
//!
//! # Examples
//!
//! Detect an input format before constructing a request:
//!
//! ```rust
//! # fn main() -> Result<(), markoff_core::MarkoffError> {
//! use markoff_core::{ConversionRequest, Format, detect_format};
//!
//! let source = "report.md";
//! let request = ConversionRequest {
//!     input: source.into(),
//!     output: "report.html".into(),
//!     from: detect_format(source)?,
//!     to: Format::Html,
//!     overwrite: false,
//!     csv_delimiter: b',',
//!     tables_only: false,
//! };
//! assert_eq!(request.from, Format::Markdown);
//! # Ok(())
//! # }
//! ```
//!
//! Run a conversion with [`convert_file`]:
//!
//! ```no_run
//! # use markoff_core::{convert_file, Format, MarkoffError};
//! # fn main() -> Result<(), MarkoffError> {
//! convert_file("report.md", "report.html", Format::Markdown, Format::Html)?;
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

mod csv_format;
mod data;
mod document;
mod document_model;
mod docx_footnotes;
mod docx_inline;
mod docx_markdown;
mod docx_postprocess;
mod docx_reader;
mod docx_resources;
mod docx_runs;
mod docx_writer;
mod error;
mod html;
mod html_tokenizer;
mod model;
mod pdf;
mod pptx;
mod pptx_reader;
mod tables;
mod xlsx;
mod xml_utils;
mod zip_utils;

pub use model::{ConversionRequest, Format, MarkoffError, detect_format};

use csv_format::{convert_csv_to_markdown, convert_markdown_to_csv};
use data::{convert_data_to_xlsx, convert_xlsx_to_data};
use document::{
    convert_document_to_markdown, convert_markdown_to_document, convert_structured_data_format,
};
use docx_reader::convert_docx_to_markdown;
use docx_writer::convert_markdown_to_docx;
use html::{convert_html_to_markdown, convert_markdown_to_html};
use pdf::convert_pdf_to_markdown;
use pptx::{convert_markdown_to_pptx, convert_pptx_to_markdown};
use xlsx::{convert_markdown_to_xlsx, convert_xlsx_to_markdown};

static INTERMEDIATE_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Converts one input file according to a [`ConversionRequest`].
///
/// The input must exist. Missing output directories are created before
/// conversion. When `tables_only` is enabled, the requested format pair must
/// be supported by [`supports_tables_only`].
///
/// # Errors
///
/// Returns a typed error when validation, file access, parsing, or conversion
/// fails. Unsupported format pairs return [`MarkoffError::NotImplemented`].
///
/// - [`MarkoffError::InvalidInput`] when the input file does not exist.
/// - [`MarkoffError::InvalidOption`] when `tables_only` is not supported for
///   the requested format pair.
/// - [`MarkoffError::OutputExists`] when overwriting is disabled and the
///   destination already exists.
/// - [`MarkoffError::OutputDirectory`] when a missing destination directory
///   cannot be created.
/// - [`MarkoffError::Io`] when reading, writing, or converting a file fails.
/// - [`MarkoffError::NotImplemented`] when the format pair is unavailable.
pub fn convert_document(request: &ConversionRequest) -> Result<(), MarkoffError> {
    if !request.input.exists() {
        return Err(MarkoffError::InvalidInput {
            path: request.input.to_string_lossy().to_string(),
        });
    }

    if request.tables_only && !supports_tables_only(request.from, request.to) {
        return Err(MarkoffError::InvalidOption {
            message: "--tables-only requires a conversion between a document format and JSON, YAML, or TOML".to_string(),
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
        (Format::Docx, Format::Html) => convert_via_markdown_intermediate(
            |markdown| convert_docx_to_markdown(&request.input, markdown),
            |markdown| convert_markdown_to_html(markdown, &request.output),
        ),
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
                |markdown| {
                    convert_markdown_to_document(
                        markdown,
                        &request.output,
                        request.to,
                        request.tables_only,
                    )
                },
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
                |markdown| {
                    convert_document_to_markdown(
                        &request.input,
                        markdown,
                        request.from,
                        request.tables_only,
                    )
                },
                |markdown| convert_markdown_to_docx(markdown, &request.output),
            )
        }
        (Format::Pdf, Format::Markdown) => convert_pdf_to_markdown(&request.input, &request.output),
        (Format::Pdf, Format::Json | Format::Yaml | Format::Toml) => {
            convert_via_markdown_intermediate(
                |markdown| convert_pdf_to_markdown(&request.input, markdown),
                |markdown| {
                    convert_markdown_to_document(
                        markdown,
                        &request.output,
                        request.to,
                        request.tables_only,
                    )
                },
            )
        }
        (Format::Json | Format::Yaml | Format::Toml, Format::Markdown) => {
            convert_document_to_markdown(
                &request.input,
                &request.output,
                request.from,
                request.tables_only,
            )
        }
        (Format::Markdown, Format::Json | Format::Yaml | Format::Toml) => {
            convert_markdown_to_document(
                &request.input,
                &request.output,
                request.to,
                request.tables_only,
            )
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
        (Format::Pptx, Format::Json | Format::Yaml | Format::Toml) => {
            convert_via_markdown_intermediate(
                |markdown| convert_pptx_to_markdown(&request.input, markdown),
                |markdown| {
                    convert_markdown_to_document(
                        markdown,
                        &request.output,
                        request.to,
                        request.tables_only,
                    )
                },
            )
        }
        (Format::Markdown, Format::Pptx) => {
            convert_markdown_to_pptx(&request.input, &request.output)
        }
        (Format::Html, Format::Markdown) => {
            convert_html_to_markdown(&request.input, &request.output)
        }
        (Format::Html, Format::Json | Format::Yaml | Format::Toml) => {
            convert_via_markdown_intermediate(
                |markdown| convert_html_to_markdown(&request.input, markdown),
                |markdown| {
                    convert_markdown_to_document(
                        markdown,
                        &request.output,
                        request.to,
                        request.tables_only,
                    )
                },
            )
        }
        (Format::Markdown, Format::Html) => {
            convert_markdown_to_html(&request.input, &request.output)
        }
        (Format::Json | Format::Yaml | Format::Toml, Format::Pptx) => {
            convert_via_markdown_intermediate(
                |markdown| {
                    convert_document_to_markdown(
                        &request.input,
                        markdown,
                        request.from,
                        request.tables_only,
                    )
                },
                |markdown| convert_markdown_to_pptx(markdown, &request.output),
            )
        }
        (Format::Json | Format::Yaml | Format::Toml, Format::Html) => {
            convert_via_markdown_intermediate(
                |markdown| {
                    convert_document_to_markdown(
                        &request.input,
                        markdown,
                        request.from,
                        request.tables_only,
                    )
                },
                |markdown| convert_markdown_to_html(markdown, &request.output),
            )
        }
        (
            Format::Json | Format::Yaml | Format::Toml,
            Format::Json | Format::Yaml | Format::Toml,
        ) if request.from != request.to => convert_structured_data_format(
            &request.input,
            &request.output,
            request.from,
            request.to,
        ),
        _ => Err(MarkoffError::NotImplemented {
            from: request.from,
            to: request.to,
        }),
    }
}

/// Reports whether table-only conversion applies to a format pair.
///
/// The option is available between JSON/YAML/TOML and Markdown, DOCX, PPTX, or
/// HTML. PDF is additionally supported as a source format, but not as a
/// destination.
///
/// # Examples
///
/// ```
/// use markoff_core::{Format, supports_tables_only};
///
/// assert!(supports_tables_only(Format::Docx, Format::Json));
/// assert!(!supports_tables_only(Format::Json, Format::Pdf));
/// ```
#[must_use]
pub fn supports_tables_only(from: Format, to: Format) -> bool {
    let structured = |format| matches!(format, Format::Json | Format::Yaml | Format::Toml);
    let document_source = |format| {
        matches!(
            format,
            Format::Markdown | Format::Docx | Format::Pdf | Format::Pptx | Format::Html
        )
    };
    let document_target = |format| {
        matches!(
            format,
            Format::Markdown | Format::Docx | Format::Pptx | Format::Html
        )
    };

    (structured(from) && document_target(to)) || (document_source(from) && structured(to))
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
///
/// # Errors
///
/// Returns the same typed conversion errors as [`convert_document`].
///
/// # Examples
///
/// ```no_run
/// use markoff_core::{Format, MarkoffError, convert_file};
///
/// # fn main() -> Result<(), MarkoffError> {
/// convert_file("report.md", "report.docx", Format::Markdown, Format::Docx)?;
/// # Ok(())
/// # }
/// ```
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
        tables_only: false,
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
mod tests;
