use crate::MarkoffError;
use crate::document::expand_inline_footnotes;
use crate::xml_utils::xml_escape;
use base64::Engine as _;
use pulldown_cmark::{CowStr, Event, Tag};
use std::path::Path;

pub(crate) fn convert_markdown_to_html(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    use pulldown_cmark::{Options, Parser, html};

    let source = std::fs::read_to_string(input)?;
    let markdown = expand_inline_footnotes(&source);
    let title = source
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .unwrap_or("Document");

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_MATH);
    options.insert(Options::ENABLE_SUPERSCRIPT);
    options.insert(Options::ENABLE_SUBSCRIPT);
    let base_dir = input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let events = Parser::new_ext(&markdown, options)
        .map(|event| match event {
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => embed_local_image(dest_url.as_ref(), base_dir).map(|embedded| {
                Event::Start(Tag::Image {
                    link_type,
                    dest_url: embedded
                        .map(|destination| CowStr::Boxed(destination.into_boxed_str()))
                        .unwrap_or(dest_url),
                    title,
                    id,
                })
            }),
            event => Ok(event),
        })
        .collect::<Result<Vec<_>, MarkoffError>>()?;
    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());

    let document = format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n</head>\n<body>\n{body}</body>\n</html>\n",
        xml_escape(title)
    );
    std::fs::write(output, document)?;
    Ok(())
}

fn embed_local_image(destination: &str, base_dir: &Path) -> Result<Option<String>, MarkoffError> {
    if destination.is_empty()
        || destination.contains("://")
        || destination.starts_with("data:")
        || destination.starts_with('#')
    {
        return Ok(None);
    }
    let path = destination.split(['?', '#']).next().unwrap_or(destination);
    let image_path = base_dir.join(path);
    match std::fs::read(&image_path) {
        Ok(bytes) => {
            let media_type = match image_path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "svg" => "image/svg+xml",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                _ => "application/octet-stream",
            };
            Ok(Some(format!(
                "data:{media_type};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
