use crate::MarkoffError;
use crate::error::invalid_data;
use crate::xml_utils::{MarkdownEscapeContext, markdown_escape, parse_relationships};
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
    let slide_relationship_ids = read_slide_relationship_order(&presentation_xml);

    let mut sections = Vec::new();
    for (index, relationship_id) in slide_relationship_ids.iter().enumerate() {
        let Some(target) = relationship_targets.get(relationship_id) else {
            continue;
        };
        let part_path = resolve_part_path("ppt", target);
        let mut slide_xml = String::new();
        let Ok(mut part) = archive.by_name(&part_path) else {
            continue;
        };
        if part.read_to_string(&mut slide_xml).is_err() {
            continue;
        }
        sections.push(slide_to_markdown(&slide_xml, index + 1));
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

fn read_slide_relationship_order(xml: &str) -> Vec<String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    let mut ids = Vec::new();
    let mut in_slide_id_list = false;
    loop {
        match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(tag)) if tag.local_name().as_ref() == "sldIdLst" => {
                in_slide_id_list = true;
            }
            Ok(Event::End(tag)) if tag.local_name().as_ref() == "sldIdLst" => {
                in_slide_id_list = false;
            }
            Ok(Event::Start(tag) | Event::Empty(tag))
                if in_slide_id_list && tag.local_name().as_ref() == "sldId" =>
            {
                if let Some(id) = tag
                    .attributes()
                    .flatten()
                    .find(|attribute| attribute.key.as_ref() == "r:id")
                    .and_then(|attribute| {
                        attribute
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .ok()
                            .map(|value| value.into_owned())
                    })
                {
                    ids.push(id);
                }
            }
            _ => {}
        }
    }
    ids
}

fn slide_to_markdown(xml: &str, slide_number: usize) -> String {
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

    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
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
                if let Ok(unescaped) = quick_xml::escape::unescape(text.as_ref()) {
                    paragraph_text.push_str(&unescaped);
                }
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
    section
}
