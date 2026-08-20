use std::collections::BTreeMap;
use std::path::Path;

use crate::error::invalid_data;
use crate::xlsx::{CellValue, read_xlsx_value_sheets, write_xlsx_value_sheets};
use crate::{Format, MarkoffError};

fn rows_from_json(value: &serde_json::Value) -> Result<Vec<Vec<CellValue>>, MarkoffError> {
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

    let mut rows = vec![headers.iter().cloned().map(CellValue::String).collect()];
    for object in objects {
        let object = object.as_object().expect("validated above");
        rows.push(
            headers
                .iter()
                .map(|key| object.get(key).map_or(CellValue::Empty, json_cell_to_value))
                .collect(),
        );
    }
    Ok(rows)
}

fn json_cell_to_value(value: &serde_json::Value) -> CellValue {
    match value {
        serde_json::Value::Null => CellValue::Empty,
        serde_json::Value::Bool(value) => CellValue::Boolean(*value),
        serde_json::Value::Number(value) => value.as_i64().map_or_else(
            || CellValue::Float(value.as_f64().unwrap_or_default()),
            CellValue::Integer,
        ),
        serde_json::Value::String(value) => CellValue::String(value.clone()),
        value => CellValue::String(value.to_string()),
    }
}

fn json_from_rows(rows: &[Vec<CellValue>]) -> serde_json::Value {
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
                        let value = row.get(index).unwrap_or(&CellValue::Empty);
                        (header.as_text(), json_from_cell(value))
                    })
                    .collect();
                serde_json::Value::Object(object)
            })
            .collect(),
    )
}

fn json_from_cell(value: &CellValue) -> serde_json::Value {
    match value {
        CellValue::Empty => serde_json::Value::Null,
        CellValue::Integer(value) => serde_json::Value::Number((*value).into()),
        CellValue::Float(value) => serde_json::Number::from_f64(*value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        CellValue::String(value) => serde_json::Value::String(value.clone()),
        CellValue::Boolean(value) => serde_json::Value::Bool(*value),
    }
}

pub(crate) fn convert_data_to_xlsx(
    input: &Path,
    output: &Path,
    format: Format,
) -> Result<(), MarkoffError> {
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
            let headers: Vec<String> = reader
                .headers()
                .map_err(invalid_data)?
                .iter()
                .map(str::to_string)
                .collect();
            let mut rows = vec![headers.into_iter().map(CellValue::String).collect()];
            for record in reader.records() {
                rows.push(
                    record
                        .map_err(invalid_data)?
                        .iter()
                        .map(str::to_string)
                        .map(CellValue::String)
                        .collect(),
                );
            }
            rows
        }
        _ => unreachable!("only structured data formats use this helper"),
    };
    let mut sheets = BTreeMap::new();
    sheets.insert("Sheet1".to_string(), rows);
    write_xlsx_value_sheets(output, &sheets)
}

pub(crate) fn convert_xlsx_to_data(
    input: &Path,
    output: &Path,
    format: Format,
) -> Result<(), MarkoffError> {
    let sheets = read_xlsx_value_sheets(input)?;
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
        Format::Toml => {
            let value = if value.is_array() {
                serde_json::json!({ "rows": value })
            } else {
                value
            };
            std::fs::write(
                output,
                toml::to_string_pretty(&value).map_err(invalid_data)?,
            )?
        }
        Format::Csv => {
            let rows = sheets
                .values()
                .next()
                .ok_or_else(|| MarkoffError::InvalidInput {
                    path: input.to_string_lossy().to_string(),
                })?;
            let mut writer = csv::Writer::from_writer(Vec::new());
            for row in rows {
                writer
                    .write_record(row.iter().map(CellValue::as_text))
                    .map_err(invalid_data)?;
            }
            let bytes = writer
                .into_inner()
                .map_err(|error| invalid_data(error.into_error()))?;
            std::fs::write(output, bytes)?;
        }
        _ => unreachable!("only structured data formats use this helper"),
    }
    Ok(())
}
