use crate::MarkoffError;
use crate::error::invalid_data;
use crate::xml_utils::{MarkdownEscapeContext, markdown_escape, parse_relationships};
use std::io::{Error, ErrorKind};
use std::path::Path;

pub(crate) fn convert_pptx_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    use std::io::Read;

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

    let mut sections = Vec::new();
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
        sections.push(slide_to_markdown(&slide_xml, index + 1)?);
    }

    let markdown = sections.join("\n---\n\n");
    std::fs::write(output, markdown)?;
    Ok(())
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

fn slide_to_markdown(xml: &str, slide_number: usize) -> Result<String, Error> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut title = String::new();
    let mut body_paragraphs: Vec<(String, bool)> = Vec::new();

    let mut in_shape = false;
    let mut is_title_shape = false;
    let mut in_paragraph = false;
    let mut in_run_text = false;
    let mut paragraph_text = String::new();
    let mut paragraph_bulleted = false;
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
            Event::Start(tag) | Event::Empty(tag) => match tag.local_name().as_ref() {
                "sp" => {
                    in_shape = true;
                    is_title_shape = false;
                }
                "ph" if in_shape => {
                    let is_title = tag.attributes().flatten().any(|attribute| {
                        attribute.key.local_name().as_ref() == "type"
                            && matches!(attribute.value.as_ref(), "title" | "ctrTitle")
                    });
                    if is_title {
                        is_title_shape = true;
                    }
                }
                "p" if in_shape => {
                    in_paragraph = true;
                    paragraph_text.clear();
                    paragraph_bulleted = false;
                }
                "buChar" | "buAutoNum" if in_paragraph => paragraph_bulleted = true,
                "t" if in_paragraph => in_run_text = true,
                _ => {}
            },
            Event::Text(text) if in_run_text => {
                let unescaped = quick_xml::escape::unescape(text.as_ref()).map_err(|error| {
                    pptx_invalid(format!(
                        "invalid escaped text in slide {slide_number}: {error}"
                    ))
                })?;
                paragraph_text.push_str(&unescaped);
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                "t" => in_run_text = false,
                "p" if in_shape => {
                    in_paragraph = false;
                    let text = markdown_escape(paragraph_text.trim(), MarkdownEscapeContext::Plain);
                    if !text.is_empty() {
                        if is_title_shape && title.is_empty() {
                            title = text;
                        } else {
                            body_paragraphs.push((text, paragraph_bulleted));
                        }
                    }
                }
                "sp" => in_shape = false,
                _ => {}
            },
            _ => {}
        }
    }

    let heading = if title.is_empty() {
        format!("Slide {slide_number}")
    } else {
        title
    };
    let mut section = format!("## {heading}\n\n");
    for (text, bulleted) in body_paragraphs {
        if bulleted {
            section.push_str("- ");
        }
        section.push_str(&text);
        section.push_str("\n\n");
    }
    Ok(section)
}

#[cfg(test)]
mod tests {
    use super::{convert_pptx_to_markdown, read_slide_relationship_order, slide_to_markdown};
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
    fn reports_truncated_slide_xml() {
        let error = slide_to_markdown("<p:sld><p:cSld>", 3).unwrap_err();
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
