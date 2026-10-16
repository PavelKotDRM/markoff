use std::collections::BTreeMap;
use std::path::Path;

use crate::tables::{markdown_table_from_rows, parse_markdown_tables};
use crate::{Format, MarkoffError};

mod reader;
mod writer;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CellValue {
    Empty,
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
}

impl CellValue {
    pub(crate) fn as_text(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Integer(value) => value.to_string(),
            Self::Float(value) => value.to_string(),
            Self::String(value) => value.clone(),
            Self::Boolean(value) => value.to_string(),
        }
    }
}

fn cell_value_from_float(value: f64) -> CellValue {
    if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        CellValue::Integer(value as i64)
    } else {
        CellValue::Float(value)
    }
}

pub(crate) fn read_xlsx_sheets(
    input: &Path,
) -> Result<BTreeMap<String, Vec<Vec<String>>>, MarkoffError> {
    Ok(read_xlsx_value_sheets(input)?
        .into_iter()
        .map(|(name, rows)| {
            (
                name,
                rows.into_iter()
                    .map(|row| row.iter().map(CellValue::as_text).collect())
                    .collect(),
            )
        })
        .collect())
}

pub(crate) fn read_xlsx_value_sheets(
    input: &Path,
) -> Result<BTreeMap<String, Vec<Vec<CellValue>>>, MarkoffError> {
    reader::read_sheets(input)
}

pub(crate) fn write_xlsx_sheets(
    output: &Path,
    sheets: &BTreeMap<String, Vec<Vec<String>>>,
) -> Result<(), MarkoffError> {
    let values = sheets
        .iter()
        .map(|(name, rows)| {
            (
                name.clone(),
                rows.iter()
                    .map(|row| row.iter().cloned().map(CellValue::String).collect())
                    .collect(),
            )
        })
        .collect();
    write_xlsx_value_sheets(output, &values)
}

pub(crate) fn write_xlsx_value_sheets(
    output: &Path,
    sheets: &BTreeMap<String, Vec<Vec<CellValue>>>,
) -> Result<(), MarkoffError> {
    writer::write_sheets(output, sheets)
}

pub(crate) fn convert_xlsx_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let sheets = read_xlsx_sheets(input)?;
    let markdown = sheets
        .iter()
        .map(|(name, rows)| format!("## {name}\n\n{}", markdown_table_from_rows(rows)))
        .collect::<Vec<_>>()
        .join("\n\n");
    std::fs::write(output, markdown)?;
    Ok(())
}

pub(crate) fn convert_markdown_to_xlsx(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let sheets = parse_markdown_tables(&source)?;
    if sheets.is_empty() {
        return Err(MarkoffError::NotImplemented {
            from: Format::Markdown,
            to: Format::Xlsx,
        });
    }
    write_xlsx_sheets(output, &sheets)
}
