use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Supported document and data formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    /// Microsoft Word document format.
    Docx,
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
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Docx => "docx",
            Self::Markdown => "md",
            Self::Xlsx => "xlsx",
            Self::Json => "json",
            Self::Csv => "csv",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Pptx => "pptx",
        };
        f.write_str(value)
    }
}

impl Format {
    /// Parses a format from a file extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension is not recognized by the converter.
    pub fn from_extension(extension: &str) -> Result<Self, MarkoffError> {
        match extension.trim().to_ascii_lowercase().as_str() {
            "docx" => Ok(Self::Docx),
            "md" | "markdown" => Ok(Self::Markdown),
            "xlsx" | "xlsm" => Ok(Self::Xlsx),
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            "yaml" | "yml" => Ok(Self::Yaml),
            "toml" => Ok(Self::Toml),
            "pptx" => Ok(Self::Pptx),
            other => Err(MarkoffError::UnsupportedFormat {
                format: other.to_string(),
            }),
        }
    }

    /// Returns whether the format can be safely read from or written to a text stream.
    #[must_use]
    pub const fn is_text(self) -> bool {
        matches!(
            self,
            Self::Markdown | Self::Json | Self::Csv | Self::Yaml | Self::Toml
        )
    }
}

/// Request describing a single conversion operation.
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
}

/// Error type returned by the conversion engine.
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
    /// The output directory could not be created.
    #[error("failed to create output directory: {path}")]
    OutputDirectory {
        /// The output path whose parent directory could not be created.
        path: String,
    },
    /// The requested conversion is not yet implemented in the scaffold.
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

/// Detects a format from a file path.
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
