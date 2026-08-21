//! Shared helper for writing a single text part into an OOXML zip package,
//! used by both the DOCX and PPTX writers.

use crate::MarkoffError;
use crate::error::invalid_data;

pub(crate) fn write_zip_part(
    archive: &mut zip::ZipWriter<std::fs::File>,
    options: zip::write::SimpleFileOptions,
    name: &str,
    content: &str,
) -> Result<(), MarkoffError> {
    use std::io::Write as _;
    archive.start_file(name, options).map_err(invalid_data)?;
    archive.write_all(content.as_bytes())?;
    Ok(())
}
