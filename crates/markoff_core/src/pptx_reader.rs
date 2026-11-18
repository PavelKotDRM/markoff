use crate::MarkoffError;
use crate::error::invalid_data;
use crate::slides::{ImageSink, Slide, SlideBlock, chart_to_table, render_slides_markdown};
use crate::xml_utils::{MarkdownEscapeContext, markdown_escape, parse_relationships};
use std::io::{Error, ErrorKind, Read};
use std::path::Path;

enum RawBlock {
    Text(String, bool),
    Image { alt: String, relationship: String },
    Table(Vec<Vec<String>>),
    Chart(String),
}

struct RawSlide {
    title: String,
    blocks: Vec<RawBlock>,
}

pub(crate) fn convert_pptx_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let file = std::fs::File::open(input)?;
    let mut archive = zip::ZipArchive::new(file).map_err(invalid_data)?;

    let mut presentation_xml = String::new();
    archive
        .by_name("ppt/presentation.xml")
        .map_err(invalid_data)?
        .read_to_string(&mut presentation_xml)?;
    let mut rels_xml = String::new();
    archive
        .by_name("ppt/_rels/presentation.xml.rels")
        .map_err(invalid_data)?
        .read_to_string(&mut rels_xml)?;

    let relationship_targets = parse_relationships(&rels_xml)?;
    let slide_relationship_ids = read_slide_relationship_order(&presentation_xml)?;

    let mut slides = Vec::new();
    let mut images = ImageSink::new(output);
    for (index, relationship_id) in slide_relationship_ids.iter().enumerate() {
        let target = relationship_targets.get(relationship_id).ok_or_else(|| {
            pptx_invalid(format!(
                "slide {} references missing relationship `{relationship_id}`",
                index + 1
            ))
        })?;
        let part_path = resolve_part_path("ppt", target);
        let mut slide_xml = String::new();
        let mut part = archive.by_name(&part_path).map_err(|error| {
            pptx_invalid(format!(
                "slide {} part `{part_path}` is missing or unreadable: {error}",
                index + 1
            ))
        })?;
        part.read_to_string(&mut slide_xml).map_err(|error| {
            pptx_invalid(format!(
                "failed to read slide {} part `{part_path}` as XML: {error}",
                index + 1
            ))
        })?;
        drop(part);
        let raw = parse_slide(&slide_xml, index + 1)?;
        let (slide_dir, slide_file) = part_path.rsplit_once('/').unwrap_or(("", &part_path));
        let slide_relationships = read_part_text(
            &mut archive,
            &format!("{slide_dir}/_rels/{slide_file}.rels"),
        )
        .map(|xml| parse_relationships(&xml))
        .transpose()?
        .unwrap_or_default();
        slides.push(resolve_slide(
            raw,
            index + 1,
            slide_dir,
            &slide_relationships,
            &mut archive,
            &mut images,
        )?);
    }

    std::fs::write(output, render_slides_markdown(&slides))?;
    Ok(())
}

fn read_part_bytes(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut part = archive.by_name(name).ok()?;
    let mut bytes = Vec::new();
    part.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn read_part_text(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    String::from_utf8(read_part_bytes(archive, name)?).ok()
}

fn resolve_slide(
    raw: RawSlide,
    slide_number: usize,
    slide_dir: &str,
    relationships: &std::collections::BTreeMap<String, String>,
    archive: &mut zip::ZipArchive<std::fs::File>,
    images: &mut ImageSink,
) -> Result<Slide, MarkoffError> {
    let escape = |text: &str| markdown_escape(text, MarkdownEscapeContext::Plain);
    let escape_rows = |rows: Vec<Vec<String>>| {
        rows.into_iter()
            .map(|row| row.iter().map(|cell| escape(cell)).collect())
            .collect::<Vec<_>>()
    };
    let mut blocks = Vec::new();
    for block in raw.blocks {
        match block {
            RawBlock::Text(text, bulleted) => blocks.push(SlideBlock::Text { text, bulleted }),
            RawBlock::Table(rows) => blocks.push(SlideBlock::Table(escape_rows(rows))),
            RawBlock::Chart(relationship) => {
                let rows = relationships
                    .get(&relationship)
                    .and_then(|target| {
                        read_part_text(archive, &resolve_part_path(slide_dir, target))
                    })
                    .map(|xml| chart_to_table(&xml))
                    .unwrap_or_default();
                if !rows.is_empty() {
                    blocks.push(SlideBlock::Table(escape_rows(rows)));
                }
            }
            RawBlock::Image { alt, relationship } => {
                let Some(target) = relationships.get(&relationship) else {
                    continue;
                };
                let alt = escape(&alt);
                if target.contains("://") {
                    blocks.push(SlideBlock::Image {
                        alt,
                        source: target.clone(),
                        data: None,
                    });
                    continue;
                }
                let part = resolve_part_path(slide_dir, target);
                if let Some(source) = images.saved(&part) {
                    blocks.push(SlideBlock::Image {
                        alt,
                        source,
                        data: None,
                    });
                    continue;
                }
                let Some(bytes) = read_part_bytes(archive, &part) else {
                    tracing::warn!(part, "slide image part is missing");
                    continue;
                };
                let source = images.save(slide_number, &part, &bytes)?;
                blocks.push(SlideBlock::Image {
                    alt,
                    source,
                    data: None,
                });
            }
        }
    }
    Ok(Slide {
        title: raw.title,
        blocks,
    })
}

fn resolve_part_path(base_dir: &str, target: &str) -> String {
    if let Some(stripped) = target.strip_prefix('/') {
        return stripped.to_string();
    }
    let mut segments: Vec<&str> = base_dir
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in target.split('/') {
        match part {
            "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

fn pptx_invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidData, message.into())
}

fn read_slide_relationship_order(xml: &str) -> Result<Vec<String>, Error> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    let mut ids = Vec::new();
    let mut in_slide_id_list = false;
    let mut open_elements = 0usize;
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) if open_elements == 0 => break,
            Ok(Event::Eof) => {
                return Err(pptx_invalid(
                    "invalid ppt/presentation.xml: unexpected end of file",
                ));
            }
            Err(error) => {
                return Err(pptx_invalid(format!(
                    "invalid ppt/presentation.xml: {error}"
                )));
            }
            Ok(event) => event,
        };
        if matches!(&event, Event::Start(_)) {
            open_elements += 1;
        } else if matches!(&event, Event::End(_)) {
            open_elements = open_elements.saturating_sub(1);
        }
        match event {
            Event::Start(tag) if tag.local_name().as_ref() == "sldIdLst" => {
                in_slide_id_list = true;
            }
            Event::End(tag) if tag.local_name().as_ref() == "sldIdLst" => {
                in_slide_id_list = false;
            }
            Event::Start(tag) | Event::Empty(tag)
                if in_slide_id_list && tag.local_name().as_ref() == "sldId" =>
            {
                let mut relationship_id = None;
                for attribute in tag.attributes() {
                    let attribute = attribute.map_err(|error| {
                        pptx_invalid(format!(
                            "invalid slide relationship attribute in ppt/presentation.xml: {error}"
                        ))
                    })?;
                    if attribute.key.as_ref() == "r:id" {
                        relationship_id = Some(
                            attribute
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .map_err(|error| {
                                    pptx_invalid(format!(
                                        "invalid slide relationship ID in ppt/presentation.xml: {error}"
                                    ))
                                })?
                                .into_owned(),
                        );
                    }
                }
                ids.push(relationship_id.ok_or_else(|| {
                    pptx_invalid("slide entry in ppt/presentation.xml is missing r:id")
                })?);
            }
            _ => {}
        }
    }
    Ok(ids)
}

fn tag_attribute(tag: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    tag.attributes()
        .flatten()
        .find(|attribute| attribute.key.local_name().as_ref() == name)
        .and_then(|attribute| {
            attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned())
        })
}

fn parse_slide(xml: &str, slide_number: usize) -> Result<RawSlide, Error> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut title = String::new();
    let mut blocks = Vec::new();

    let mut in_shape = false;
    let mut is_title_shape = false;
    let mut paragraph: Option<(String, bool)> = None;
    let mut in_run_text = false;
    let mut table: Option<Vec<Vec<String>>> = None;
    let mut row: Option<Vec<String>> = None;
    let mut cell: Option<Vec<String>> = None;
    let mut picture: Option<(String, Option<String>)> = None;
    let mut open_elements = 0usize;

    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) if open_elements == 0 => break,
            Ok(Event::Eof) => {
                return Err(pptx_invalid(format!(
                    "invalid XML in slide {slide_number}: unexpected end of file"
                )));
            }
            Err(error) => {
                return Err(pptx_invalid(format!(
                    "invalid XML in slide {slide_number}: {error}"
                )));
            }
            Ok(event) => event,
        };
        if matches!(&event, Event::Start(_)) {
            open_elements += 1;
        } else if matches!(&event, Event::End(_)) {
            open_elements = open_elements.saturating_sub(1);
        }
        match event {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "sp" => {
                    in_shape = true;
                    is_title_shape = false;
                }
                "tbl" => table = Some(Vec::new()),
                "tr" if table.is_some() => row = Some(Vec::new()),
                "tc" if row.is_some() => cell = Some(Vec::new()),
                "p" if in_shape || cell.is_some() => paragraph = Some((String::new(), false)),
                "t" => in_run_text = paragraph.is_some(),
                "pic" => picture = Some((String::new(), None)),
                _ => handle_empty_like(
                    &tag,
                    &mut blocks,
                    &mut picture,
                    &mut paragraph,
                    in_shape,
                    &mut is_title_shape,
                ),
            },
            Event::Empty(tag) => match tag.local_name().as_ref() {
                "tc" => {
                    if let Some(row) = row.as_mut() {
                        row.push(String::new());
                    }
                }
                "br" => {
                    if let Some((text, _)) = paragraph.as_mut() {
                        text.push(if cell.is_some() { '\n' } else { ' ' });
                    }
                }
                _ => handle_empty_like(
                    &tag,
                    &mut blocks,
                    &mut picture,
                    &mut paragraph,
                    in_shape,
                    &mut is_title_shape,
                ),
            },
            Event::Text(text) if in_run_text => {
                let unescaped = quick_xml::escape::unescape(text.as_ref()).map_err(|error| {
                    pptx_invalid(format!(
                        "invalid escaped text in slide {slide_number}: {error}"
                    ))
                })?;
                if let Some((paragraph_text, _)) = paragraph.as_mut() {
                    paragraph_text.push_str(&unescaped);
                }
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                "t" => in_run_text = false,
                "p" => {
                    let Some((paragraph_text, bulleted)) = paragraph.take() else {
                        continue;
                    };
                    if let Some(lines) = cell.as_mut() {
                        lines.push(paragraph_text.trim().to_string());
                        continue;
                    }
                    let text = markdown_escape(paragraph_text.trim(), MarkdownEscapeContext::Plain);
                    if !text.is_empty() {
                        if is_title_shape && title.is_empty() {
                            title = text;
                        } else {
                            blocks.push(RawBlock::Text(text, bulleted));
                        }
                    }
                }
                "tc" => {
                    if let (Some(lines), Some(row)) = (cell.take(), row.as_mut()) {
                        let mut value = lines.join("\n");
                        value.truncate(value.trim_end().len());
                        row.push(value.trim_start().to_string());
                    }
                }
                "tr" => {
                    if let (Some(finished), Some(table)) = (row.take(), table.as_mut()) {
                        table.push(finished);
                    }
                }
                "tbl" => {
                    if let Some(rows) = table.take().filter(|rows| !rows.is_empty()) {
                        blocks.push(RawBlock::Table(rows));
                    }
                }
                "pic" => {
                    if let Some((alt, Some(relationship))) = picture.take() {
                        blocks.push(RawBlock::Image { alt, relationship });
                    }
                }
                "sp" => in_shape = false,
                _ => {}
            },
            _ => {}
        }
    }

    Ok(RawSlide { title, blocks })
}

/// Handles elements whose meaning does not depend on being empty or not.
fn handle_empty_like(
    tag: &quick_xml::events::BytesStart<'_>,
    blocks: &mut Vec<RawBlock>,
    picture: &mut Option<(String, Option<String>)>,
    paragraph: &mut Option<(String, bool)>,
    in_shape: bool,
    is_title_shape: &mut bool,
) {
    match tag.local_name().as_ref() {
        "ph" if in_shape => {
            if matches!(
                tag_attribute(tag, "type").as_deref(),
                Some("title" | "ctrTitle")
            ) {
                *is_title_shape = true;
            }
        }
        "buChar" | "buAutoNum" => {
            if let Some((_, bulleted)) = paragraph.as_mut() {
                *bulleted = true;
            }
        }
        "cNvPr" => {
            if let Some((alt, _)) = picture.as_mut() {
                *alt = tag_attribute(tag, "descr").unwrap_or_default();
            }
        }
        "blip" => {
            if let Some((_, relationship)) = picture.as_mut() {
                *relationship = tag_attribute(tag, "embed").or_else(|| tag_attribute(tag, "link"));
            }
        }
        "chart" => {
            if let Some(relationship) = tag_attribute(tag, "id") {
                blocks.push(RawBlock::Chart(relationship));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{RawBlock, convert_pptx_to_markdown, parse_slide, read_slide_relationship_order};
    use crate::test_support::unique_temp_path;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn rejects_slide_entries_without_relationship_ids() {
        let error = read_slide_relationship_order(
            r#"<p:presentation xmlns:p="p"><p:sldIdLst><p:sldId id="256"/></p:sldIdLst></p:presentation>"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing r:id"));
    }

    #[test]
    fn collects_pictures_tables_and_charts_in_order() {
        let slide = parse_slide(
            r#"<p:sld xmlns:p="p" xmlns:a="a" xmlns:r="r" xmlns:c="c"><p:cSld><p:spTree><p:pic><p:nvPicPr><p:cNvPr id="4" name="Pic" descr="Logo"/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/></p:blipFill></p:pic><p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>A</a:t></a:r></a:p></a:txBody></a:tc><a:tc/></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame><p:graphicFrame><a:graphic><a:graphicData><c:chart r:id="rId3"/></a:graphicData></a:graphic></p:graphicFrame></p:spTree></p:cSld></p:sld>"#,
            1,
        )
        .unwrap();
        assert!(matches!(
            &slide.blocks[..],
            [
                RawBlock::Image { alt, relationship },
                RawBlock::Table(rows),
                RawBlock::Chart(chart),
            ] if alt == "Logo" && relationship == "rId2" && rows == &[vec!["A".to_string(), String::new()]] && chart == "rId3"
        ));
    }

    #[test]
    fn reports_truncated_slide_xml() {
        let Err(error) = parse_slide("<p:sld><p:cSld>", 3) else {
            panic!("truncated XML must fail");
        };
        assert!(error.to_string().contains("slide 3"));
        assert!(error.to_string().contains("unexpected end of file"));
    }

    #[test]
    fn reports_a_missing_slide_part() {
        let pptx = unique_temp_path("pptx_missing_slide", "pptx");
        let output = unique_temp_path("pptx_missing_slide_output", "md");
        let file = std::fs::File::create(&pptx).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        archive.start_file("ppt/presentation.xml", options).unwrap();
        archive
            .write_all(
                br#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#,
            )
            .unwrap();
        archive
            .start_file("ppt/_rels/presentation.xml.rels", options)
            .unwrap();
        archive
            .write_all(
                br#"<Relationships><Relationship Id="rId1" Target="slides/slide1.xml"/></Relationships>"#,
            )
            .unwrap();
        archive.finish().unwrap();

        let error = convert_pptx_to_markdown(&pptx, &output).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("slide 1 part"));
        assert!(message.contains("ppt/slides/slide1.xml"));
        assert!(!output.exists());

        std::fs::remove_file(pptx).ok();
    }
}
