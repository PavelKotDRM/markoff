use eframe::egui;
use markoff_core::{Format, convert_file, detect_format};
use std::path::{Path, PathBuf};

pub(super) enum SourcePreview {
    Markdown(String),
    Tree(serde_json::Value),
    Text(String),
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
    match detect_format(input) {
        Ok(Format::Markdown) => SourcePreview::Markdown(read_text_or_error(input)),
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
            let rendered = convert_file(input, &preview, format, Format::Markdown)
                .map_err(|error| error.to_string())
                .and_then(|()| {
                    std::fs::read_to_string(&preview).map_err(|error| error.to_string())
                });
            std::fs::remove_file(&preview).ok();
            match rendered {
                Ok(markdown) => SourcePreview::Markdown(markdown),
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

fn read_text_or_error(input: &Path) -> String {
    std::fs::read_to_string(input)
        .unwrap_or_else(|error| format!("Unable to read {}: {error}", input.display()))
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
