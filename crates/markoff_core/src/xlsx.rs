use std::collections::BTreeMap;
use std::path::Path;

use crate::tables::{markdown_table_from_rows, parse_markdown_tables};
use crate::{Format, MarkoffError};

#[derive(Clone, Debug)]
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
    use calamine::{Reader, open_workbook_auto};

    let mut workbook = open_workbook_auto(input)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut sheets = BTreeMap::new();

    for name in workbook.sheet_names().to_owned() {
        let range = workbook
            .worksheet_range(&name)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let rows = range
            .rows()
            .map(|row| {
                row.iter()
                    .map(|cell| match cell {
                        calamine::Data::Empty => CellValue::Empty,
                        calamine::Data::Int(value) => CellValue::Integer(*value),
                        calamine::Data::Float(value) => cell_value_from_float(*value),
                        calamine::Data::String(value) => CellValue::String(value.clone()),
                        calamine::Data::Bool(value) => CellValue::Boolean(*value),
                        _ => CellValue::String(cell.to_string()),
                    })
                    .collect()
            })
            .collect();
        sheets.insert(name, rows);
    }

    Ok(sheets)
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
    let mut workbook = rust_xlsxwriter::Workbook::new();

    for (name, rows) in sheets {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name(name)
            .map_err(|error| std::io::Error::other(error.to_string()))?;

        for (row_index, row) in rows.iter().enumerate() {
            for (column_index, value) in row.iter().enumerate() {
                let row_index = row_index as u32;
                let column_index = column_index as u16;
                match value {
                    CellValue::Empty => {}
                    CellValue::Integer(value) => {
                        worksheet
                            .write_number(row_index, column_index, *value as f64)
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                    }
                    CellValue::Float(value) => {
                        worksheet
                            .write_number(row_index, column_index, *value)
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                    }
                    CellValue::String(value) => {
                        worksheet
                            .write_string(row_index, column_index, value)
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                    }
                    CellValue::Boolean(value) => {
                        worksheet
                            .write_boolean(row_index, column_index, *value)
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                    }
                };
            }
        }

        if !rows.is_empty() {
            worksheet
                .set_freeze_panes(1, 0)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
            for column_index in 0..column_count {
                let width = rows
                    .iter()
                    .filter_map(|row| row.get(column_index))
                    .map(|value| value.as_text().chars().count())
                    .max()
                    .unwrap_or(0)
                    .clamp(8, 40) as f64;
                worksheet
                    .set_column_width(column_index as u16, width)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
            }
        }
    }

    workbook
        .save(output)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(())
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
    let sheets = parse_markdown_tables(&source);
    if sheets.is_empty() {
        return Err(MarkoffError::NotImplemented {
            from: Format::Markdown,
            to: Format::Xlsx,
        });
    }
    write_xlsx_sheets(output, &sheets)
}
