use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use markoff_core::{Format, convert_file, detect_format};
use std::path::{Component, Path, PathBuf};

pub(super) enum SourcePreview {
    Markdown { content: String, base_dir: PathBuf },
    Tree(serde_json::Value),
    Text(String),
}

impl SourcePreview {
    pub(super) fn message(message: impl Into<String>) -> Self {
        Self::Text(message.into())
    }
}

fn preview_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time is after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "markoff_preview_{}_{}.md",
        std::process::id(),
        nanos
    ))
}

pub(super) fn load_source_preview(input: &Path) -> SourcePreview {
    let base_dir = input.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    match detect_format(input) {
        Ok(Format::Markdown) => SourcePreview::Markdown {
            content: read_text_or_error(input),
            base_dir,
        },
        Ok(Format::Json) => structured_tree_preview(input, |source| {
            serde_json::from_str(source).map_err(|error| error.to_string())
        }),
        Ok(Format::Yaml) => structured_tree_preview(input, |source| {
            serde_yaml::from_str::<serde_json::Value>(source).map_err(|error| error.to_string())
        }),
        Ok(Format::Toml) => structured_tree_preview(input, |source| {
            source
                .parse::<toml::Value>()
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }),
        Ok(format @ (Format::Docx | Format::Pdf | Format::Xlsx | Format::Pptx | Format::Html)) => {
            let preview = preview_path();
            let preview_base_dir = preview.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
            let rendered = convert_file(input, &preview, format, Format::Markdown)
                .map_err(|error| error.to_string())
                .and_then(|()| {
                    std::fs::read_to_string(&preview).map_err(|error| error.to_string())
                });
            std::fs::remove_file(&preview).ok();
            match rendered {
                Ok(markdown) => SourcePreview::Markdown {
                    content: markdown,
                    base_dir: preview_base_dir,
                },
                Err(error) => {
                    SourcePreview::Text(format!("Unable to create Markdown preview: {error}"))
                }
            }
        }
        Ok(_) => SourcePreview::Text(read_text_or_error(input)),
        Err(_) => SourcePreview::Text(format!(
            "Unable to preview {}: unrecognized format",
            input.display()
        )),
    }
}

pub(super) fn render_preview(
    ui: &mut egui::Ui,
    preview: Option<&SourcePreview>,
    markdown_cache: &mut CommonMarkCache,
    empty_message: &str,
) {
    match preview {
        Some(SourcePreview::Markdown { content, base_dir }) => {
            let markdown = markdown_with_absolute_image_paths(content, base_dir);
            CommonMarkViewer::new()
                .explicit_image_uri_scheme(true)
                .show(ui, markdown_cache, &markdown);
        }
        Some(SourcePreview::Tree(value)) => render_json_tree(ui, value),
        Some(SourcePreview::Text(text)) => {
            ui.monospace(text);
        }
        None => {
            ui.monospace(empty_message);
        }
    }
}

fn read_text_or_error(input: &Path) -> String {
    std::fs::read_to_string(input)
        .unwrap_or_else(|error| format!("Unable to read {}: {error}", input.display()))
}

fn markdown_with_absolute_image_paths(markdown: &str, base_dir: &Path) -> String {
    let mut rendered = String::with_capacity(markdown.len());
    let mut remaining = markdown;
    while let Some(image_start) = remaining.find("![") {
        let (before, from_image) = remaining.split_at(image_start);
        rendered.push_str(before);

        let Some(label_end) = from_image.find("](") else {
            rendered.push_str(from_image);
            return rendered;
        };
        let destination_start = label_end + 2;
        let Some(destination_end_offset) = from_image[destination_start..].find(')') else {
            rendered.push_str(from_image);
            return rendered;
        };
        let destination_end = destination_start + destination_end_offset;
        let destination = &from_image[destination_start..destination_end];

        rendered.push_str(&from_image[..destination_start]);
        rendered.push_str(&absolute_image_uri(destination, base_dir));
        rendered.push(')');
        remaining = &from_image[destination_end + 1..];
    }
    rendered.push_str(remaining);
    rendered
}

fn absolute_image_uri(destination: &str, base_dir: &Path) -> String {
    let destination_path = Path::new(destination);
    if destination_path.is_absolute() {
        return file_uri(destination_path);
    }

    if has_uri_scheme(destination) || destination.starts_with("data:") {
        return destination.to_string();
    }

    let normalized = destination.replace('\\', "/");
    let path = Path::new(&normalized);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return destination.to_string();
    }

    let absolute = base_dir.join(path);
    file_uri(&absolute)
}

fn file_uri(path: &Path) -> String {
    format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
}

fn has_uri_scheme(destination: &str) -> bool {
    destination
        .find(':')
        .is_some_and(|index| destination[..index].chars().all(|character| character.is_ascii_alphanumeric()))
}

fn structured_tree_preview(
    input: &Path,
    parse: impl FnOnce(&str) -> Result<serde_json::Value, String>,
) -> SourcePreview {
    match std::fs::read_to_string(input) {
        Ok(source) => match parse(&source) {
            Ok(value) => SourcePreview::Tree(value),
            Err(error) => {
                SourcePreview::Text(format!("Unable to parse {}: {error}", input.display()))
            }
        },
        Err(error) => SourcePreview::Text(format!("Unable to read {}: {error}", input.display())),
    }
}

pub(super) fn render_json_tree(ui: &mut egui::Ui, value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                render_json_node(ui, key, child);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                render_json_node(ui, &index.to_string(), child);
            }
        }
        scalar => {
            ui.label(json_scalar_to_string(scalar));
        }
    }
}

fn render_json_node(ui: &mut egui::Ui, key: &str, value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            egui::CollapsingHeader::new(key)
                .id_salt(key)
                .default_open(false)
                .show(ui, |ui| {
                    for (child_key, child) in map {
                        render_json_node(ui, child_key, child);
                    }
                });
        }
        serde_json::Value::Array(items) => {
            egui::CollapsingHeader::new(format!("{key} [{}]", items.len()))
                .id_salt(key)
                .default_open(false)
                .show(ui, |ui| {
                    for (index, child) in items.iter().enumerate() {
                        render_json_node(ui, &index.to_string(), child);
                    }
                });
        }
        scalar => {
            ui.label(format!("{key}: {}", json_scalar_to_string(scalar)));
        }
    }
}

fn json_scalar_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::markdown_with_absolute_image_paths;
    use std::path::Path;

    #[test]
    fn makes_relative_image_paths_absolute_file_uris() {
        let rendered = markdown_with_absolute_image_paths(
            "Before\n\n![Chart](image/chart.png)\n",
            Path::new("D:/Project/markoff/sample"),
        );

        assert!(rendered.contains("![Chart](file:///D:/Project/markoff/sample/image/chart.png)"));
    }

    #[test]
    fn keeps_explicit_image_uris_unchanged() {
        let rendered = markdown_with_absolute_image_paths(
            "![Remote](https://example.com/chart.png)\n![Inline](data:image/png;base64,abc)",
            Path::new("D:/Project/markoff/sample"),
        );

        assert!(rendered.contains("![Remote](https://example.com/chart.png)"));
        assert!(rendered.contains("![Inline](data:image/png;base64,abc)"));
    }
}
