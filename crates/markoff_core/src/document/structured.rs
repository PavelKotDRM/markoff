use crate::Format;
use crate::MarkoffError;
use crate::error::invalid_data;
use std::path::Path;

const TOML_JSON_VALUE_KEY: &str = "__markoff_json_value_v1";

pub(super) fn convert_structured_data_format(
    input: &Path,
    output: &Path,
    from: Format,
    to: Format,
) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let value = parse_structured_value(&source, from)?;
    let rendered = render_structured_value(&value, to)?;
    std::fs::write(output, rendered)?;
    Ok(())
}

pub(crate) fn render_structured_value(
    value: &serde_json::Value,
    format: Format,
) -> Result<String, MarkoffError> {
    Ok(match format {
        Format::Json => serde_json::to_string_pretty(&value).map_err(invalid_data)?,
        Format::Yaml => serde_yaml::to_string(&value).map_err(invalid_data)?,
        Format::Toml
            if !value.is_object()
                || requires_json_encoding(value)
                || value.as_object().is_some_and(|object| {
                    object.len() == 1 && object.contains_key(TOML_JSON_VALUE_KEY)
                }) =>
        {
            // TOML has neither null nor non-table roots. Retain the exact JSON value
            // under a versioned key rather than discarding cells or changing types.
            let envelope = serde_json::json!({
                TOML_JSON_VALUE_KEY: serde_json::to_string_pretty(value).map_err(invalid_data)?
            });
            toml::to_string_pretty(&envelope).map_err(invalid_data)?
        }
        Format::Toml => toml::to_string_pretty(&value).map_err(invalid_data)?,
        _ => unreachable!("only JSON, YAML, and TOML use this helper"),
    })
}

fn requires_json_encoding(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::Number(number) => number.is_u64() && !number.is_i64(),
        serde_json::Value::Array(values) => values.iter().any(requires_json_encoding),
        serde_json::Value::Object(values) => values.values().any(requires_json_encoding),
        _ => false,
    }
}

pub(crate) fn parse_structured_value(
    source: &str,
    format: Format,
) -> Result<serde_json::Value, MarkoffError> {
    match format {
        Format::Json => Ok(serde_json::from_str(source).map_err(invalid_data)?),
        Format::Yaml => Ok(serde_yaml::from_str(source).map_err(invalid_data)?),
        Format::Toml => {
            let value: serde_json::Value = toml::from_str(source).map_err(invalid_data)?;
            if let Some(object) = value.as_object()
                && object.len() == 1
                && let Some(encoded) = object.get(TOML_JSON_VALUE_KEY)
            {
                let encoded = encoded.as_str().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid markoff TOML interchange payload",
                    )
                })?;
                return Ok(serde_json::from_str(encoded).map_err(invalid_data)?);
            }
            Ok(value)
        }
        _ => unreachable!("only JSON, YAML, and TOML use this helper"),
    }
}
