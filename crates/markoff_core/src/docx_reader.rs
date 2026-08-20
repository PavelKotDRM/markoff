use crate::MarkoffError;
use crate::docx_inline::{markdown_from_docx_run, pageref_target};
use crate::error::invalid_data;
use std::path::Path;

type PendingRun = (String, bool, bool, bool, bool, Option<String>);

pub(crate) fn convert_docx_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    use std::io::Read;

    let file = std::fs::File::open(input)?;
    let mut archive = zip::ZipArchive::new(file).map_err(invalid_data)?;
    let mut document = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(invalid_data)?
        .read_to_string(&mut document)?;

    let mut reader = Reader::from_str(&document);
    reader.config_mut().trim_text(false);
    let mut markdown = Vec::new();
    let mut paragraph = String::new();
    let mut run = String::new();
    let mut pending_run: Option<PendingRun> = None;
    let mut heading_level = None;
    let mut list_numbering_id = None;
    let mut list_level = 0usize;
    let mut bookmarks = Vec::new();
    let mut field_instruction = String::new();
    let mut page_reference = None;
    let mut in_field_result = false;
    let mut in_instruction_text = false;
    let mut bold = false;
    let mut italic = false;
    let mut strikethrough = false;
    let mut underline = false;
    let mut in_paragraph = false;

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(event) | Event::Empty(event) => match event.local_name().as_ref() {
                b"p" => {
                    in_paragraph = true;
                    paragraph.clear();
                    heading_level = None;
                    list_numbering_id = None;
                    list_level = 0;
                    pending_run = None;
                    bookmarks.clear();
                    field_instruction.clear();
                    page_reference = None;
                    in_field_result = false;
                }
                b"r" => run.clear(),
                b"b" => bold = true,
                b"i" => italic = true,
                b"strike" => strikethrough = true,
                b"u" => underline = true,
                b"instrText" => in_instruction_text = true,
                b"bookmarkStart" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"name" {
                            bookmarks.push(
                                attribute
                                    .decode_and_unescape_value(reader.decoder())
                                    .map_err(invalid_data)?
                                    .into_owned(),
                            );
                        }
                    }
                }
                b"fldChar" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"fldCharType" {
                            let field_type = attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(invalid_data)?;
                            match field_type.as_ref() {
                                "begin" => {
                                    field_instruction.clear();
                                    page_reference = None;
                                    in_field_result = false;
                                }
                                "separate" => {
                                    page_reference = pageref_target(&field_instruction);
                                    in_field_result = true;
                                }
                                "end" => in_field_result = false,
                                _ => {}
                            }
                        }
                    }
                }
                b"fldSimple" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"instr" {
                            let instruction = attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(invalid_data)?;
                            page_reference = pageref_target(&instruction);
                            in_field_result = true;
                        }
                    }
                }
                b"pStyle" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"val" {
                            let value = attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(invalid_data)?
                                .into_owned();
                            heading_level = value
                                .strip_prefix("Heading")
                                .and_then(|level| level.parse::<usize>().ok())
                                .filter(|level| (1..=6).contains(level));
                        }
                    }
                }
                b"numId" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"val" {
                            list_numbering_id = Some(
                                attribute
                                    .decode_and_unescape_value(reader.decoder())
                                    .map_err(invalid_data)?
                                    .into_owned(),
                            );
                        }
                    }
                }
                b"ilvl" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"val" {
                            list_level = attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(invalid_data)?
                                .parse()
                                .unwrap_or(0);
                        }
                    }
                }
                _ => {}
            },
            Event::Text(event) if in_paragraph => {
                let text = event.decode().map_err(invalid_data)?;
                if in_instruction_text {
                    field_instruction.push_str(&text);
                } else {
                    run.push_str(&text);
                }
            }
            Event::End(event) => match event.local_name().as_ref() {
                b"instrText" => in_instruction_text = false,
                b"fldSimple" => in_field_result = false,
                b"r" => {
                    if !run.is_empty() {
                        let link_target = in_field_result.then(|| page_reference.clone()).flatten();
                        if let Some((
                            previous_text,
                            previous_bold,
                            previous_italic,
                            previous_strikethrough,
                            previous_underline,
                            previous_target,
                        )) = pending_run.as_mut()
                            && (
                                *previous_bold,
                                *previous_italic,
                                *previous_strikethrough,
                                *previous_underline,
                                previous_target.as_deref(),
                            ) == (
                                bold,
                                italic,
                                strikethrough,
                                underline,
                                link_target.as_deref(),
                            )
                        {
                            previous_text.push_str(&run);
                        } else {
                            flush_pending_run(&mut paragraph, &mut pending_run);
                            pending_run = Some((
                                run.clone(),
                                bold,
                                italic,
                                strikethrough,
                                underline,
                                link_target,
                            ));
                        }
                    }
                    bold = false;
                    italic = false;
                    strikethrough = false;
                    underline = false;
                }
                b"p" => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    if !paragraph.is_empty() {
                        let anchors = bookmarks
                            .iter()
                            .map(|bookmark| format!("<a id=\"{bookmark}\"></a>"))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let prefix = heading_level.map_or_else(
                            || match list_numbering_id.as_deref() {
                                Some("1") => format!("{}- ", "    ".repeat(list_level)),
                                Some(_) => format!("{}1. ", "    ".repeat(list_level)),
                                None => String::new(),
                            },
                            |level| "#".repeat(level) + " ",
                        );
                        let separator = if anchors.is_empty() { "" } else { "\n" };
                        markdown.push(format!("{anchors}{separator}{prefix}{paragraph}"));
                    }
                    in_paragraph = false;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    std::fs::write(output, markdown.join("\n\n") + "\n")?;
    Ok(())
}

fn flush_pending_run(paragraph: &mut String, pending_run: &mut Option<PendingRun>) {
    if let Some((text, bold, italic, strikethrough, underline, target)) = pending_run.take() {
        paragraph.push_str(&markdown_from_docx_run(
            &text,
            bold,
            italic,
            strikethrough,
            underline,
            target.as_deref(),
        ));
    }
}
