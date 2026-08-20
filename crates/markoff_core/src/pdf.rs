use crate::MarkoffError;
use crate::error::invalid_data;
use std::path::Path;

pub(crate) fn convert_pdf_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let text = pdf_extract::extract_text(input).map_err(invalid_data)?;
    let markdown = text.trim_end();
    std::fs::write(output, format!("{markdown}\n"))?;
    Ok(())
}
