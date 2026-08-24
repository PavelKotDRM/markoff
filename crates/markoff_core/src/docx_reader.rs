use crate::MarkoffError;
use crate::docx_footnotes::read_footnotes;
use crate::docx_inline::{DocxRunStyle, VerticalAlign, pageref_target};
use crate::docx_markdown::{list_prefix, table_from_rows, textual_list_item};
use crate::docx_postprocess::{convert_formula_section, convert_textual_footnotes};
use crate::docx_resources::{read_media_part, read_numbering, read_relationships};
use crate::docx_runs::{
    PendingRun, flush_pending_run, queue_docx_run, resolve_general_ref, word_property_enabled,
};
use crate::error::invalid_data;
use crate::xml_utils::attribute_value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

struct RunProperties {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    underline: bool,
    code: bool,
    vertical_align: VerticalAlign,
}

impl Default for RunProperties {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            strikethrough: false,
            underline: false,
            code: false,
            vertical_align: VerticalAlign::Baseline,
        }
    }
}

impl RunProperties {
    fn style(&self, code_block: bool) -> DocxRunStyle {
        DocxRunStyle {
            bold: self.bold,
            italic: self.italic,
            strikethrough: self.strikethrough,
            underline: self.underline,
            code: self.code && !code_block,
            vertical_align: self.vertical_align,
        }
    }

    fn start_run(&mut self) {
        self.code = false;
        self.vertical_align = VerticalAlign::Baseline;
    }
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
    let relationships = read_relationships(&mut archive)?;
    let image_dir = output
        .parent()
        .map_or_else(|| PathBuf::from("image"), |parent| parent.join("image"));
    let mut image_counter = 0usize;

    let mut reader = Reader::from_str(&document);
    reader.config_mut().trim_text(false);
    let mut markdown = Vec::new();
    let mut paragraph = String::new();
    let mut raw_paragraph = String::new();
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
    let mut run_properties = RunProperties::default();
    let mut paragraph_has_code = false;
    let mut paragraph_has_plain_text = false;
    let mut in_paragraph = false;
    let mut referenced_footnotes = BTreeSet::new();
    let mut table_rows = Vec::new();
    let mut table_row = Vec::new();
    let mut table_cell_paragraphs = Vec::new();
    let mut in_table = false;
    let mut in_table_cell = false;
    let mut list_counters = BTreeMap::new();
    let mut image_rel_id = None;
    let mut image_alt = String::new();

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(event) | Event::Empty(event) => match event.local_name().as_ref() {
                "tbl" => {
                    in_table = true;
                    table_rows.clear();
                }
                "tr" if in_table => table_row.clear(),
                "tc" if in_table => {
                    in_table_cell = true;
                    table_cell_paragraphs.clear();
                }
                "p" => {
                    in_paragraph = true;
                    paragraph.clear();
                    raw_paragraph.clear();
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
                    paragraph_has_code = false;
                    paragraph_has_plain_text = false;
                }
                "r" => {
                    run.clear();
                    run_properties.start_run();
                }
                "b" => run_properties.bold = word_property_enabled(&event),
                "i" => run_properties.italic = word_property_enabled(&event),
                "strike" => run_properties.strikethrough = word_property_enabled(&event),
                "u" => run_properties.underline = word_property_enabled(&event),
                "vertAlign" => {
                    run_properties.vertical_align = attribute_value(&event, "val")?
                        .map(|value| match value.as_str() {
                            "superscript" => VerticalAlign::Superscript,
                            "subscript" => VerticalAlign::Subscript,
                            _ => VerticalAlign::Baseline,
                        })
                        .unwrap_or(VerticalAlign::Baseline);
                }
                "drawing" => {
                    image_rel_id = None;
                    image_alt.clear();
                }
                "docPr" => {
                    if let Some(description) = attribute_value(&event, "descr")?
                        .filter(|value| !value.is_empty())
                        .or(attribute_value(&event, "name")?)
                    {
                        image_alt = description;
                    }
                }
                "blip" => {
                    image_rel_id = attribute_value(&event, "embed")?;
                }
                "rFonts" => {
                    run_properties.code = event.attributes().flatten().any(|attribute| {
                        attribute.key.local_name().as_ref() == "ascii"
                            && attribute
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .is_ok_and(|value| {
                                    value.eq_ignore_ascii_case("Consolas")
                                        || value.eq_ignore_ascii_case("Courier New")
                                })
                    });
                }
                "tab" if in_paragraph => {
                    let trailing_is_whitespace = run
                        .chars()
                        .last()
                        .or_else(|| {
                            pending_run
                                .as_ref()
                                .and_then(|pending| pending.text.chars().last())
                        })
                        .or_else(|| paragraph.chars().last())
                        .is_some_and(char::is_whitespace);
                    if !trailing_is_whitespace {
                        run.push(' ');
                    }
                }
                "br" if in_paragraph => {
                    if !run.is_empty() {
                        raw_paragraph.push_str(&run);
                        paragraph_has_code |= run_properties.code;
                        paragraph_has_plain_text |= !run_properties.code;
                        queue_docx_run(
                            &mut paragraph,
                            &mut pending_run,
                            &run,
                            run_properties.style(code_block),
                            in_field_result.then(|| page_reference.clone()).flatten(),
                        );
                        run.clear();
                    }
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    paragraph.push_str("  \n");
                    raw_paragraph.push('\n');
                }
                "instrText" => in_instruction_text = true,
                "bookmarkStart" => {
                    if let Some(name) = attribute_value(&event, "name")? {
                        bookmarks.push(name);
                    }
                }
                "fldChar" => {
                    if let Some(field_type) = attribute_value(&event, "fldCharType")? {
                        match field_type.as_str() {
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
                "fldSimple" => {
                    if let Some(instruction) = attribute_value(&event, "instr")? {
                        page_reference = pageref_target(&instruction);
                        in_field_result = true;
                    }
                }
                "footnoteReference" => {
                    if let Some(id) =
                        attribute_value(&event, "id")?.and_then(|value| value.parse::<i64>().ok())
                    {
                        flush_pending_run(&mut paragraph, &mut pending_run);
                        paragraph.push_str(&format!("[^{id}]"));
                        referenced_footnotes.insert(id);
                    }
                }
                "pStyle" => {
                    if let Some(value) = attribute_value(&event, "val")? {
                        heading_level = value
                            .strip_prefix("Heading")
                            .and_then(|level| level.parse::<usize>().ok())
                            .filter(|level| (1..=6).contains(level));
                        code_block = value == "CodeBlock";
                        quote = value == "Quote";
                    }
                }
                "bottom" if in_paragraph => {
                    horizontal_rule = event.attributes().flatten().any(|attribute| {
                        attribute.key.local_name().as_ref() == "val"
                            && attribute
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .is_ok_and(|value| {
                                    !value.eq_ignore_ascii_case("nil")
                                        && !value.eq_ignore_ascii_case("none")
                                })
                    });
                }
                "numId" => {
                    list_numbering_id = attribute_value(&event, "val")?;
                }
                "ilvl" => {
                    list_level = attribute_value(&event, "val")?
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0);
                }
                _ => {}
            },
            Event::Text(event) if in_paragraph => {
                let decoded = event.as_ref();
                let text = quick_xml::escape::unescape(decoded).map_err(invalid_data)?;
                if in_instruction_text {
                    field_instruction.push_str(&text);
                } else {
                    run.push_str(&text);
                }
            }
            Event::GeneralRef(event) if in_paragraph => {
                let text = resolve_general_ref(&event)?;
                if in_instruction_text {
                    field_instruction.push_str(&text);
                } else {
                    run.push_str(&text);
                }
            }
            Event::End(event) => match event.local_name().as_ref() {
                "instrText" => in_instruction_text = false,
                "fldSimple" => in_field_result = false,
                "drawing" if in_paragraph => {
                    if let Some(rel_id) = image_rel_id.take()
                        && let Some(target) = relationships.get(&rel_id)
                        && let Some(bytes) = read_media_part(&mut archive, target)
                    {
                        let extension = Path::new(target)
                            .extension()
                            .and_then(std::ffi::OsStr::to_str)
                            .unwrap_or("png");
                        image_counter += 1;
                        let file_name = format!("image{image_counter}.{extension}");
                        if std::fs::create_dir_all(&image_dir).is_ok()
                            && std::fs::write(image_dir.join(&file_name), bytes).is_ok()
                        {
                            flush_pending_run(&mut paragraph, &mut pending_run);
                            paragraph.push_str(&format!("![{image_alt}](image/{file_name})"));
                        }
                    }
                    image_alt.clear();
                }
                "r" => {
                    if !run.is_empty() {
                        raw_paragraph.push_str(&run);
                        paragraph_has_code |= run_properties.code;
                        paragraph_has_plain_text |= !run_properties.code;
                        queue_docx_run(
                            &mut paragraph,
                            &mut pending_run,
                            &run,
                            run_properties.style(code_block),
                            in_field_result.then(|| page_reference.clone()).flatten(),
                        );
                    }
                    run_properties = RunProperties::default();
                }
                "p" => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    if horizontal_rule || !paragraph.is_empty() {
                        if in_table_cell {
                            table_cell_paragraphs.push(paragraph.clone());
                        } else if horizontal_rule {
                            markdown.push("---".to_string());
                        } else if code_block
                            || (paragraph_has_code
                                && !paragraph_has_plain_text
                                && paragraph.starts_with('`')
                                && paragraph.ends_with('`'))
                        {
                            markdown.push(format!("```\n{raw_paragraph}\n```"));
                        } else {
                            let anchors = bookmarks
                                .iter()
                                .map(|bookmark| format!("<a id=\"{bookmark}\"></a>"))
                                .collect::<Vec<_>>()
                                .join("\n");
                            let textual_list_item = (heading_level.is_none()
                                && list_numbering_id.is_none())
                            .then(|| textual_list_item(&paragraph))
                            .flatten();
                            let is_list_item = heading_level.is_none()
                                && (list_numbering_id.is_some() || textual_list_item.is_some());
                            let (prefix, content) = textual_list_item.unwrap_or_else(|| {
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
                            });
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
                "tc" if in_table_cell => {
                    table_row.push(table_cell_paragraphs.join("<br>"));
                    in_table_cell = false;
                }
                "tr" if in_table => {
                    if !table_row.is_empty() {
                        table_rows.push(table_row.clone());
                    }
                }
                "tbl" if in_table => {
                    if !table_rows.is_empty() {
                        markdown.push(table_from_rows(&table_rows));
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
    convert_textual_footnotes(&mut markdown);
    convert_formula_section(&mut markdown);
    std::fs::write(output, markdown.join("\n\n") + "\n")?;
    Ok(())
}
