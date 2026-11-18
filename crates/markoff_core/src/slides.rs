//! Slide model shared by the PPTX and ODP converters.
//!
//! A slide is a title followed by an ordered list of blocks: paragraphs,
//! bulleted items, images, and tables. Charts are imported as tables of their
//! cached data. Exact positioning is not preserved; writers reflow blocks
//! top-to-bottom in source order.

use crate::MarkoffError;
use crate::pptx::markdown_inline_to_plain_text;
use crate::tables::{markdown_table_from_rows, parse_markdown_table};
use crate::xml_utils::markdown_link_destination;
use base64::Engine as _;
use image::ImageFormat;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(crate) struct Slide {
    pub(crate) title: String,
    pub(crate) blocks: Vec<SlideBlock>,
}

pub(crate) enum SlideBlock {
    Text {
        text: String,
        bulleted: bool,
    },
    Image {
        alt: String,
        source: String,
        data: Option<SlideImage>,
    },
    Table(Vec<Vec<String>>),
}

pub(crate) struct SlideImage {
    pub(crate) bytes: Vec<u8>,
    pub(crate) extension: &'static str,
    pub(crate) media_type: &'static str,
    /// Pixel dimensions for raster images.
    pub(crate) size: Option<(u32, u32)>,
}

/// Text used in place of an image that cannot be embedded.
pub(crate) fn image_placeholder_text(alt: &str, source: &str) -> String {
    tracing::warn!(source, "image cannot be embedded in the presentation");
    if alt.is_empty() {
        source.to_string()
    } else {
        format!("{alt} ({source})")
    }
}

/// Parses slide Markdown. `#`/`##` headings start slides; image paths are
/// resolved relative to `base_dir`.
pub(crate) fn parse_markdown_slides(source: &str, base_dir: &Path) -> Vec<Slide> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut slides = Vec::new();
    let mut current: Option<Slide> = None;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        index += 1;
        if line.is_empty() || line == "---" {
            continue;
        }
        if let Some(text) = line.strip_prefix("# ").or_else(|| line.strip_prefix("## ")) {
            if let Some(slide) = current.take() {
                slides.push(slide);
            }
            current = Some(Slide {
                title: markdown_inline_to_plain_text(text),
                blocks: Vec::new(),
            });
            continue;
        }
        let slide = current.get_or_insert_with(Slide::default);
        if line.starts_with('|') {
            let start = index - 1;
            while index < lines.len() && lines[index].trim().starts_with('|') {
                index += 1;
            }
            let table = parse_markdown_table(&lines[start..index].join("\n")).unwrap_or_default();
            if table.is_empty() {
                for line in &lines[start..index] {
                    slide.blocks.push(text_block(line.trim()));
                }
            } else {
                slide.blocks.push(SlideBlock::Table(
                    table
                        .into_iter()
                        .map(|row| {
                            row.into_iter()
                                .map(|cell| {
                                    cell.lines()
                                        .map(markdown_inline_to_plain_text)
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .collect()
                        })
                        .collect(),
                ));
            }
            continue;
        }
        if let Some((alt, destination)) = sole_image(line) {
            let data = load_slide_image(&destination, base_dir);
            slide.blocks.push(SlideBlock::Image {
                alt,
                source: destination,
                data,
            });
            continue;
        }
        slide.blocks.push(text_block(line));
    }
    if let Some(slide) = current {
        slides.push(slide);
    }
    if slides.is_empty() {
        slides.push(Slide::default());
    }
    slides
}

fn text_block(line: &str) -> SlideBlock {
    let (text, bulleted) =
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            (rest, true)
        } else if let Some(rest) = strip_ordered_marker(line) {
            (rest, true)
        } else {
            (line, false)
        };
    SlideBlock::Text {
        text: markdown_inline_to_plain_text(text),
        bulleted,
    }
}

fn strip_ordered_marker(line: &str) -> Option<&str> {
    let digits_end = line.find(|character: char| !character.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    line.get(digits_end..)?.strip_prefix(". ")
}

/// Returns `(alt, destination)` when the line consists of a single image.
fn sole_image(line: &str) -> Option<(String, String)> {
    let mut alt = String::new();
    let mut destination = String::new();
    // 0: before the image, 1: inside it, 2: after it.
    let mut state = 0;
    for event in Parser::new(line) {
        match (state, event) {
            (0 | 2, Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph)) => {}
            (0, Event::Start(Tag::Image { dest_url, .. })) => {
                destination = dest_url.into_string();
                state = 1;
            }
            (1, Event::End(TagEnd::Image)) => state = 2,
            (1, Event::Text(text) | Event::Code(text)) => alt.push_str(&text),
            (1, _) => {}
            _ => return None,
        }
    }
    (state == 2).then_some((alt, destination))
}

/// Loads a local, `data:` URI, or embedded image for a presentation.
pub(crate) fn load_slide_image(destination: &str, base_dir: &Path) -> Option<SlideImage> {
    if let Some(rest) = destination.strip_prefix("data:") {
        let (media_type, data) = rest.split_once(";base64,")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.trim())
            .ok()?;
        let hint = match media_type {
            "image/svg+xml" => "svg",
            "image/x-emf" | "image/emf" => "emf",
            "image/x-wmf" | "image/wmf" => "wmf",
            _ => "",
        };
        return slide_image_from_bytes(bytes, hint);
    }
    if destination.contains("://") {
        return None;
    }
    let path = Path::new(destination);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    let bytes = std::fs::read(&path).ok()?;
    let hint = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    slide_image_from_bytes(bytes, &hint)
}

fn slide_image_from_bytes(bytes: Vec<u8>, extension_hint: &str) -> Option<SlideImage> {
    if let Ok(format) = image::guess_format(&bytes) {
        let (extension, media_type) = match format {
            ImageFormat::Png => ("png", "image/png"),
            ImageFormat::Jpeg => ("jpeg", "image/jpeg"),
            ImageFormat::Gif => ("gif", "image/gif"),
            ImageFormat::Bmp => ("bmp", "image/bmp"),
            _ => {
                // Formats presentation apps may not render are stored as PNG.
                let decoded = image::load_from_memory(&bytes).ok()?;
                let mut png = Vec::new();
                decoded
                    .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
                    .ok()?;
                return Some(SlideImage {
                    bytes: png,
                    extension: "png",
                    media_type: "image/png",
                    size: Some((decoded.width(), decoded.height())),
                });
            }
        };
        let size = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok();
        return Some(SlideImage {
            bytes,
            extension,
            media_type,
            size,
        });
    }
    let (extension, media_type) = match extension_hint {
        "svg" => ("svg", "image/svg+xml"),
        "emf" => ("emf", "image/x-emf"),
        "wmf" => ("wmf", "image/x-wmf"),
        _ => return None,
    };
    Some(SlideImage {
        bytes,
        extension,
        media_type,
        size: None,
    })
}

/// Renders slides as Markdown. Text and table cells must already be escaped.
pub(crate) fn render_slides_markdown(slides: &[Slide]) -> String {
    slides
        .iter()
        .enumerate()
        .map(|(index, slide)| {
            let heading = if slide.title.is_empty() {
                format!("Slide {}", index + 1)
            } else {
                slide.title.clone()
            };
            let mut section = format!("## {heading}\n\n");
            for block in &slide.blocks {
                match block {
                    SlideBlock::Text { text, bulleted } => {
                        if *bulleted {
                            section.push_str("- ");
                        }
                        section.push_str(text);
                    }
                    SlideBlock::Image { alt, source, .. } => {
                        section
                            .push_str(&format!("![{alt}]({})", markdown_link_destination(source)));
                    }
                    SlideBlock::Table(rows) => {
                        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
                        if width == 0 {
                            continue;
                        }
                        let rows = rows
                            .iter()
                            .map(|row| {
                                let mut row = row.clone();
                                row.resize(width, String::new());
                                row
                            })
                            .collect::<Vec<_>>();
                        section.push_str(&markdown_table_from_rows(&rows));
                    }
                }
                section.push_str("\n\n");
            }
            section
        })
        .collect::<Vec<_>>()
        .join("---\n\n")
}

/// Writes images extracted from a presentation next to the Markdown output.
pub(crate) struct ImageSink {
    directory: PathBuf,
    prefix: String,
    count: usize,
    saved: BTreeMap<String, String>,
}

impl ImageSink {
    pub(crate) fn new(output: &Path) -> Self {
        Self {
            directory: output
                .parent()
                .map_or_else(|| PathBuf::from("image"), |parent| parent.join("image")),
            prefix: output
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("slides")
                .to_string(),
            count: 0,
            saved: BTreeMap::new(),
        }
    }

    /// Returns the link of a package part that was already saved.
    pub(crate) fn saved(&self, part: &str) -> Option<String> {
        self.saved.get(part).cloned()
    }

    /// Saves a package part's image and returns its Markdown-relative path.
    pub(crate) fn save(
        &mut self,
        slide_number: usize,
        part: &str,
        bytes: &[u8],
    ) -> Result<String, MarkoffError> {
        if let Some(link) = self.saved(part) {
            return Ok(link);
        }
        self.count += 1;
        let extension = part_extension(part);
        let extension = if extension.is_empty() {
            "bin"
        } else {
            &extension
        };
        let name = format!(
            "{}_slide{slide_number}_{}.{extension}",
            self.prefix, self.count
        );
        std::fs::create_dir_all(&self.directory)?;
        std::fs::write(self.directory.join(&name), bytes)?;
        let link = format!("image/{name}");
        self.saved.insert(part.to_string(), link.clone());
        Ok(link)
    }
}

/// Lower-cased extension of a package part or URL.
pub(crate) fn part_extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Converts the cached data of a DrawingML chart part into table rows.
pub(crate) fn chart_to_table(xml: &str) -> Vec<Vec<String>> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    #[derive(Clone, Copy, PartialEq)]
    enum Section {
        None,
        Name,
        Category,
        Value,
    }
    #[derive(Default)]
    struct Series {
        name: String,
        categories: BTreeMap<usize, String>,
        values: BTreeMap<usize, String>,
    }

    let mut reader = Reader::from_str(xml);
    let mut series = Vec::new();
    let mut current: Option<Series> = None;
    let mut section = Section::None;
    let mut point = 0usize;
    let mut in_value = false;
    let mut title = String::new();
    let mut title_depth = 0usize;
    let mut in_title_text = false;
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        match event {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "ser" => current = Some(Series::default()),
                "tx" if current.is_some() && section == Section::None => section = Section::Name,
                "cat" | "xVal" if current.is_some() => section = Section::Category,
                "val" | "yVal" if current.is_some() => section = Section::Value,
                "pt" => {
                    point = tag
                        .attributes()
                        .flatten()
                        .find(|attribute| attribute.key.local_name().as_ref() == "idx")
                        .and_then(|attribute| attribute.value.parse().ok())
                        .unwrap_or(0);
                }
                "v" => in_value = true,
                "title" if current.is_none() => title_depth += 1,
                "t" if title_depth > 0 => in_title_text = true,
                _ => {}
            },
            Event::Text(text) => {
                let Ok(value) = quick_xml::escape::unescape(text.as_ref()) else {
                    continue;
                };
                if in_title_text {
                    title.push_str(&value);
                } else if in_value && let Some(current) = current.as_mut() {
                    match section {
                        Section::Name => current.name.push_str(&value),
                        Section::Category => {
                            current.categories.insert(point, value.into_owned());
                        }
                        Section::Value => {
                            current.values.insert(point, value.into_owned());
                        }
                        Section::None => {}
                    }
                }
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                "v" => in_value = false,
                "t" => in_title_text = false,
                "title" => title_depth = title_depth.saturating_sub(1),
                "tx" if section == Section::Name => section = Section::None,
                "cat" | "xVal" | "val" | "yVal" => section = Section::None,
                "ser" => {
                    if let Some(finished) = current.take() {
                        series.push(finished);
                    }
                    section = Section::None;
                }
                _ => {}
            },
            _ => {}
        }
    }
    if series.is_empty() {
        return Vec::new();
    }
    let count = series
        .iter()
        .flat_map(|series| series.categories.keys().chain(series.values.keys()))
        .max()
        .map_or(0, |last| last + 1);
    let mut header = vec![if title.trim().is_empty() {
        "Category".to_string()
    } else {
        title.trim().to_string()
    }];
    header.extend(series.iter().enumerate().map(|(index, series)| {
        if series.name.trim().is_empty() {
            format!("Series {}", index + 1)
        } else {
            series.name.trim().to_string()
        }
    }));
    let mut rows = vec![header];
    for index in 0..count {
        let category = series
            .iter()
            .find_map(|series| series.categories.get(&index))
            .cloned()
            .unwrap_or_else(|| (index + 1).to_string());
        let mut row = vec![category];
        row.extend(
            series
                .iter()
                .map(|series| series.values.get(&index).cloned().unwrap_or_default()),
        );
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::{SlideBlock, chart_to_table, parse_markdown_slides, sole_image};
    use std::path::Path;

    #[test]
    fn parses_images_tables_and_text_in_source_order() {
        let slides = parse_markdown_slides(
            "## Data\n\nIntro\n\n| A | B |\n| --- | --- |\n| 1 | **2** |\n\n![Logo](missing.png)\n\n1. step\n",
            Path::new("."),
        );
        let blocks = &slides[0].blocks;
        assert!(
            matches!(&blocks[0], SlideBlock::Text { text, bulleted: false } if text == "Intro")
        );
        assert!(matches!(&blocks[1], SlideBlock::Table(rows) if rows[1] == ["1", "2"]));
        assert!(
            matches!(&blocks[2], SlideBlock::Image { alt, source, data: None } if alt == "Logo" && source == "missing.png")
        );
        assert!(matches!(&blocks[3], SlideBlock::Text { text, bulleted: true } if text == "step"));
    }

    #[test]
    fn only_standalone_images_become_image_blocks() {
        assert!(sole_image("![a](b.png)").is_some());
        assert!(sole_image("see ![a](b.png)").is_none());
        assert!(sole_image("![a](b.png) ![c](d.png)").is_none());
    }

    #[test]
    fn reads_chart_cache_as_table() {
        let xml = r#"<c:chartSpace xmlns:c="c" xmlns:a="a"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Sales</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>2025</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>4</c:v></c:pt><c:pt idx="1"><c:v>5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        assert_eq!(
            chart_to_table(xml),
            vec![
                vec!["Sales".to_string(), "2025".to_string()],
                vec!["Q1".to_string(), "4".to_string()],
                vec!["Q2".to_string(), "5".to_string()],
            ]
        );
    }
}
