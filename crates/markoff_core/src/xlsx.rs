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

fn cell_value_from_data(cell: &calamine::Data) -> Result<CellValue, MarkoffError> {
    match cell {
        calamine::Data::Empty => Ok(CellValue::Empty),
        calamine::Data::Int(value) => Ok(CellValue::Integer(*value)),
        calamine::Data::Float(value) => Ok(cell_value_from_float(*value)),
        calamine::Data::String(value) => Ok(CellValue::String(value.clone())),
        calamine::Data::Bool(value) => Ok(CellValue::Boolean(*value)),
        unsupported => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("XLSX cell type cannot be represented without data loss: {unsupported:?}"),
        )
        .into()),
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
                    .map(cell_value_from_data)
                    .collect::<Result<Vec<_>, MarkoffError>>()
            })
            .collect::<Result<Vec<_>, MarkoffError>>()?;
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
            let row_index = u32::try_from(row_index).map_err(|_| MarkoffError::InvalidInput {
                path: format!("worksheet {name:?} exceeds the XLSX row limit"),
            })?;
            for (column_index, value) in row.iter().enumerate() {
                let column_index =
                    u16::try_from(column_index).map_err(|_| MarkoffError::InvalidInput {
                        path: format!("worksheet {name:?} exceeds the XLSX column limit"),
                    })?;
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

        if rows.len() > 1 && rows.first().is_some_and(|row| !row.is_empty()) {
            worksheet
                .set_freeze_panes(1, 0)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
        }
        let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
        for column_index in 0..column_count {
            let column_index =
                u16::try_from(column_index).map_err(|_| MarkoffError::InvalidInput {
                    path: format!("worksheet {name:?} exceeds the XLSX column limit"),
                })?;
            let width = rows
                .iter()
                .filter_map(|row| row.get(usize::from(column_index)))
                .map(|value| value.as_text().chars().count())
                .max()
                .unwrap_or(0)
                .clamp(8, 40) as f64;
            worksheet
                .set_column_width(column_index, width)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
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
    let sheets = parse_markdown_tables(&source)?;
    if sheets.is_empty() {
        return Err(MarkoffError::NotImplemented {
            from: Format::Markdown,
            to: Format::Xlsx,
        });
    }
    write_xlsx_sheets(output, &sheets)
}

#[cfg(test)]
mod tests {
    use super::cell_value_from_data;

    #[test]
    fn rejects_cell_types_that_would_be_stringified_lossily() {
        let cell = calamine::Data::DateTimeIso("2026-09-30T12:00:00".to_string());
        let error = cell_value_from_data(&cell).unwrap_err();
        assert!(error.to_string().contains("cannot be represented"));
    }
}
