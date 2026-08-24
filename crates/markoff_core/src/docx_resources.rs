use crate::MarkoffError;
use crate::error::invalid_data;
use crate::xml_utils::{attribute_value, parse_relationships};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(super) enum ListKind {
    Bullet,
    Decimal { start: usize },
}

fn numbering_key(
    abstract_number: &Option<String>,
    level: Option<usize>,
) -> Option<(String, usize)> {
    Some((abstract_number.as_ref()?.clone(), level?))
}

pub(super) fn read_relationships(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Result<BTreeMap<String, String>, MarkoffError> {
    use std::io::Read;

    let Ok(mut file) = archive.by_name("word/_rels/document.xml.rels") else {
        return Ok(BTreeMap::new());
    };
    let mut document = String::new();
    file.read_to_string(&mut document)?;
    parse_relationships(&document)
}

pub(super) fn read_media_part(
    archive: &mut zip::ZipArchive<std::fs::File>,
    target: &str,
) -> Option<Vec<u8>> {
    use std::io::Read;

    let path = target
        .strip_prefix("../")
        .map(str::to_string)
        .unwrap_or_else(|| format!("word/{target}"));
    let mut file = archive.by_name(&path).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

pub(super) fn read_numbering(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Result<BTreeMap<(String, usize), ListKind>, MarkoffError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    use std::io::Read;

    let Ok(mut file) = archive.by_name("word/numbering.xml") else {
        return Ok(BTreeMap::new());
    };
    let mut document = String::new();
    file.read_to_string(&mut document)?;

    let mut reader = Reader::from_str(&document);
    let mut abstract_number = None;
    let mut level = None;
    let mut formats = BTreeMap::new();
    let mut starts = BTreeMap::new();
    let mut number_to_abstract = BTreeMap::new();
    let mut number_id = None;

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(event) | Event::Empty(event) => match event.local_name().as_ref() {
                "abstractNum" => abstract_number = attribute_value(&event, "abstractNumId")?,
                "num" => number_id = attribute_value(&event, "numId")?,
                "lvl" => {
                    level = attribute_value(&event, "ilvl")?.and_then(|value| value.parse().ok());
                }
                "abstractNumId" if number_id.is_some() => {
                    if let Some(abstract_id) = attribute_value(&event, "val")?
                        && let Some(number_id) = number_id.clone()
                    {
                        number_to_abstract.insert(number_id, abstract_id);
                    }
                }
                "numFmt" => {
                    if let (Some(format), Some(key)) = (
                        attribute_value(&event, "val")?,
                        numbering_key(&abstract_number, level),
                    ) {
                        let kind = if format == "bullet" {
                            ListKind::Bullet
                        } else {
                            ListKind::Decimal {
                                start: starts.get(&key).copied().unwrap_or(1),
                            }
                        };
                        formats.insert(key, kind);
                    }
                }
                "start" => {
                    if let (Some(start), Some(key)) = (
                        attribute_value(&event, "val")?.and_then(|value| value.parse().ok()),
                        numbering_key(&abstract_number, level),
                    ) {
                        starts.insert(key.clone(), start);
                        if let Some(ListKind::Decimal { start: current }) = formats.get_mut(&key) {
                            *current = start;
                        }
                    }
                }
                _ => {}
            },
            Event::End(event) => match event.local_name().as_ref() {
                "lvl" => level = None,
                "abstractNum" => abstract_number = None,
                "num" => number_id = None,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(number_to_abstract
        .into_iter()
        .flat_map(|(number_id, abstract_id)| {
            formats
                .iter()
                .filter(move |((id, _), _)| *id == abstract_id)
                .map(move |((_, level), kind)| ((number_id.clone(), *level), *kind))
        })
        .collect())
}
