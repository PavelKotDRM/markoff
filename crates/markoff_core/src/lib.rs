#![warn(missing_docs)]

//! Core conversion engine for the `markoff` document converter.
//!
//! The crate provides a shared [`Format`] model, typed conversion
//! options ([`ConversionRequest`]), format detection, and the conversion
//! entry points used by the CLI and GUI.
//!
//! # Supported formats
//!
//! Conversions include Markdown, DOCX, ODT, PDF, PPTX, ODP, HTML, XLSX, ODS,
//! CSV, JSON, YAML, and TOML. Not every format pair is available; an unsupported pair returns
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
//!     style: None,
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
mod opendocument;
mod pdf;
mod pdf_writer;
mod pptx;
mod pptx_reader;
mod style;
mod tables;
mod xlsx;
mod xml_utils;
mod zip_utils;

pub use model::{ConversionRequest, Format, MarkoffError, detect_format};
pub use style::{
    StyleColor, StylePageOrientation, StylePageSize, StyleTextAlign, StyleThemePreview,
    default_style_theme_toml, load_style_theme_preview, write_default_style_theme,
};

use csv_format::{convert_csv_to_markdown, convert_markdown_to_csv};
use data::{convert_data_to_xlsx, convert_xlsx_to_data};
use document::{
    convert_document_to_markdown, convert_markdown_to_document, convert_structured_data_format,
    convert_structured_data_to_pdf,
};
use docx_reader::convert_docx_to_markdown;
use docx_writer::convert_markdown_to_docx;
use html::{convert_html_to_markdown, convert_markdown_to_html};
use opendocument::{
    convert_data_to_ods, convert_markdown_to_odp, convert_markdown_to_ods, convert_markdown_to_odt,
    convert_odp_to_markdown, convert_ods_to_data, convert_ods_to_markdown, convert_odt_to_markdown,
};
use pdf::convert_pdf_to_markdown;
use pdf_writer::convert_markdown_to_pdf;
use pptx::{convert_markdown_to_pptx, convert_pptx_to_markdown};
use style::{DocumentTheme, load_document_theme};
use xlsx::{convert_markdown_to_xlsx, convert_xlsx_to_markdown};

static INTERMEDIATE_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Converts one input file according to a [`ConversionRequest`].
///
/// The input must exist. Missing output directories are created before
/// conversion. When `tables_only` is enabled, the requested format pair must
/// be supported by [`supports_tables_only`]. The output must not refer to the
/// input file, even when overwriting is enabled.
///
/// # Errors
///
/// Returns a typed error when validation, file access, parsing, or conversion
/// fails. Unsupported format pairs return [`MarkoffError::NotImplemented`].
///
/// - [`MarkoffError::InvalidInput`] when the input file does not exist.
/// - [`MarkoffError::InvalidOption`] when `tables_only` is not supported for
///   the requested format pair, or the output refers to the input file.
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

    if request.output.exists() && same_file::is_same_file(&request.input, &request.output)? {
        return Err(MarkoffError::InvalidOption {
            message: "input and output paths must differ".to_string(),
        });
    }

    if request.tables_only && !supports_tables_only(request.from, request.to) {
        return Err(MarkoffError::InvalidOption {
            message: "--tables-only requires a conversion between a document format and JSON, YAML, or TOML".to_string(),
        });
    }
    if request.style.is_some()
        && !matches!(
            request.to,
            Format::Pdf | Format::Html | Format::Docx | Format::Odt
        )
    {
        return Err(MarkoffError::InvalidOption {
            message: format!(
                "style themes are supported only for PDF, HTML, DOCX, and ODT output, not {}",
                request.to
            ),
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

    route_conversion(request)
}

fn route_conversion(request: &ConversionRequest) -> Result<(), MarkoffError> {
    match request.from {
        Format::Docx => route_docx(request),
        Format::Odt => route_odt(request),
        Format::Pdf => route_pdf(request),
        Format::Markdown => route_markdown(request),
        Format::Csv => route_csv(request),
        Format::Xlsx => route_xlsx(request),
        Format::Ods => route_ods(request),
        Format::Json | Format::Yaml | Format::Toml => route_structured_data(request),
        Format::Pptx => route_pptx(request),
        Format::Odp => route_odp(request),
        Format::Html => route_html(request),
    }
}

fn request_theme(request: &ConversionRequest) -> Result<DocumentTheme, MarkoffError> {
    load_document_theme(request.style.as_deref())
}

fn route_odt(request: &ConversionRequest) -> Result<(), MarkoffError> {
    let theme = request_theme(request)?;
    match request.to {
        Format::Markdown => convert_odt_to_markdown(&request.input, &request.output),
        Format::Html | Format::Pdf | Format::Csv | Format::Xlsx | Format::Ods => {
            convert_via_markdown_intermediate(
                |markdown| convert_odt_to_markdown(&request.input, markdown),
                |markdown| match request.to {
                    Format::Html => convert_markdown_to_html(markdown, &request.output, &theme),
                    Format::Pdf => convert_markdown_to_pdf(markdown, &request.output, &theme),
                    Format::Csv => {
                        convert_markdown_to_csv(markdown, &request.output, request.csv_delimiter)
                    }
                    Format::Xlsx => convert_markdown_to_xlsx(markdown, &request.output),
                    Format::Ods => convert_markdown_to_ods(markdown, &request.output),
                    _ => unreachable!("only Markdown-backed ODT targets reach this branch"),
                },
            )
        }
        Format::Json | Format::Yaml | Format::Toml => {
            convert_source_to_structured(request, |markdown| {
                convert_odt_to_markdown(&request.input, markdown)
            })
        }
        _ => not_implemented(request),
    }
}

fn route_docx(request: &ConversionRequest) -> Result<(), MarkoffError> {
    let theme = request_theme(request)?;
    match request.to {
        Format::Markdown => convert_docx_to_markdown(&request.input, &request.output),
        Format::Html => convert_via_markdown_intermediate(
            |markdown| convert_docx_to_markdown(&request.input, markdown),
            |markdown| convert_markdown_to_html(markdown, &request.output, &theme),
        ),
        Format::Csv | Format::Xlsx => convert_via_markdown_intermediate(
            |markdown| convert_docx_to_markdown(&request.input, markdown),
            |markdown| match request.to {
                Format::Csv => {
                    convert_markdown_to_csv(markdown, &request.output, request.csv_delimiter)
                }
                Format::Xlsx => convert_markdown_to_xlsx(markdown, &request.output),
                _ => unreachable!("only CSV and XLSX reach this branch"),
            },
        ),
        Format::Json | Format::Yaml | Format::Toml => {
            convert_source_to_structured(request, |markdown| {
                convert_docx_to_markdown(&request.input, markdown)
            })
        }
        Format::Pdf => convert_via_markdown_intermediate(
            |markdown| convert_docx_to_markdown(&request.input, markdown),
            |markdown| convert_markdown_to_pdf(markdown, &request.output, &theme),
        ),
        _ => not_implemented(request),
    }
}

fn route_pdf(request: &ConversionRequest) -> Result<(), MarkoffError> {
    match request.to {
        Format::Markdown => convert_pdf_to_markdown(&request.input, &request.output),
        Format::Json | Format::Yaml | Format::Toml => {
            convert_source_to_structured(request, |markdown| {
                convert_pdf_to_markdown(&request.input, markdown)
            })
        }
        _ => not_implemented(request),
    }
}

fn route_markdown(request: &ConversionRequest) -> Result<(), MarkoffError> {
    let theme = request_theme(request)?;
    match request.to {
        Format::Docx => convert_markdown_to_docx(&request.input, &request.output, &theme),
        Format::Odt => convert_markdown_to_odt(&request.input, &request.output, &theme),
        Format::Pdf => convert_markdown_to_pdf(&request.input, &request.output, &theme),
        Format::Json | Format::Yaml | Format::Toml => convert_markdown_to_document(
            &request.input,
            &request.output,
            request.to,
            request.tables_only,
        ),
        Format::Csv => {
            convert_markdown_to_csv(&request.input, &request.output, request.csv_delimiter)
        }
        Format::Xlsx => convert_markdown_to_xlsx(&request.input, &request.output),
        Format::Ods => convert_markdown_to_ods(&request.input, &request.output),
        Format::Pptx => convert_markdown_to_pptx(&request.input, &request.output),
        Format::Odp => convert_markdown_to_odp(&request.input, &request.output),
        Format::Html => convert_markdown_to_html(&request.input, &request.output, &theme),
        _ => not_implemented(request),
    }
}

fn route_csv(request: &ConversionRequest) -> Result<(), MarkoffError> {
    let theme = request_theme(request)?;
    match request.to {
        Format::Markdown => {
            convert_csv_to_markdown(&request.input, &request.output, request.csv_delimiter)
        }
        Format::Docx => convert_via_markdown_intermediate(
            |markdown| convert_csv_to_markdown(&request.input, markdown, request.csv_delimiter),
            |markdown| convert_markdown_to_docx(markdown, &request.output, &theme),
        ),
        Format::Odt => convert_via_markdown_intermediate(
            |markdown| convert_csv_to_markdown(&request.input, markdown, request.csv_delimiter),
            |markdown| convert_markdown_to_odt(markdown, &request.output, &theme),
        ),
        Format::Xlsx => convert_data_to_xlsx(
            &request.input,
            &request.output,
            request.from,
            request.csv_delimiter,
        ),
        Format::Ods => convert_via_markdown_intermediate(
            |markdown| convert_csv_to_markdown(&request.input, markdown, request.csv_delimiter),
            |markdown| convert_markdown_to_ods(markdown, &request.output),
        ),
        _ => not_implemented(request),
    }
}

fn route_ods(request: &ConversionRequest) -> Result<(), MarkoffError> {
    let theme = request_theme(request)?;
    match request.to {
        Format::Markdown => convert_ods_to_markdown(&request.input, &request.output),
        Format::Docx | Format::Odt | Format::Html | Format::Pdf => {
            convert_via_markdown_intermediate(
                |markdown| convert_ods_to_markdown(&request.input, markdown),
                |markdown| match request.to {
                    Format::Docx => convert_markdown_to_docx(markdown, &request.output, &theme),
                    Format::Odt => convert_markdown_to_odt(markdown, &request.output, &theme),
                    Format::Html => convert_markdown_to_html(markdown, &request.output, &theme),
                    Format::Pdf => convert_markdown_to_pdf(markdown, &request.output, &theme),
                    _ => unreachable!("only Markdown-backed ODS targets reach this branch"),
                },
            )
        }
        Format::Csv => convert_via_markdown_intermediate(
            |markdown| convert_ods_to_markdown(&request.input, markdown),
            |markdown| convert_markdown_to_csv(markdown, &request.output, request.csv_delimiter),
        ),
        Format::Xlsx => convert_via_markdown_intermediate(
            |markdown| convert_ods_to_markdown(&request.input, markdown),
            |markdown| convert_markdown_to_xlsx(markdown, &request.output),
        ),
        Format::Json | Format::Yaml | Format::Toml => convert_ods_to_data(
            &request.input,
            &request.output,
            request.to,
            request.csv_delimiter,
        ),
        _ => not_implemented(request),
    }
}

fn route_xlsx(request: &ConversionRequest) -> Result<(), MarkoffError> {
    let theme = request_theme(request)?;
    match request.to {
        Format::Markdown => convert_xlsx_to_markdown(&request.input, &request.output),
        Format::Docx => convert_via_markdown_intermediate(
            |markdown| convert_xlsx_to_markdown(&request.input, markdown),
            |markdown| convert_markdown_to_docx(markdown, &request.output, &theme),
        ),
        Format::Ods => convert_via_markdown_intermediate(
            |markdown| convert_xlsx_to_markdown(&request.input, markdown),
            |markdown| convert_markdown_to_ods(markdown, &request.output),
        ),
        Format::Json | Format::Csv | Format::Yaml | Format::Toml => convert_xlsx_to_data(
            &request.input,
            &request.output,
            request.to,
            request.csv_delimiter,
        ),
        _ => not_implemented(request),
    }
}

fn route_structured_data(request: &ConversionRequest) -> Result<(), MarkoffError> {
    let theme = request_theme(request)?;
    match request.to {
        Format::Markdown => convert_document_to_markdown(
            &request.input,
            &request.output,
            request.from,
            request.tables_only,
        ),
        Format::Docx => convert_structured_via_markdown(request, |markdown| {
            convert_markdown_to_docx(markdown, &request.output, &theme)
        }),
        Format::Odt => convert_structured_via_markdown(request, |markdown| {
            convert_markdown_to_odt(markdown, &request.output, &theme)
        }),
        Format::Xlsx => convert_data_to_xlsx(
            &request.input,
            &request.output,
            request.from,
            request.csv_delimiter,
        ),
        Format::Ods => convert_data_to_ods(
            &request.input,
            &request.output,
            request.from,
            request.csv_delimiter,
        ),
        Format::Pptx => convert_structured_via_markdown(request, |markdown| {
            convert_markdown_to_pptx(markdown, &request.output)
        }),
        Format::Odp => convert_structured_via_markdown(request, |markdown| {
            convert_markdown_to_odp(markdown, &request.output)
        }),
        Format::Pdf => convert_structured_data_to_pdf(
            &request.input,
            &request.output,
            request.from,
            request.tables_only,
            &theme,
        ),
        Format::Html => convert_structured_via_markdown(request, |markdown| {
            convert_markdown_to_html(markdown, &request.output, &theme)
        }),
        Format::Json | Format::Yaml | Format::Toml if request.from != request.to => {
            convert_structured_data_format(
                &request.input,
                &request.output,
                request.from,
                request.to,
            )
        }
        _ => not_implemented(request),
    }
}

fn route_odp(request: &ConversionRequest) -> Result<(), MarkoffError> {
    match request.to {
        Format::Markdown => convert_odp_to_markdown(&request.input, &request.output),
        Format::Json | Format::Yaml | Format::Toml => {
            convert_source_to_structured(request, |markdown| {
                convert_odp_to_markdown(&request.input, markdown)
            })
        }
        _ => not_implemented(request),
    }
}

fn route_pptx(request: &ConversionRequest) -> Result<(), MarkoffError> {
    match request.to {
        Format::Markdown => convert_pptx_to_markdown(&request.input, &request.output),
        Format::Json | Format::Yaml | Format::Toml => {
            convert_source_to_structured(request, |markdown| {
                convert_pptx_to_markdown(&request.input, markdown)
            })
        }
        _ => not_implemented(request),
    }
}

fn route_html(request: &ConversionRequest) -> Result<(), MarkoffError> {
    match request.to {
        Format::Markdown => convert_html_to_markdown(&request.input, &request.output),
        Format::Json | Format::Yaml | Format::Toml => {
            convert_source_to_structured(request, |markdown| {
                convert_html_to_markdown(&request.input, markdown)
            })
        }
        _ => not_implemented(request),
    }
}

fn convert_source_to_structured(
    request: &ConversionRequest,
    source_to_markdown: impl FnOnce(&Path) -> Result<(), MarkoffError>,
) -> Result<(), MarkoffError> {
    convert_via_markdown_intermediate(source_to_markdown, |markdown| {
        convert_markdown_to_document(markdown, &request.output, request.to, request.tables_only)
    })
}

fn convert_structured_via_markdown(
    request: &ConversionRequest,
    markdown_to_target: impl FnOnce(&Path) -> Result<(), MarkoffError>,
) -> Result<(), MarkoffError> {
    convert_via_markdown_intermediate(
        |markdown| {
            convert_document_to_markdown(
                &request.input,
                markdown,
                request.from,
                request.tables_only,
            )
        },
        markdown_to_target,
    )
}

fn not_implemented(request: &ConversionRequest) -> Result<(), MarkoffError> {
    Err(MarkoffError::NotImplemented {
        from: request.from,
        to: request.to,
    })
}

/// Reports whether table-only conversion applies to a format pair.
///
/// The option is available between JSON/YAML/TOML and Markdown, DOCX, ODT,
/// PPTX, ODP, or HTML. PDF is supported as both a source and destination format.
///
/// # Examples
///
/// ```
/// use markoff_core::{Format, supports_tables_only};
///
/// assert!(supports_tables_only(Format::Docx, Format::Json));
/// assert!(supports_tables_only(Format::Json, Format::Pdf));
/// ```
#[must_use]
pub fn supports_tables_only(from: Format, to: Format) -> bool {
    let structured = |format| matches!(format, Format::Json | Format::Yaml | Format::Toml);
    let document_source = |format| {
        matches!(
            format,
            Format::Markdown
                | Format::Docx
                | Format::Odt
                | Format::Pdf
                | Format::Pptx
                | Format::Odp
                | Format::Html
        )
    };
    let document_target = |format| {
        matches!(
            format,
            Format::Markdown
                | Format::Docx
                | Format::Odt
                | Format::Pdf
                | Format::Pptx
                | Format::Odp
                | Format::Html
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
    let markdown = intermediate_path("md")?;
    let result = to_markdown(&markdown).and_then(|()| from_markdown(&markdown));
    remove_intermediate_markdown(&markdown);
    result
}

fn intermediate_path(extension: &str) -> Result<PathBuf, MarkoffError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| std::io::Error::other(error.to_string()))?
        .as_nanos();
    loop {
        let sequence = INTERMEDIATE_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "markoff_intermediate_{}_{}_{}",
            std::process::id(),
            nanos,
            sequence
        ));
        match std::fs::create_dir(&directory) {
            Ok(()) => return Ok(directory.join(format!("intermediate.{extension}"))),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

/// Removes an intermediate file's own temp directory, including any `image`
/// folder that DOCX image extraction may have created beside it. Each
/// intermediate path lives in its own directory (see `intermediate_path`), so
/// this cannot affect unrelated concurrent conversions.
fn remove_intermediate_markdown(markdown: &Path) {
    match markdown.parent() {
        Some(parent) => {
            if let Err(error) = std::fs::remove_dir_all(parent) {
                tracing::warn!(path = %parent.display(), %error, "failed to remove intermediate directory");
            }
        }
        None => {
            if let Err(error) = std::fs::remove_file(markdown) {
                tracing::warn!(path = %markdown.display(), %error, "failed to remove intermediate file");
            }
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
        style: None,
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
