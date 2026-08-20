use std::path::Path;

use crate::tables::{markdown_table_from_rows, parse_markdown_table};
use crate::{Format, MarkoffError};

pub(crate) fn convert_csv_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let mut reader = csv::Reader::from_reader(source.as_bytes());

    let headers = reader
        .headers()
        .map(|row| row.iter().map(str::to_string).collect::<Vec<_>>())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?
        .to_vec();

    let mut rows = vec![headers];
    for record in reader.records() {
        let row = record
            .map(|row| row.iter().map(str::to_string).collect::<Vec<_>>())
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        rows.push(row);
    }

    std::fs::write(output, markdown_table_from_rows(&rows))?;
    Ok(())
}

pub(crate) fn convert_markdown_to_csv(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let rows = parse_markdown_table(&source);

    if rows.is_empty() {
        return Err(MarkoffError::NotImplemented {
            from: Format::Markdown,
            to: Format::Csv,
        });
    }

    let mut writer = csv::Writer::from_writer(Vec::new());
    for row in rows {
        writer
            .write_record(row)
            .map_err(crate::error::invalid_data)?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|error| crate::error::invalid_data(error.into_error()))?;
    std::fs::write(output, bytes)?;
    Ok(())
}
