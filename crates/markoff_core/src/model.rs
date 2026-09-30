use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// A document or data format understood by the conversion engine.
///
/// File extensions are parsed case-insensitively by [`Format::from_extension`].
/// Extensions are notated without a leading dot. Markdown accepts `md` and
/// `markdown`; Excel accepts `xlsx` and `xlsm`; YAML accepts `yaml` and `yml`;
/// and HTML accepts `html` and `htm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    /// Microsoft Word document format.
    Docx,
    /// Portable Document Format.
    Pdf,
    /// Markdown text format.
    Markdown,
    /// Microsoft Excel workbook format.
    Xlsx,
    /// JavaScript Object Notation.
    Json,
    /// Comma-separated values.
    Csv,
    /// YAML data format.
    Yaml,
    /// TOML data format.
    Toml,
    /// Microsoft PowerPoint presentation format.
    Pptx,
    /// HyperText Markup Language.
    Html,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Docx => "docx",
            Self::Pdf => "pdf",
            Self::Markdown => "md",
            Self::Xlsx => "xlsx",
            Self::Json => "json",
            Self::Csv => "csv",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Pptx => "pptx",
            Self::Html => "html",
        };
        f.write_str(value)
    }
}

impl Format {
    /// Parses a format from a file extension, with or without surrounding
    /// whitespace.
    ///
    /// Matching is case-insensitive. Pass the extension itself, not a full
    /// file name or a leading dot.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension is not recognized by the converter.
    pub fn from_extension(extension: &str) -> Result<Self, MarkoffError> {
        match extension.trim().to_ascii_lowercase().as_str() {
            "docx" => Ok(Self::Docx),
            "pdf" => Ok(Self::Pdf),
            "md" | "markdown" => Ok(Self::Markdown),
            "xlsx" | "xlsm" => Ok(Self::Xlsx),
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            "yaml" | "yml" => Ok(Self::Yaml),
            "toml" => Ok(Self::Toml),
            "pptx" => Ok(Self::Pptx),
            "html" | "htm" => Ok(Self::Html),
            other => Err(MarkoffError::UnsupportedFormat {
                format: other.to_string(),
            }),
        }
    }

    /// Returns whether the format can be read from or written to a text stream.
    ///
    /// Binary office formats and PDF return `false`.
    #[must_use]
    pub const fn is_text(self) -> bool {
        matches!(
            self,
            Self::Markdown | Self::Json | Self::Csv | Self::Yaml | Self::Toml | Self::Html
        )
    }
}

/// Options and paths for a single conversion operation.
///
/// Use [`convert_document`](crate::convert_document) to honor `overwrite` and
/// `tables_only`. The CSV delimiter is supplied as one byte and defaults to
/// a comma (`,`).
///
/// # Examples
///
/// ```
/// use markoff_core::{ConversionRequest, Format};
///
/// let request = ConversionRequest {
///     input: "report.md".into(),
///     output: "report.html".into(),
///     from: Format::Markdown,
///     to: Format::Html,
///     overwrite: false,
///     csv_delimiter: b',',
///     tables_only: false,
/// };
/// assert_eq!(request.to, Format::Html);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversionRequest {
    /// Source file path.
    pub input: PathBuf,
    /// Destination file path.
    pub output: PathBuf,
    /// Source format.
    pub from: Format,
    /// Target format.
    pub to: Format,
    /// Whether an existing file at `output` may be overwritten.
    pub overwrite: bool,
    /// Field delimiter byte used when reading or writing CSV. Defaults to a
    /// comma (`,`); common alternatives include `;` and tab (`\t`).
    #[serde(default = "default_csv_delimiter")]
    pub csv_delimiter: u8,
    /// Whether to retain only table blocks when converting between documents
    /// and the JSON, YAML, or TOML document schema.
    #[serde(default)]
    pub tables_only: bool,
}

/// The default CSV field delimiter (`,`).
#[must_use]
pub const fn default_csv_delimiter() -> u8 {
    b','
}

/// Error returned when a conversion cannot be completed.
///
/// The variants distinguish invalid paths and options from unsupported
/// format pairs, output conflicts, and underlying I/O or parsing failures.
#[derive(Debug, Error)]
pub enum MarkoffError {
    /// A conversion format was not recognized.
    #[error("unsupported format: {format}")]
    UnsupportedFormat {
        /// The unsupported format string supplied by the caller.
        format: String,
    },
    /// The input path is invalid for the requested conversion.
    #[error("invalid input path: {path}")]
    InvalidInput {
        /// The invalid input path.
        path: String,
    },
    /// A conversion option is not valid for the requested format pair.
    #[error("invalid conversion option: {message}")]
    InvalidOption {
        /// Explanation of the invalid option.
        message: String,
    },
    /// The output directory could not be created.
    #[error("failed to create output directory: {path}")]
    OutputDirectory {
        /// The output path whose parent directory could not be created.
        path: String,
    },
    /// The destination file already exists and overwriting was not requested.
    #[error("output file already exists: {path}")]
    OutputExists {
        /// The existing output path.
        path: String,
    },
    /// The requested source and destination format pair is not supported.
    #[error("conversion from {from} to {to} is not implemented yet")]
    NotImplemented {
        /// Source format requested for conversion.
        from: Format,
        /// Target format requested for conversion.
        to: Format,
    },
    /// General I/O-related errors.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Detects a format from the final extension of a file path.
///
/// The extension is interpreted case-insensitively. A path without a known
/// extension returns [`MarkoffError::UnsupportedFormat`].
///
/// # Errors
///
/// Returns `MarkoffError::UnsupportedFormat` when the extension is not known.
///
/// # Examples
///
/// ```rust
/// use markoff_core::{detect_format, Format};
/// assert!(matches!(detect_format("example.xlsx"), Ok(Format::Xlsx)));
/// ```
pub fn detect_format<P: AsRef<Path>>(path: P) -> Result<Format, MarkoffError> {
    let extension = path
        .as_ref()
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("");
    Format::from_extension(extension)
}
