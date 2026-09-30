use crate::MarkoffError;
use crate::document_model::Inline;
use crate::error::invalid_data;
use base64::Engine as _;
use std::path::Path;

pub(super) fn load_image_asset(
    destination: &str,
    base_dir: &Path,
) -> Result<(Option<String>, Option<String>), MarkoffError> {
    if let Some(data_uri) = destination.strip_prefix("data:image/")
        && let Some((media_type, data)) = data_uri.split_once(",")
        && media_type.ends_with(";base64")
    {
        let format = match media_type
            .strip_suffix(";base64")
            .unwrap_or(media_type)
            .to_ascii_lowercase()
            .as_str()
        {
            "image/jpeg" => Some("jpg".to_string()),
            "image/svg+xml" => Some("svg".to_string()),
            "image/png" => Some("png".to_string()),
            "image/gif" => Some("gif".to_string()),
            "image/webp" => Some("webp".to_string()),
            "image/bmp" => Some("bmp".to_string()),
            _ => None,
        };
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(invalid_data)?;
        return Ok((
            format,
            Some(base64::engine::general_purpose::STANDARD.encode(decoded)),
        ));
    }

    let path = destination.split(['?', '#']).next().unwrap_or(destination);
    if path.is_empty() || path.contains("://") || path.starts_with('#') {
        return Ok((None, None));
    }
    let image_path = base_dir.join(path);
    match std::fs::read(&image_path) {
        Ok(bytes) => {
            let format = Path::new(path)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("png")
                .to_ascii_lowercase();
            Ok((
                Some(format),
                Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((None, None)),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn inline_plain_text(content: &[Inline]) -> String {
    let mut text = String::new();
    for inline in content {
        match inline {
            Inline::Text { text: value }
            | Inline::Code { text: value }
            | Inline::Math { text: value, .. } => {
                text.push_str(value);
            }
            Inline::Emphasis { content }
            | Inline::Strong { content }
            | Inline::Strikethrough { content }
            | Inline::Underline { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Footnote { content }
            | Inline::Link { content, .. } => text.push_str(&inline_plain_text(content)),
            Inline::Image { alt, .. } => text.push_str(alt),
            Inline::FootnoteReference { label } => {
                text.push_str("[^");
                text.push_str(label);
                text.push(']');
            }
            Inline::Bookmark { .. } | Inline::TaskListMarker { .. } | Inline::Html { .. } => {}
            Inline::SoftBreak | Inline::HardBreak => text.push(' '),
        }
    }
    text
}
