use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use markoff_core::{Format, convert_file, detect_format};
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

pub(super) enum SourcePreview {
    Markdown {
        content: String,
        base_dir: PathBuf,
        _temporary_assets: Option<PreviewAssets>,
    },
    Tree(serde_json::Value),
    Text(String),
}

pub(super) struct PreviewAssets {
    directory: PathBuf,
}

impl Drop for PreviewAssets {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.directory)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "Unable to remove temporary Markdown preview assets {}: {error}",
                self.directory.display()
            );
        }
    }
}

impl SourcePreview {
    pub(super) fn message(message: impl Into<String>) -> Self {
        Self::Text(message.into())
    }
}

fn preview_directory() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time is after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("markoff_preview_{}_{}", std::process::id(), nanos))
}

fn markdown_document_preview(input: &Path, format: Format, directory: PathBuf) -> SourcePreview {
    if let Err(error) = std::fs::create_dir(&directory) {
        return SourcePreview::Text(format!(
            "Unable to create temporary Markdown preview directory {}: {error}",
            directory.display()
        ));
    }
    let assets = PreviewAssets { directory };
    let preview = assets.directory.join("preview.md");
    let rendered = convert_file(input, &preview, format, Format::Markdown)
        .map_err(|error| error.to_string())
        .and_then(|()| std::fs::read_to_string(&preview).map_err(|error| error.to_string()));
    let cleanup = match std::fs::remove_file(&preview) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Unable to remove temporary Markdown preview {}: {error}",
            preview.display()
        )),
    };

    match (rendered, cleanup) {
        (Ok(content), Ok(())) => {
            let base_dir = assets.directory.clone();
            SourcePreview::Markdown {
                content,
                base_dir,
                _temporary_assets: Some(assets),
            }
        }
        (Err(error), Ok(())) => {
            SourcePreview::Text(format!("Unable to create Markdown preview: {error}"))
        }
        (Ok(_), Err(error)) => SourcePreview::Text(error),
        (Err(render_error), Err(cleanup_error)) => SourcePreview::Text(format!(
            "Unable to create Markdown preview: {render_error}; {cleanup_error}"
        )),
    }
}

pub(super) fn load_source_preview(input: &Path) -> SourcePreview {
    let base_dir = input
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    match detect_format(input) {
        Ok(Format::Markdown) => match std::fs::read_to_string(input) {
            Ok(content) => SourcePreview::Markdown {
                content,
                base_dir,
                _temporary_assets: None,
            },
            Err(error) => {
                SourcePreview::Text(format!("Unable to read {}: {error}", input.display()))
            }
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
        Ok(
            format @ (Format::Docx
            | Format::Doc
            | Format::Odt
            | Format::Pdf
            | Format::Xls
            | Format::Xlsx
            | Format::Ods
            | Format::Ppt
            | Format::Pptx
            | Format::Odp
            | Format::Html),
        ) => markdown_document_preview(input, format, preview_directory()),
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
        Some(SourcePreview::Markdown {
            content, base_dir, ..
        }) => {
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
    let mut replacements = Parser::new_ext(markdown, Options::all())
        .into_offset_iter()
        .filter_map(|(event, image_range)| {
            let Event::Start(Tag::Image {
                link_type: LinkType::Inline,
                dest_url,
                ..
            }) = event
            else {
                return None;
            };
            let replacement = absolute_image_uri(&dest_url, base_dir)?;
            let image = markdown.get(image_range.clone())?;
            let destination_range = inline_image_destination_range(image)?;
            Some((
                image_range.start + destination_range.start
                    ..image_range.start + destination_range.end,
                replacement,
            ))
        })
        .collect::<Vec<_>>();
    replacements.sort_by_key(|(range, _)| range.start);

    let mut rendered = String::with_capacity(markdown.len());
    let mut cursor = 0;
    for (range, replacement) in replacements {
        if range.start < cursor {
            continue;
        }
        rendered.push_str(&markdown[cursor..range.start]);
        rendered.push_str(&replacement);
        cursor = range.end;
    }
    rendered.push_str(&markdown[cursor..]);
    rendered
}

fn inline_image_destination_range(source: &str) -> Option<Range<usize>> {
    let bytes = source.as_bytes();
    if !source.starts_with("![") {
        return None;
    }

    let mut cursor = 2;
    let mut bracket_depth = 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = (cursor + 2).min(bytes.len()),
            b'[' => {
                bracket_depth += 1;
                cursor += 1;
            }
            b']' => {
                bracket_depth -= 1;
                cursor += 1;
                if bracket_depth == 0 {
                    break;
                }
            }
            _ => cursor += 1,
        }
    }
    if bracket_depth != 0 || bytes.get(cursor) != Some(&b'(') {
        return None;
    }

    cursor += 1;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }

    if bytes.get(cursor) == Some(&b'<') {
        let start = cursor + 1;
        cursor = start;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'\\' => cursor = (cursor + 2).min(bytes.len()),
                b'>' => return (cursor > start).then_some(start..cursor),
                b'\n' | b'\r' => return None,
                _ => cursor += 1,
            }
        }
        return None;
    }

    let start = cursor;
    let mut parenthesis_depth = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = (cursor + 2).min(bytes.len()),
            b'(' => {
                parenthesis_depth += 1;
                cursor += 1;
            }
            b')' if parenthesis_depth == 0 => {
                return (cursor > start).then_some(start..cursor);
            }
            b')' => {
                parenthesis_depth -= 1;
                cursor += 1;
            }
            byte if byte.is_ascii_whitespace() && parenthesis_depth == 0 => {
                return (cursor > start).then_some(start..cursor);
            }
            _ => cursor += 1,
        }
    }
    None
}

fn absolute_image_uri(destination: &str, base_dir: &Path) -> Option<String> {
    if has_uri_scheme(destination) {
        return None;
    }

    let (destination, suffix) = split_uri_suffix(destination);
    let destination = decode_uri_path(destination)?;
    let path = if is_windows_absolute_path(&destination) || Path::new(&destination).is_absolute() {
        PathBuf::from(&destination)
    } else if let Some((drive, relative)) = windows_drive_relative_path(&destination) {
        let normalized = relative.replace('\\', "/");
        let path = Path::new(&normalized);
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return None;
        }
        let base_drive = windows_drive_prefix(&base_dir.to_string_lossy());
        if base_drive.is_some_and(|base| base.eq_ignore_ascii_case(&drive)) {
            base_dir.join(path)
        } else {
            PathBuf::from(format!("{drive}:/{normalized}"))
        }
    } else {
        let normalized = destination.replace('\\', "/");
        let path = Path::new(&normalized);
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return None;
        }
        base_dir.join(path)
    };

    Some(format!("{}{}", file_uri(&path), encode_uri_suffix(suffix)))
}

fn split_uri_suffix(destination: &str) -> (&str, &str) {
    destination
        .find(['?', '#'])
        .map_or((destination, ""), |suffix_start| {
            (&destination[..suffix_start], &destination[suffix_start..])
        })
}

fn decode_uri_path(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut cursor = 0;

    while cursor < bytes.len() {
        if bytes[cursor] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(cursor + 1).and_then(|byte| hex_digit(*byte)),
                bytes.get(cursor + 2).and_then(|byte| hex_digit(*byte)),
            )
        {
            decoded.push((high << 4) | low);
            cursor += 3;
        } else {
            decoded.push(bytes[cursor]);
            cursor += 1;
        }
    }

    String::from_utf8(decoded).ok()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn is_windows_absolute_path(path: &str) -> bool {
    path.as_bytes().get(2).is_some_and(|separator| {
        windows_drive_prefix(path).is_some() && matches!(*separator, b'/' | b'\\')
    })
}

fn windows_drive_relative_path(path: &str) -> Option<(char, &str)> {
    let drive = windows_drive_prefix(path)?;
    let relative = path.get(2..)?;
    if relative.is_empty() || relative.starts_with('/') || relative.starts_with('\\') {
        return None;
    }
    Some((drive, relative))
}

fn windows_drive_prefix(path: &str) -> Option<char> {
    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        .then_some(bytes[0] as char)
}

fn file_uri(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let uri_path = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };
    format!("file://{}", encode_uri_path(&uri_path))
}

fn encode_uri_path(path: &str) -> String {
    encode_uri_component(path, b"/:", false)
}

fn encode_uri_suffix(suffix: &str) -> String {
    encode_uri_component(suffix, b"/?:#@!$&'()*+,;=", true)
}

fn encode_uri_component(value: &str, reserved: &[u8], preserve_percent_escapes: bool) -> String {
    let mut encoded = String::with_capacity(value.len());
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let bytes = value.as_bytes();
    let mut cursor = 0;

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if preserve_percent_escapes
            && byte == b'%'
            && bytes
                .get(cursor + 1..cursor + 3)
                .is_some_and(|hex| hex[0].is_ascii_hexdigit() && hex[1].is_ascii_hexdigit())
        {
            encoded.push('%');
            encoded.push(char::from(bytes[cursor + 1]));
            encoded.push(char::from(bytes[cursor + 2]));
            cursor += 3;
        } else if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~')
            || reserved.contains(&byte)
        {
            encoded.push(char::from(byte));
            cursor += 1;
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0F)]));
            cursor += 1;
        }
    }

    encoded
}

fn has_uri_scheme(destination: &str) -> bool {
    let Some((scheme, _)) = destination.split_once(':') else {
        return false;
    };
    if scheme.len() == 1 && windows_drive_prefix(destination).is_some() {
        return false;
    }

    let mut characters = scheme.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
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
    ui.push_id(key, |ui| match value {
        serde_json::Value::Object(map) => {
            egui::CollapsingHeader::new(key)
                .default_open(false)
                .show(ui, |ui| {
                    for (child_key, child) in map {
                        render_json_node(ui, child_key, child);
                    }
                });
        }
        serde_json::Value::Array(items) => {
            egui::CollapsingHeader::new(format!("{key} [{}]", items.len()))
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
    });
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
    use super::{
        SourcePreview, absolute_image_uri, markdown_document_preview,
        markdown_with_absolute_image_paths,
    };
    use markoff_core::{Format, convert_file};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_path(name: &str, extension: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "markoff_gui_{name}_{}_{nanos}.{extension}",
            std::process::id()
        ))
    }

    #[test]
    fn makes_relative_image_paths_absolute_file_uris() {
        let rendered = markdown_with_absolute_image_paths(
            "Before\n\n![Chart](image/chart.png)\n",
            Path::new("D:/Project/markoff/sample"),
        );

        assert!(rendered.contains("![Chart](file:///D:/Project/markoff/sample/image/chart.png)"));
    }

    #[test]
    fn preserves_image_titles_and_rewrites_escaped_and_nested_destinations() {
        let rendered = markdown_with_absolute_image_paths(
            "![Chart \\]](images/chart(2025).png \"Sales chart\")\n![Draft](images/chart\\(draft\\).png 'Draft chart')",
            Path::new("D:/Project/markoff/sample"),
        );

        assert_eq!(
            rendered,
            "![Chart \\]](file:///D:/Project/markoff/sample/images/chart%282025%29.png \"Sales chart\")\n![Draft](file:///D:/Project/markoff/sample/images/chart%28draft%29.png 'Draft chart')"
        );
    }

    #[test]
    fn resolves_percent_encoded_image_paths_without_double_encoding() {
        let rendered = markdown_with_absolute_image_paths(
            "![Space](images/chart%20small.png)\n![Percent](images/chart%2520small.png)",
            Path::new("D:/Project/markoff/sample"),
        );

        assert_eq!(
            rendered,
            "![Space](file:///D:/Project/markoff/sample/images/chart%20small.png)\n![Percent](file:///D:/Project/markoff/sample/images/chart%2520small.png)"
        );
    }

    #[test]
    fn preserves_query_and_fragment_on_local_image_paths() {
        let rendered = markdown_with_absolute_image_paths(
            "![Chart](images/chart%20small.png?scale=2#preview)",
            Path::new("D:/Project/markoff/sample"),
        );

        assert_eq!(
            rendered,
            "![Chart](file:///D:/Project/markoff/sample/images/chart%20small.png?scale=2#preview)"
        );
    }

    #[test]
    fn ignores_percent_encoded_parent_traversal() {
        let markdown = "![Outside](%2E%2E/secret.png)";

        assert_eq!(
            markdown_with_absolute_image_paths(markdown, Path::new("D:/Project/markoff/sample")),
            markdown
        );
    }

    #[test]
    fn resolves_windows_absolute_and_drive_relative_image_paths() {
        let base_dir = Path::new("C:/Project");

        assert_eq!(
            absolute_image_uri("C:folder\\img.png", base_dir),
            Some("file:///C:/Project/folder/img.png".to_string())
        );
        assert_eq!(
            absolute_image_uri("D:/assets/img.png", base_dir),
            Some("file:///D:/assets/img.png".to_string())
        );
    }

    #[test]
    fn keeps_real_image_uris_unchanged() {
        let markdown = "![Remote](https://example.com/chart.png \"Remote chart\")\n![Inline](data:image/png;base64,abc)\n![Custom](custom+scheme:asset)";

        assert_eq!(
            markdown_with_absolute_image_paths(markdown, Path::new("D:/Project/markoff/sample")),
            markdown
        );
    }

    #[test]
    fn missing_markdown_is_shown_as_an_error_not_rendered_markdown() {
        let missing = temporary_path("missing_markdown", "md");
        let SourcePreview::Text(message) = super::load_source_preview(&missing) else {
            panic!("expected an explicit read error");
        };
        assert!(message.contains("Unable to read"));
    }

    #[test]
    fn successful_document_preview_removes_markdown_temp_file_and_assets_on_drop() {
        let source = temporary_path("preview_success_source", "md");
        let document = temporary_path("preview_success_document", "docx");
        let directory = temporary_path("preview_success_assets", "tmp");
        fs::write(&source, "# Project\n\nA **bold** note.\n").unwrap();
        convert_file(&source, &document, Format::Markdown, Format::Docx).unwrap();

        let preview = markdown_document_preview(&document, Format::Docx, directory.clone());
        let SourcePreview::Markdown { content, .. } = &preview else {
            panic!("expected a Markdown preview for a converted DOCX file");
        };
        assert!(content.contains("# Project"));
        assert!(!directory.join("preview.md").exists());
        assert!(directory.exists());

        drop(preview);
        assert!(!directory.exists());
        fs::remove_file(source).unwrap();
        fs::remove_file(document).unwrap();
    }

    #[test]
    fn failed_document_preview_reports_the_conversion_error_and_cleans_up() {
        let document = temporary_path("preview_failed_document", "docx");
        let directory = temporary_path("preview_failed_assets", "tmp");
        fs::write(&document, "not a DOCX archive").unwrap();

        let preview = markdown_document_preview(&document, Format::Docx, directory.clone());
        let SourcePreview::Text(message) = preview else {
            panic!("expected an explicit error message for an invalid DOCX file");
        };
        assert!(message.starts_with("Unable to create Markdown preview:"));
        assert!(!directory.exists());
        fs::remove_file(document).unwrap();
    }

    #[test]
    fn preview_directory_creation_errors_preserve_existing_files() {
        let directory = temporary_path("preview_existing_assets", "tmp");
        fs::create_dir(&directory).unwrap();
        let existing_file = directory.join("keep.txt");
        fs::write(&existing_file, "keep").unwrap();

        let preview =
            markdown_document_preview(Path::new("missing.docx"), Format::Docx, directory.clone());
        let SourcePreview::Text(message) = preview else {
            panic!("expected a preview-directory error");
        };

        assert!(message.starts_with("Unable to create temporary Markdown preview directory"));
        assert_eq!(fs::read_to_string(&existing_file).unwrap(), "keep");
        fs::remove_dir_all(directory).unwrap();
    }
}
