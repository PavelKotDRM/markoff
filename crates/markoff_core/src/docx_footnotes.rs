use crate::MarkoffError;
use crate::docx_inline::{DocxRunStyle, VerticalAlign, markdown_from_docx_run};
use crate::docx_runs::{PendingRun, flush_pending_run, resolve_general_ref, word_property_enabled};
use crate::error::invalid_data;
use crate::xml_utils::attribute_value;
use quick_xml::XmlVersion;
use std::collections::BTreeMap;

pub(super) fn read_footnotes(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Result<BTreeMap<i64, String>, MarkoffError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    use std::io::Read;

    let Ok(mut file) = archive.by_name("word/footnotes.xml") else {
        return Ok(BTreeMap::new());
    };
    let mut document = String::new();
    file.read_to_string(&mut document)?;

    let mut reader = Reader::from_str(&document);
    reader.config_mut().trim_text(false);
    let mut footnotes = BTreeMap::new();
    let mut footnote_id = None;
    let mut paragraphs = Vec::new();
    let mut paragraph = String::new();
    let mut run = String::new();
    let mut pending_run: Option<PendingRun> = None;
    let mut bold = false;
    let mut italic = false;
    let mut strikethrough = false;
    let mut underline = false;
    let mut code = false;
    let mut vert_align = VerticalAlign::Baseline;

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(event) | Event::Empty(event) => match event.local_name().as_ref() {
                b"footnote" => {
                    footnote_id = event
                        .attributes()
                        .flatten()
                        .find(|attribute| attribute.key.local_name().as_ref() == b"id")
                        .map(|attribute| {
                            attribute
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
                                .map_err(invalid_data)?
                                .parse::<i64>()
                                .map_err(invalid_data)
                        })
                        .transpose()?;
                    paragraphs.clear();
                }
                b"p" if footnote_id.is_some() => {
                    paragraph.clear();
                    pending_run = None;
                }
                b"r" if footnote_id.is_some() => {
                    run.clear();
                    vert_align = VerticalAlign::Baseline;
                }
                b"b" if footnote_id.is_some() => {
                    bold = word_property_enabled(&event, reader.decoder())
                }
                b"i" if footnote_id.is_some() => {
                    italic = word_property_enabled(&event, reader.decoder())
                }
                b"strike" if footnote_id.is_some() => {
                    strikethrough = word_property_enabled(&event, reader.decoder())
                }
                b"u" if footnote_id.is_some() => {
                    underline = word_property_enabled(&event, reader.decoder())
                }
                b"vertAlign" if footnote_id.is_some() => {
                    vert_align = attribute_value(&event, b"val", reader.decoder())?
                        .map(|value| match value.as_str() {
                            "superscript" => VerticalAlign::Superscript,
                            "subscript" => VerticalAlign::Subscript,
                            _ => VerticalAlign::Baseline,
                        })
                        .unwrap_or(VerticalAlign::Baseline);
                }
                _ => {}
            },
            Event::Text(event) if footnote_id.is_some() => {
                let decoded = event.decode().map_err(invalid_data)?;
                run.push_str(&quick_xml::escape::unescape(&decoded).map_err(invalid_data)?)
            }
            Event::GeneralRef(event) if footnote_id.is_some() => {
                run.push_str(&resolve_general_ref(&event, reader.decoder())?)
            }
            Event::End(event) => match event.local_name().as_ref() {
                b"r" if footnote_id.is_some() => {
                    if !run.is_empty() {
                        if let Some(previous) = pending_run.take() {
                            paragraph.push_str(&markdown_from_docx_run(
                                &previous.text,
                                previous.style,
                                previous.target.as_deref(),
                            ));
                        }
                        pending_run = Some(PendingRun {
                            text: run.clone(),
                            style: DocxRunStyle {
                                bold,
                                italic,
                                strikethrough,
                                underline,
                                code,
                                vertical_align: vert_align,
                            },
                            target: None,
                        });
                    }
                    bold = false;
                    italic = false;
                    strikethrough = false;
                    underline = false;
                    code = false;
                }
                b"p" if footnote_id.is_some() => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    if !paragraph.is_empty() {
                        paragraphs.push(paragraph.clone());
                    }
                }
                b"footnote" => {
                    if let Some(id) = footnote_id.take()
                        && id > 0
                    {
                        footnotes.insert(id, paragraphs.join("\n\n"));
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(footnotes)
}
