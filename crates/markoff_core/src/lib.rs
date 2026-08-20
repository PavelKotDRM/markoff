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

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
        let s = match self {
            Self::Docx => "docx",
            Self::Markdown => "md",
            Self::Xlsx => "xlsx",
            Self::Json => "json",
            Self::Csv => "csv",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Pptx => "pptx",
        };
        f.write_str(s)
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
    let path = path.as_ref();
    let ext = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("");

    Format::from_extension(ext)
}

fn convert_json_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let value: serde_json::Value = serde_json::from_str(&source)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let pretty = serde_json::to_string_pretty(&value)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let markdown = format!("# JSON Document\n\n```json\n{pretty}\n```\n");
    std::fs::write(output, markdown)?;
    Ok(())
}

fn convert_markdown_to_json(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let title = source
        .lines()
        .find(|line| line.trim_start().starts_with('#'))
        .map(|line| line.trim().trim_start_matches('#').trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("untitled");

    let content = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    let payload = serde_json::json!({
        "title": title,
        "content": content,
    });

    let rendered = serde_json::to_string_pretty(&payload)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    std::fs::write(output, rendered)?;
    Ok(())
}

fn parse_markdown_table(markdown: &str) -> Vec<Vec<String>> {
    let rows = markdown
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| line.starts_with('|') && line.ends_with('|'))
        .collect::<Vec<_>>();

    if rows.len() < 3 {
        return Vec::new();
    }

    let header = rows[0]
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect::<Vec<_>>();

    let body = rows[1..]
        .iter()
        .skip_while(|row| row.contains("---"))
        .map(|row| {
            row.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|row| !row.is_empty() && row.len() == header.len())
        .collect::<Vec<_>>();

    let mut result = Vec::new();
    if !header.is_empty() {
        result.push(header);
    }
    result.extend(body);
    result
}

fn parse_markdown_tables(markdown: &str) -> BTreeMap<String, Vec<Vec<String>>> {
    let mut tables = BTreeMap::new();
    let mut sheet_name = "Sheet1".to_string();
    let mut table_lines = Vec::new();

    for line in markdown.lines() {
        if let Some(name) = line.trim().strip_prefix("## ") {
            if !table_lines.is_empty() {
                let rows = parse_markdown_table(&table_lines.join("\n"));
                if !rows.is_empty() {
                    tables.insert(sheet_name, rows);
                }
                table_lines.clear();
            }
            sheet_name = name.trim().to_string();
        } else if line.trim().starts_with('|') && line.trim().ends_with('|') {
            table_lines.push(line.to_string());
        }
    }

    if !table_lines.is_empty() {
        let rows = parse_markdown_table(&table_lines.join("\n"));
        if !rows.is_empty() {
            tables.insert(sheet_name, rows);
        }
    }
    tables
}

fn markdown_table_from_rows(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let format_row = |row: &[String]| format!("| {} |", row.join(" | "));
    let separator = (0..rows[0].len())
        .map(|_| "---")
        .collect::<Vec<_>>()
        .join(" | ");

    let mut output = Vec::new();
    output.push(format_row(&rows[0]));
    output.push(format!("| {} |", separator));

    for row in rows.iter().skip(1) {
        output.push(format_row(row));
    }

    output.join("\n")
}

fn convert_csv_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let mut rdr = csv::Reader::from_reader(source.as_bytes());

    let headers = rdr
        .headers()
        .map(|row| row.iter().map(str::to_string).collect::<Vec<_>>())
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?
        .to_vec();

    let mut rows = vec![headers];
    for record in rdr.records() {
        let row = record
            .map(|row| row.iter().map(str::to_string).collect::<Vec<_>>())
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        rows.push(row);
    }

    let markdown = markdown_table_from_rows(&rows);
    std::fs::write(output, markdown)?;
    Ok(())
}

fn convert_markdown_to_csv(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let rows = parse_markdown_table(&source);

    if rows.is_empty() {
        return Err(MarkoffError::NotImplemented {
            from: Format::Markdown,
            to: Format::Csv,
        });
    }

    let mut out = Vec::new();
    for row in &rows {
        out.push(row.join(","));
    }
    std::fs::write(output, out.join("\n"))?;
    Ok(())
}

fn read_xlsx_sheets(input: &Path) -> Result<BTreeMap<String, Vec<Vec<String>>>, MarkoffError> {
    use calamine::{Reader, open_workbook_auto};

    let mut workbook = open_workbook_auto(input)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let mut sheets = BTreeMap::new();

    for name in workbook.sheet_names().to_owned() {
        let range = workbook
            .worksheet_range(&name)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        let rows = range
            .rows()
            .map(|row| row.iter().map(ToString::to_string).collect())
            .collect();
        sheets.insert(name, rows);
    }

    Ok(sheets)
}

fn write_xlsx_sheets(
    output: &Path,
    sheets: &BTreeMap<String, Vec<Vec<String>>>,
) -> Result<(), MarkoffError> {
    let mut workbook = rust_xlsxwriter::Workbook::new();

    for (name, rows) in sheets {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name(name)
            .map_err(|err| std::io::Error::other(err.to_string()))?;

        for (row_index, row) in rows.iter().enumerate() {
            for (column_index, value) in row.iter().enumerate() {
                worksheet
                    .write_string(row_index as u32, column_index as u16, value)
                    .map_err(|err| std::io::Error::other(err.to_string()))?;
            }
        }

        if !rows.is_empty() {
            worksheet
                .set_freeze_panes(1, 0)
                .map_err(|err| std::io::Error::other(err.to_string()))?;
            let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
            for column_index in 0..column_count {
                let width = rows
                    .iter()
                    .filter_map(|row| row.get(column_index))
                    .map(|value| value.chars().count())
                    .max()
                    .unwrap_or(0)
                    .clamp(8, 40) as f64;
                worksheet
                    .set_column_width(column_index as u16, width)
                    .map_err(|err| std::io::Error::other(err.to_string()))?;
            }
        }
    }

    workbook
        .save(output)
        .map_err(|err| std::io::Error::other(err.to_string()))?;
    Ok(())
}

fn convert_xlsx_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let sheets = read_xlsx_sheets(input)?;
    let markdown = sheets
        .iter()
        .map(|(name, rows)| format!("## {name}\n\n{}", markdown_table_from_rows(rows)))
        .collect::<Vec<_>>()
        .join("\n\n");
    std::fs::write(output, markdown)?;
    Ok(())
}

fn convert_markdown_to_xlsx(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let sheets = parse_markdown_tables(&source);
    if sheets.is_empty() {
        return Err(MarkoffError::NotImplemented {
            from: Format::Markdown,
            to: Format::Xlsx,
        });
    }
    write_xlsx_sheets(output, &sheets)
}

fn rows_from_json(value: &serde_json::Value) -> Result<Vec<Vec<String>>, MarkoffError> {
    let objects = value.as_array().ok_or_else(|| MarkoffError::InvalidInput {
        path: "structured data must be an array of objects".to_string(),
    })?;
    let mut headers = Vec::new();
    for object in objects {
        let object = object
            .as_object()
            .ok_or_else(|| MarkoffError::InvalidInput {
                path: "structured data must contain only objects".to_string(),
            })?;
        for key in object.keys() {
            if !headers.contains(key) {
                headers.push(key.clone());
            }
        }
    }

    let mut rows = vec![headers.clone()];
    for object in objects {
        let object = object.as_object().expect("validated above");
        rows.push(
            headers
                .iter()
                .map(|key| object.get(key).map_or(String::new(), json_cell_to_string))
                .collect(),
        );
    }
    Ok(rows)
}

fn json_cell_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn json_from_rows(rows: &[Vec<String>]) -> serde_json::Value {
    let Some(headers) = rows.first() else {
        return serde_json::Value::Array(Vec::new());
    };
    serde_json::Value::Array(
        rows.iter()
            .skip(1)
            .map(|row| {
                let object = headers
                    .iter()
                    .enumerate()
                    .map(|(index, header)| {
                        let value = row.get(index).cloned().unwrap_or_default();
                        (header.clone(), infer_json_cell(&value))
                    })
                    .collect();
                serde_json::Value::Object(object)
            })
            .collect(),
    )
}

fn infer_json_cell(value: &str) -> serde_json::Value {
    if let Ok(value) = value.parse::<bool>() {
        return serde_json::Value::Bool(value);
    }
    if let Ok(value) = value.parse::<i64>() {
        return serde_json::Value::Number(value.into());
    }
    if let Ok(value) = value.parse::<f64>()
        && let Some(value) = serde_json::Number::from_f64(value)
    {
        return serde_json::Value::Number(value);
    }
    serde_json::Value::String(value.to_string())
}

fn convert_data_to_xlsx(input: &Path, output: &Path, format: Format) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let rows = match format {
        Format::Json => rows_from_json(&serde_json::from_str(&source).map_err(invalid_data)?)?,
        Format::Yaml => {
            let value: serde_json::Value = serde_yaml::from_str(&source).map_err(invalid_data)?;
            rows_from_json(&value)?
        }
        Format::Toml => {
            let value: toml::Value = source.parse().map_err(invalid_data)?;
            let value = serde_json::to_value(value).map_err(invalid_data)?;
            rows_from_json(&value)?
        }
        Format::Csv => {
            let mut reader = csv::Reader::from_reader(source.as_bytes());
            let headers = reader
                .headers()
                .map_err(invalid_data)?
                .iter()
                .map(str::to_string)
                .collect();
            let mut rows = vec![headers];
            for record in reader.records() {
                rows.push(
                    record
                        .map_err(invalid_data)?
                        .iter()
                        .map(str::to_string)
                        .collect(),
                );
            }
            rows
        }
        _ => unreachable!("only structured data formats use this helper"),
    };
    let mut sheets = BTreeMap::new();
    sheets.insert("Sheet1".to_string(), rows);
    write_xlsx_sheets(output, &sheets)
}

fn invalid_data(error: impl std::error::Error + Send + Sync + 'static) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
}

fn convert_xlsx_to_data(input: &Path, output: &Path, format: Format) -> Result<(), MarkoffError> {
    let sheets = read_xlsx_sheets(input)?;
    let value = if sheets.len() == 1 {
        json_from_rows(sheets.values().next().expect("one sheet exists"))
    } else {
        serde_json::Value::Object(
            sheets
                .iter()
                .map(|(name, rows)| (name.clone(), json_from_rows(rows)))
                .collect(),
        )
    };

    match format {
        Format::Json => std::fs::write(
            output,
            serde_json::to_string_pretty(&value).map_err(invalid_data)?,
        )?,
        Format::Yaml => {
            std::fs::write(output, serde_yaml::to_string(&value).map_err(invalid_data)?)?
        }
        Format::Toml => std::fs::write(
            output,
            toml::to_string_pretty(&value).map_err(invalid_data)?,
        )?,
        Format::Csv => {
            let rows = sheets
                .values()
                .next()
                .ok_or_else(|| MarkoffError::InvalidInput {
                    path: input.to_string_lossy().to_string(),
                })?;
            let mut writer = csv::Writer::from_writer(Vec::new());
            for row in rows {
                writer.write_record(row).map_err(invalid_data)?;
            }
            let bytes = writer
                .into_inner()
                .map_err(|err| invalid_data(err.into_error()))?;
            std::fs::write(output, bytes)?;
        }
        _ => unreachable!("only structured data formats use this helper"),
    }
    Ok(())
}

fn convert_yaml_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let rendered = source.trim();
    std::fs::write(
        output,
        format!("# YAML Document\n\n```yaml\n{rendered}\n```\n"),
    )?;
    Ok(())
}

fn convert_toml_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let value: toml::Value = source
        .parse()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let rendered = value.to_string();
    std::fs::write(
        output,
        format!("# TOML Document\n\n```toml\n{rendered}\n```\n"),
    )?;
    Ok(())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn markdown_inline_to_docx_runs(value: &str) -> String {
    let mut runs = String::new();
    let mut remaining = value;
    let mut bold = false;
    let mut italic = false;

    while !remaining.is_empty() {
        let delimiter = if remaining.starts_with("**") {
            bold = !bold;
            remaining = &remaining[2..];
            continue;
        } else if remaining.starts_with('*') {
            italic = !italic;
            remaining = &remaining[1..];
            continue;
        } else {
            remaining.char_indices().find_map(|(index, _)| {
                (remaining[index..].starts_with("**") || remaining[index..].starts_with('*'))
                    .then_some(index)
            })
        };
        let length = delimiter.unwrap_or(remaining.len());
        let (text, rest) = remaining.split_at(length);
        if !text.is_empty() {
            let properties = match (bold, italic) {
                (true, true) => "<w:rPr><w:b/><w:i/></w:rPr>",
                (true, false) => "<w:rPr><w:b/></w:rPr>",
                (false, true) => "<w:rPr><w:i/></w:rPr>",
                (false, false) => "",
            };
            runs.push_str(&format!(
                "<w:r>{properties}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
                xml_escape(text)
            ));
        }
        remaining = rest;
    }
    runs
}

fn markdown_list_item(line: &str) -> Option<(u32, &str)> {
    let value = line.trim_start();
    if let Some(content) = value
        .strip_prefix("- ")
        .or_else(|| value.strip_prefix("* "))
    {
        return Some((1, content));
    }

    let (number, content) = value.split_once(". ")?;
    number
        .chars()
        .all(|character| character.is_ascii_digit())
        .then_some((2, content))
}

fn pageref_target(instruction: &str) -> Option<String> {
    let mut tokens = instruction.split_whitespace();
    while let Some(token) = tokens.next() {
        if token.eq_ignore_ascii_case("PAGEREF") {
            return tokens.next().map(str::to_string);
        }
    }
    None
}

fn convert_markdown_to_docx(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let source = std::fs::read_to_string(input)?;
    let body = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let heading_level = line
                .chars()
                .take_while(|character| *character == '#')
                .count();
            let (style, content) = if heading_level > 0
                && heading_level <= 6
                && line.as_bytes().get(heading_level) == Some(&b' ')
            {
                (
                    format!("<w:pPr><w:pStyle w:val=\"Heading{heading_level}\"/></w:pPr>"),
                    &line[heading_level + 1..],
                )
            } else if let Some((numbering_id, content)) = markdown_list_item(line) {
                (
                    format!(
                        "<w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"{numbering_id}\"/></w:numPr></w:pPr>"
                    ),
                    content,
                )
            } else {
                (String::new(), line)
            };
            format!(
                "<w:p>{style}{}</w:p>",
                markdown_inline_to_docx_runs(content)
            )
        })
        .collect::<String>();
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr/></w:body></w:document>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    let content_types = content_types.replace(
        "</Types>",
        "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/></Types>",
    );
    let relationships = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    let document_relationships = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/></Relationships>";
    let numbering = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"•\"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId=\"1\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/></w:lvl></w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num></w:numbering>";

    let file = std::fs::File::create(output)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    archive
        .start_file("[Content_Types].xml", options)
        .map_err(invalid_data)?;
    archive.write_all(content_types.as_bytes())?;
    archive
        .start_file("_rels/.rels", options)
        .map_err(invalid_data)?;
    archive.write_all(relationships.as_bytes())?;
    archive
        .start_file("word/_rels/document.xml.rels", options)
        .map_err(invalid_data)?;
    archive.write_all(document_relationships.as_bytes())?;
    archive
        .start_file("word/document.xml", options)
        .map_err(invalid_data)?;
    archive.write_all(document.as_bytes())?;
    archive
        .start_file("word/numbering.xml", options)
        .map_err(invalid_data)?;
    archive.write_all(numbering.as_bytes())?;
    archive.finish().map_err(invalid_data)?;
    Ok(())
}

fn convert_docx_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    use std::io::Read;

    let file = std::fs::File::open(input)?;
    let mut archive = zip::ZipArchive::new(file).map_err(invalid_data)?;
    let mut document = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(invalid_data)?
        .read_to_string(&mut document)?;

    let mut reader = Reader::from_str(&document);
    reader.config_mut().trim_text(false);
    let mut markdown = Vec::new();
    let mut paragraph = String::new();
    let mut run = String::new();
    let mut heading_level = None;
    let mut list_numbering_id = None;
    let mut bookmarks = Vec::new();
    let mut field_instruction = String::new();
    let mut page_reference = None;
    let mut in_field_result = false;
    let mut in_instruction_text = false;
    let mut bold = false;
    let mut italic = false;
    let mut in_paragraph = false;

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(event) | Event::Empty(event) => match event.local_name().as_ref() {
                b"p" => {
                    in_paragraph = true;
                    paragraph.clear();
                    heading_level = None;
                    list_numbering_id = None;
                    bookmarks.clear();
                    field_instruction.clear();
                    page_reference = None;
                    in_field_result = false;
                }
                b"r" => run.clear(),
                b"b" => bold = true,
                b"i" => italic = true,
                b"instrText" => in_instruction_text = true,
                b"bookmarkStart" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"name" {
                            bookmarks.push(
                                attribute
                                    .decode_and_unescape_value(reader.decoder())
                                    .map_err(invalid_data)?
                                    .into_owned(),
                            );
                        }
                    }
                }
                b"fldChar" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"fldCharType" {
                            let field_type = attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(invalid_data)?;
                            match field_type.as_ref() {
                                "begin" => {
                                    field_instruction.clear();
                                    page_reference = None;
                                    in_field_result = false;
                                }
                                "separate" => {
                                    page_reference = pageref_target(&field_instruction);
                                    in_field_result = true;
                                }
                                "end" => in_field_result = false,
                                _ => {}
                            }
                        }
                    }
                }
                b"fldSimple" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"instr" {
                            let instruction = attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(invalid_data)?;
                            page_reference = pageref_target(&instruction);
                            in_field_result = true;
                        }
                    }
                }
                b"pStyle" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"val" {
                            let value = attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(invalid_data)?
                                .into_owned();
                            heading_level = value
                                .strip_prefix("Heading")
                                .and_then(|level| level.parse::<usize>().ok())
                                .filter(|level| (1..=6).contains(level));
                        }
                    }
                }
                b"numId" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"val" {
                            list_numbering_id = Some(
                                attribute
                                    .decode_and_unescape_value(reader.decoder())
                                    .map_err(invalid_data)?
                                    .into_owned(),
                            );
                        }
                    }
                }
                _ => {}
            },
            Event::Text(event) if in_paragraph => {
                let text = event.decode().map_err(invalid_data)?;
                if in_instruction_text {
                    field_instruction.push_str(&text);
                } else {
                    run.push_str(&text);
                }
            }
            Event::End(event) => match event.local_name().as_ref() {
                b"instrText" => in_instruction_text = false,
                b"fldSimple" => in_field_result = false,
                b"r" => {
                    if !run.is_empty() {
                        let mut rendered = run.clone();
                        if italic {
                            rendered = format!("*{rendered}*");
                        }
                        if bold {
                            rendered = format!("**{rendered}**");
                        }
                        if in_field_result && let Some(target) = page_reference.as_deref() {
                            rendered = format!("[{rendered}](#{target})");
                        }
                        paragraph.push_str(&rendered);
                    }
                    bold = false;
                    italic = false;
                }
                b"p" => {
                    if !paragraph.is_empty() {
                        let anchors = bookmarks
                            .iter()
                            .map(|bookmark| format!("<a id=\"{bookmark}\"></a>"))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let prefix = heading_level.map_or_else(
                            || match list_numbering_id.as_deref() {
                                Some("1") => "- ".to_string(),
                                Some(_) => "1. ".to_string(),
                                None => String::new(),
                            },
                            |level| "#".repeat(level) + " ",
                        );
                        let separator = if anchors.is_empty() { "" } else { "\n" };
                        markdown.push(format!("{anchors}{separator}{prefix}{paragraph}"));
                    }
                    in_paragraph = false;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    std::fs::write(output, markdown.join("\n\n") + "\n")?;
    Ok(())
}

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
            convert_json_to_markdown(&request.input, &request.output)
        }
        (Format::Markdown, Format::Json) => {
            convert_markdown_to_json(&request.input, &request.output)
        }
        (Format::Csv, Format::Markdown) => convert_csv_to_markdown(&request.input, &request.output),
        (Format::Markdown, Format::Csv) => convert_markdown_to_csv(&request.input, &request.output),
        (Format::Yaml, Format::Markdown) => {
            convert_yaml_to_markdown(&request.input, &request.output)
        }
        (Format::Toml, Format::Markdown) => {
            convert_toml_to_markdown(&request.input, &request.output)
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
    use super::{Format, convert_file, detect_format, read_xlsx_sheets, write_xlsx_sheets};
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
