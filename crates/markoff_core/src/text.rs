use crate::MarkoffError;
use std::path::Path;

pub(crate) fn convert_json_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let value: serde_json::Value = serde_json::from_str(&source)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let pretty = serde_json::to_string_pretty(&value)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let markdown = format!("# JSON Document\n\n```json\n{pretty}\n```\n");
    std::fs::write(output, markdown)?;
    Ok(())
}

pub(crate) fn convert_markdown_to_json(input: &Path, output: &Path) -> Result<(), MarkoffError> {
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

pub(crate) fn convert_yaml_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let rendered = source.trim();
    std::fs::write(
        output,
        format!("# YAML Document\n\n```yaml\n{rendered}\n```\n"),
    )?;
    Ok(())
}

pub(crate) fn convert_toml_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
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
