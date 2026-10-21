use crate::MarkoffError;
use crate::docx_inline::{DocxRunStyle, VerticalAlign, markdown_from_docx_run};
use crate::docx_runs::{PendingRun, flush_pending_run, resolve_general_ref, word_property_enabled};
use crate::error::invalid_data;
use crate::xml_utils::{attribute_value, parse_relationships};
use std::collections::BTreeMap;

pub(super) struct EndnoteNumbering {
    format: String,
    next: usize,
    labels: BTreeMap<i64, String>,
}

impl EndnoteNumbering {
    pub(super) fn read(
        archive: &mut zip::ZipArchive<std::fs::File>,
        document: &str,
    ) -> Result<Self, MarkoffError> {
        use std::io::Read;

        let mut numbering = Self {
            format: "lowerRoman".to_string(),
            next: 1,
            labels: BTreeMap::new(),
        };
        let mut settings = String::new();
        match archive.by_name("word/settings.xml") {
            Ok(mut file) => {
                file.read_to_string(&mut settings)?;
            }
            Err(zip::result::ZipError::FileNotFound) => {}
            Err(error) => return Err(invalid_data(error).into()),
        }
        numbering.apply_properties(&settings)?;
        numbering.apply_properties(document)?;
        Ok(numbering)
    }

    fn apply_properties(&mut self, xml: &str) -> Result<(), MarkoffError> {
        use quick_xml::events::Event;

        let mut reader = quick_xml::Reader::from_str(xml);
        let mut in_properties = false;
        loop {
            match reader.read_event().map_err(invalid_data)? {
                Event::Start(event) if event.local_name().as_ref() == "endnotePr" => {
                    in_properties = true;
                }
                Event::Start(event) | Event::Empty(event) if in_properties => {
                    match event.local_name().as_ref() {
                        "numFmt" => {
                            if let Some(format) = attribute_value(&event, "val")? {
                                self.format = format;
                            }
                        }
                        "numStart" => {
                            if let Some(start) = attribute_value(&event, "val")? {
                                self.next = start.parse().map_err(invalid_data)?;
                            }
                        }
                        _ => {}
                    }
                }
                Event::End(event) if event.local_name().as_ref() == "endnotePr" => {
                    in_properties = false;
                }
                Event::Eof => break,
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn label(&mut self, id: i64) -> String {
        if let Some(label) = self.labels.get(&id) {
            return label.clone();
        }
        let number = match self.format.as_str() {
            "lowerRoman" => roman_number(self.next).to_ascii_lowercase(),
            "upperRoman" => roman_number(self.next),
            _ => self.next.to_string(),
        };
        self.next += 1;
        let label = format!("endnote-{number}");
        self.labels.insert(id, label.clone());
        label
    }
}

fn roman_number(mut number: usize) -> String {
    if number == 0 || number > 3999 {
        return number.to_string();
    }
    let mut rendered = String::new();
    for (value, symbol) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while number >= value {
            rendered.push_str(symbol);
            number -= value;
        }
    }
    rendered
}

pub(super) fn read_footnotes(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Result<BTreeMap<i64, String>, MarkoffError> {
    read_notes(archive, "footnotes", "footnote")
}

pub(super) fn read_endnotes(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Result<BTreeMap<i64, String>, MarkoffError> {
    read_notes(archive, "endnotes", "endnote")
}

fn read_notes(
    archive: &mut zip::ZipArchive<std::fs::File>,
    part_name: &str,
    note_element: &str,
) -> Result<BTreeMap<i64, String>, MarkoffError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    use std::io::Read;

    let mut document = String::new();
    {
        let part_path = format!("word/{part_name}.xml");
        let Ok(mut file) = archive.by_name(&part_path) else {
            return Ok(BTreeMap::new());
        };
        file.read_to_string(&mut document)?;
    }
    let relationships =
        if let Ok(mut file) = archive.by_name(&format!("word/_rels/{part_name}.xml.rels")) {
            let mut relationships = String::new();
            file.read_to_string(&mut relationships)?;
            parse_relationships(&relationships)?
        } else {
            BTreeMap::new()
        };

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
    let mut hyperlink_target = None;

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(event) | Event::Empty(event) => match event.local_name().as_ref() {
                name if name == note_element => {
                    footnote_id = event
                        .attributes()
                        .flatten()
                        .find(|attribute| attribute.key.local_name().as_ref() == "id")
                        .map(|attribute| {
                            attribute
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .map_err(invalid_data)?
                                .parse::<i64>()
                                .map_err(invalid_data)
                        })
                        .transpose()?;
                    paragraphs.clear();
                }
                "p" if footnote_id.is_some() => {
                    paragraph.clear();
                    pending_run = None;
                    hyperlink_target = None;
                }
                "r" if footnote_id.is_some() => {
                    run.clear();
                    vert_align = VerticalAlign::Baseline;
                }
                "b" if footnote_id.is_some() => bold = word_property_enabled(&event),
                "i" if footnote_id.is_some() => italic = word_property_enabled(&event),
                "strike" if footnote_id.is_some() => strikethrough = word_property_enabled(&event),
                "u" if footnote_id.is_some() => underline = word_property_enabled(&event),
                "vertAlign" if footnote_id.is_some() => {
                    vert_align = attribute_value(&event, "val")?
                        .map(|value| match value.as_str() {
                            "superscript" => VerticalAlign::Superscript,
                            "subscript" => VerticalAlign::Subscript,
                            _ => VerticalAlign::Baseline,
                        })
                        .unwrap_or(VerticalAlign::Baseline);
                }
                "hyperlink" if footnote_id.is_some() => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    hyperlink_target = if let Some(anchor) = attribute_value(&event, "anchor")? {
                        Some(format!("anchor:{anchor}"))
                    } else {
                        attribute_value(&event, "id")?
                            .and_then(|id| relationships.get(&id))
                            .map(|target| format!("external:{target}"))
                    };
                }
                _ => {}
            },
            Event::Text(event) if footnote_id.is_some() => {
                let decoded = event.as_ref();
                run.push_str(&quick_xml::escape::unescape(decoded).map_err(invalid_data)?)
            }
            Event::GeneralRef(event) if footnote_id.is_some() => {
                run.push_str(&resolve_general_ref(&event)?)
            }
            Event::End(event) => match event.local_name().as_ref() {
                "r" if footnote_id.is_some() => {
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
                            target: hyperlink_target.clone(),
                        });
                    }
                    bold = false;
                    italic = false;
                    strikethrough = false;
                    underline = false;
                    code = false;
                }
                "hyperlink" if footnote_id.is_some() => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    hyperlink_target = None;
                }
                "p" if footnote_id.is_some() => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    if !paragraph.is_empty() {
                        paragraphs.push(paragraph.clone());
                    }
                }
                name if name == note_element => {
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

#[cfg(test)]
mod tests {
    use super::EndnoteNumbering;
    use std::collections::BTreeMap;

    #[test]
    fn endnote_labels_follow_numbering_not_package_ids() {
        let mut numbering = EndnoteNumbering {
            format: "decimal".to_string(),
            next: 1,
            labels: BTreeMap::new(),
        };
        numbering
            .apply_properties(
                "<w:settings xmlns:w=\"urn:test\"><w:footnotePr><w:numFmt w:val=\"decimal\"/></w:footnotePr><w:endnotePr><w:numFmt w:val=\"lowerRoman\"/><w:numStart w:val=\"4\"/></w:endnotePr></w:settings>",
            )
            .unwrap();
        assert_eq!(numbering.label(42), "endnote-iv");
        assert_eq!(numbering.label(42), "endnote-iv");
        assert_eq!(numbering.label(7), "endnote-v");
    }

    #[test]
    fn section_endnote_format_overrides_document_settings() {
        let mut numbering = EndnoteNumbering {
            format: "lowerRoman".to_string(),
            next: 1,
            labels: BTreeMap::new(),
        };
        numbering
            .apply_properties(
                "<w:sectPr xmlns:w=\"urn:test\"><w:endnotePr><w:numFmt w:val=\"upperRoman\"/></w:endnotePr></w:sectPr>",
            )
            .unwrap();
        assert_eq!(numbering.label(10), "endnote-I");
        assert_eq!(numbering.label(20), "endnote-II");
    }
}
