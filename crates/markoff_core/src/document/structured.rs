use crate::Format;
use crate::MarkoffError;
use crate::error::invalid_data;
use std::path::Path;

pub(super) fn convert_structured_data_format(
    input: &Path,
    output: &Path,
    from: Format,
    to: Format,
) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let value = parse_structured_value(&source, from)?;
    let rendered = match to {
        Format::Json => serde_json::to_string_pretty(&value).map_err(invalid_data)?,
        Format::Yaml => serde_yaml::to_string(&value).map_err(invalid_data)?,
        Format::Toml => toml::to_string_pretty(&value).map_err(invalid_data)?,
        _ => unreachable!("only JSON, YAML, and TOML use this helper"),
    };
    std::fs::write(output, rendered)?;
    Ok(())
}

fn parse_structured_value(source: &str, format: Format) -> Result<serde_json::Value, MarkoffError> {
    match format {
        Format::Json => Ok(serde_json::from_str(source).map_err(invalid_data)?),
        Format::Yaml => Ok(serde_yaml::from_str(source).map_err(invalid_data)?),
        Format::Toml => Ok(toml::from_str(source).map_err(invalid_data)?),
        _ => unreachable!("only JSON, YAML, and TOML use this helper"),
    }
}
