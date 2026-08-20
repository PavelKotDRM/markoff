use crate::MarkoffError;
use crate::docx_inline::{markdown_from_docx_run, pageref_target};
use crate::error::invalid_data;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

type PendingRun = (String, bool, bool, bool, bool, bool, Option<String>);

#[derive(Clone, Copy)]
enum ListKind {
    Bullet,
    Decimal { start: usize },
}

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
    let numbering = read_numbering(&mut archive)?;
    let footnotes = read_footnotes(&mut archive)?;

    let mut reader = Reader::from_str(&document);
    reader.config_mut().trim_text(false);
    let mut markdown = Vec::new();
    let mut paragraph = String::new();
    let mut run = String::new();
    let mut pending_run: Option<PendingRun> = None;
    let mut heading_level = None;
    let mut code_block = false;
    let mut quote = false;
    let mut horizontal_rule = false;
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
    let mut code = false;
    let mut in_paragraph = false;
    let mut referenced_footnotes = BTreeSet::new();
    let mut table_rows = Vec::new();
    let mut table_row = Vec::new();
    let mut table_cell_paragraphs = Vec::new();
    let mut in_table = false;
    let mut in_table_cell = false;
    let mut list_counters = BTreeMap::new();

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(event) | Event::Empty(event) => match event.local_name().as_ref() {
                b"tbl" => {
                    in_table = true;
                    table_rows.clear();
                }
                b"tr" if in_table => table_row.clear(),
                b"tc" if in_table => {
                    in_table_cell = true;
                    table_cell_paragraphs.clear();
                }
                b"p" => {
                    in_paragraph = true;
                    paragraph.clear();
                    heading_level = None;
                    code_block = false;
                    quote = false;
                    horizontal_rule = false;
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
                b"rFonts" => {
                    code = event.attributes().flatten().any(|attribute| {
                        attribute.key.local_name().as_ref() == b"ascii"
                            && attribute
                                .decode_and_unescape_value(reader.decoder())
                                .is_ok_and(|value| value.eq_ignore_ascii_case("Consolas"))
                    });
                }
                b"tab" if in_paragraph => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    if paragraph
                        .chars()
                        .last()
                        .is_some_and(|character| !character.is_whitespace())
                    {
                        paragraph.push(' ');
                    }
                }
                b"br" if in_paragraph => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    paragraph.push('\n');
                }
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
                b"footnoteReference" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"id"
                            && let Ok(id) = attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(invalid_data)?
                                .parse::<i64>()
                        {
                            flush_pending_run(&mut paragraph, &mut pending_run);
                            paragraph.push_str(&format!("[^{id}]"));
                            referenced_footnotes.insert(id);
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
                            code_block = value == "CodeBlock";
                            quote = value == "Quote";
                        }
                    }
                }
                b"bottom" if in_paragraph => horizontal_rule = true,
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
                        let run_code = code && !code_block;
                        if let Some((
                            previous_text,
                            previous_bold,
                            previous_italic,
                            previous_strikethrough,
                            previous_underline,
                            previous_code,
                            previous_target,
                        )) = pending_run.as_mut()
                            && (
                                *previous_bold,
                                *previous_italic,
                                *previous_strikethrough,
                                *previous_underline,
                                *previous_code,
                                previous_target.as_deref(),
                            ) == (
                                bold,
                                italic,
                                strikethrough,
                                underline,
                                run_code,
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
                                run_code,
                                link_target,
                            ));
                        }
                    }
                    bold = false;
                    italic = false;
                    strikethrough = false;
                    underline = false;
                    code = false;
                }
                b"p" => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    if horizontal_rule || !paragraph.is_empty() {
                        if in_table_cell {
                            table_cell_paragraphs.push(paragraph.clone());
                        } else if horizontal_rule {
                            markdown.push("---".to_string());
                        } else if code_block {
                            markdown.push(format!("```\n{paragraph}\n```"));
                        } else {
                            let anchors = bookmarks
                                .iter()
                                .map(|bookmark| format!("<a id=\"{bookmark}\"></a>"))
                                .collect::<Vec<_>>()
                                .join("\n");
                            let textual_list_item = (heading_level.is_none()
                                && list_numbering_id.is_none())
                            .then(|| markdown_textual_list_item(&paragraph))
                            .flatten();
                            let is_list_item = heading_level.is_none()
                                && (list_numbering_id.is_some() || textual_list_item.is_some());
                            let (prefix, content) = textual_list_item.map_or_else(
                                || {
                                    (
                                        heading_level.map_or_else(
                                            || {
                                                list_prefix(
                                                    list_numbering_id.as_deref(),
                                                    list_level,
                                                    &numbering,
                                                    &mut list_counters,
                                                )
                                            },
                                            |level| "#".repeat(level) + " ",
                                        ),
                                        paragraph.clone(),
                                    )
                                },
                                |(prefix, content)| (prefix, content),
                            );
                            let rendered = if anchors.is_empty() {
                                format!("{prefix}{content}")
                            } else if !is_list_item {
                                format!("{anchors}\n{prefix}{content}")
                            } else {
                                format!("{prefix}{anchors}{content}")
                            };
                            markdown.push(if quote {
                                format!("> {rendered}")
                            } else {
                                rendered
                            });
                        }
                    }
                    in_paragraph = false;
                }
                b"tc" if in_table_cell => {
                    table_row.push(table_cell_paragraphs.join("<br>"));
                    in_table_cell = false;
                }
                b"tr" if in_table => {
                    if !table_row.is_empty() {
                        table_rows.push(table_row.clone());
                    }
                }
                b"tbl" if in_table => {
                    if !table_rows.is_empty() {
                        markdown.push(markdown_table_from_docx_rows(&table_rows));
                    }
                    in_table = false;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    for id in referenced_footnotes {
        if let Some(content) = footnotes.get(&id) {
            let mut paragraphs = content.split("\n\n");
            if let Some(first) = paragraphs.next() {
                let mut definition = format!("[^{id}]: {first}");
                for paragraph in paragraphs {
                    definition.push_str("\n\n    ");
                    definition.push_str(paragraph);
                }
                markdown.push(definition);
            }
        }
    }
    std::fs::write(output, markdown.join("\n\n") + "\n")?;
    Ok(())
}

fn markdown_textual_list_item(paragraph: &str) -> Option<(String, String)> {
    let (marker, value) = ["**", "__", "~~", "*", "_"]
        .into_iter()
        .find_map(|marker| {
            paragraph
                .strip_prefix(marker)
                .and_then(|value| value.strip_suffix(marker))
                .map(|value| (marker, value))
        })
        .unwrap_or(("", paragraph));
    let separator = value.find(char::is_whitespace)?;
    let label = &value[..separator];
    let (label, parenthesized) = match (label.strip_suffix('.'), label.strip_suffix(')')) {
        (Some(label), _) => (label, false),
        (_, Some(label)) => (label, true),
        _ => return None,
    };
    let levels = label
        .split('.')
        .map(str::parse::<usize>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let (&number, parents) = levels.split_last()?;
    (parenthesized || !parents.is_empty()).then(|| {
        (
            format!("{}{}. ", "    ".repeat(parents.len()), number),
            format!("{marker}{}{marker}", value[separator..].trim_start()),
        )
    })
}

fn list_prefix(
    numbering_id: Option<&str>,
    level: usize,
    numbering: &BTreeMap<(String, usize), ListKind>,
    counters: &mut BTreeMap<(String, usize), usize>,
) -> String {
    let Some(numbering_id) = numbering_id else {
        return String::new();
    };
    let kind = numbering
        .get(&(numbering_id.to_string(), level))
        .copied()
        .unwrap_or(if numbering_id == "1" {
            ListKind::Bullet
        } else {
            ListKind::Decimal { start: 1 }
        });
    let indentation = "    ".repeat(level);

    match kind {
        ListKind::Bullet => format!("{indentation}- "),
        ListKind::Decimal { start } => {
            let key = (numbering_id.to_string(), level);
            let number = *counters.entry(key).or_insert(start);
            counters.insert((numbering_id.to_string(), level), number + 1);
            counters.retain(|(id, child_level), _| id != numbering_id || *child_level <= level);
            format!("{indentation}{number}. ")
        }
    }
}

fn read_numbering(
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
                b"abstractNum" => {
                    abstract_number = attribute_value(&event, b"abstractNumId", reader.decoder())?
                }
                b"num" => number_id = attribute_value(&event, b"numId", reader.decoder())?,
                b"lvl" => {
                    level = attribute_value(&event, b"ilvl", reader.decoder())?
                        .and_then(|value| value.parse().ok());
                }
                b"abstractNumId" if number_id.is_some() => {
                    if let Some(abstract_id) = attribute_value(&event, b"val", reader.decoder())? {
                        number_to_abstract
                            .insert(number_id.clone().expect("checked above"), abstract_id);
                    }
                }
                b"numFmt" if abstract_number.is_some() && level.is_some() => {
                    if let Some(format) = attribute_value(&event, b"val", reader.decoder())? {
                        let key = (
                            abstract_number.clone().expect("checked above"),
                            level.expect("checked above"),
                        );
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
                b"start" if abstract_number.is_some() && level.is_some() => {
                    if let Some(start) = attribute_value(&event, b"val", reader.decoder())?
                        .and_then(|value| value.parse().ok())
                    {
                        let key = (
                            abstract_number.clone().expect("checked above"),
                            level.expect("checked above"),
                        );
                        starts.insert(key.clone(), start);
                        if let Some(ListKind::Decimal { start: current }) = formats.get_mut(&key) {
                            *current = start;
                        }
                    }
                }
                _ => {}
            },
            Event::End(event) => match event.local_name().as_ref() {
                b"lvl" => level = None,
                b"abstractNum" => abstract_number = None,
                b"num" => number_id = None,
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

fn attribute_value(
    event: &quick_xml::events::BytesStart<'_>,
    name: &[u8],
    decoder: quick_xml::encoding::Decoder,
) -> Result<Option<String>, MarkoffError> {
    Ok(event
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.local_name().as_ref() == name)
        .map(|attribute| {
            attribute
                .decode_and_unescape_value(decoder)
                .map(|value| value.into_owned())
                .map_err(invalid_data)
        })
        .transpose()?)
}

fn markdown_table_from_docx_rows(rows: &[Vec<String>]) -> String {
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if column_count == 0 {
        return String::new();
    }

    let format_row = |row: &[String]| {
        let cells = (0..column_count)
            .map(|index| {
                row.get(index)
                    .map_or("", String::as_str)
                    .replace('\\', "\\\\")
                    .replace('|', "\\|")
            })
            .collect::<Vec<_>>();
        format!("| {} |", cells.join(" | "))
    };

    let mut markdown = vec![format_row(&rows[0])];
    markdown.push(format!("| {} |", vec!["---"; column_count].join(" | ")));
    markdown.extend(rows.iter().skip(1).map(|row| format_row(row)));
    markdown.join("\n")
}

fn read_footnotes(
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
                                .decode_and_unescape_value(reader.decoder())
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
                b"r" if footnote_id.is_some() => run.clear(),
                b"b" if footnote_id.is_some() => bold = true,
                b"i" if footnote_id.is_some() => italic = true,
                b"strike" if footnote_id.is_some() => strikethrough = true,
                b"u" if footnote_id.is_some() => underline = true,
                _ => {}
            },
            Event::Text(event) if footnote_id.is_some() => {
                run.push_str(&event.decode().map_err(invalid_data)?)
            }
            Event::End(event) => match event.local_name().as_ref() {
                b"r" if footnote_id.is_some() => {
                    if !run.is_empty() {
                        if let Some((
                            text,
                            previous_bold,
                            previous_italic,
                            previous_strikethrough,
                            previous_underline,
                            previous_code,
                            target,
                        )) = pending_run.take()
                        {
                            paragraph.push_str(&markdown_from_docx_run(
                                &text,
                                previous_bold,
                                previous_italic,
                                previous_strikethrough,
                                previous_underline,
                                previous_code,
                                target.as_deref(),
                            ));
                        }
                        pending_run = Some((
                            run.clone(),
                            bold,
                            italic,
                            strikethrough,
                            underline,
                            code,
                            None,
                        ));
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

fn flush_pending_run(paragraph: &mut String, pending_run: &mut Option<PendingRun>) {
    if let Some((text, bold, italic, strikethrough, underline, code, target)) = pending_run.take() {
        paragraph.push_str(&markdown_from_docx_run(
            &text,
            bold,
            italic,
            strikethrough,
            underline,
            code,
            target.as_deref(),
        ));
    }
}
