use crate::MarkoffError;
use crate::docx_footnotes::{EndnoteNumbering, read_endnotes, read_footnotes};
use crate::docx_graphics::extract_svg_graphics;
use crate::docx_inline::{DocxRunStyle, VerticalAlign, pageref_target};
use crate::docx_markdown::{heading_anchor, list_prefix, table_from_rows, textual_list_item};
use crate::docx_math::MathNode;
use crate::docx_postprocess::{convert_formula_section, convert_textual_footnotes};
use crate::docx_resources::{read_media_part, read_numbering, read_relationships};
use crate::docx_runs::{
    PendingRun, flush_pending_run, queue_docx_run, resolve_general_ref, word_property_enabled,
};
use crate::error::invalid_data;
use crate::xml_utils::{attribute_value, xml_escape};
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum VerticalMerge {
    Restart,
    Continue,
}

struct TableCell {
    content: String,
    colspan: usize,
    vertical_merge: Option<VerticalMerge>,
}

fn render_table(rows: &[Vec<TableCell>]) -> String {
    let has_merged_cells = rows
        .iter()
        .flatten()
        .any(|cell| cell.colspan > 1 || cell.vertical_merge.is_some());
    if !has_merged_cells {
        let rows = rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.content.clone()).collect())
            .collect::<Vec<Vec<String>>>();
        return table_from_rows(&rows);
    }

    let mut rowspans = rows
        .iter()
        .map(|row| vec![1usize; row.len()])
        .collect::<Vec<_>>();
    let mut skipped = rows
        .iter()
        .map(|row| vec![false; row.len()])
        .collect::<Vec<_>>();
    let mut active_merges = BTreeMap::<usize, (usize, usize)>::new();
    for (row_index, row) in rows.iter().enumerate() {
        let mut column = 0usize;
        for (cell_index, cell) in row.iter().enumerate() {
            match cell.vertical_merge {
                Some(VerticalMerge::Restart) => {
                    for merged_column in column..column + cell.colspan {
                        active_merges.insert(merged_column, (row_index, cell_index));
                    }
                }
                Some(VerticalMerge::Continue) => {
                    if let Some(&(origin_row, origin_cell)) = active_merges.get(&column) {
                        rowspans[origin_row][origin_cell] += 1;
                        skipped[row_index][cell_index] = true;
                    }
                }
                None => {
                    for merged_column in column..column + cell.colspan {
                        active_merges.remove(&merged_column);
                    }
                }
            }
            column += cell.colspan;
        }
    }

    let mut html = String::from("<table>\n");
    for (row_index, row) in rows.iter().enumerate() {
        html.push_str("  <tr>\n");
        for (cell_index, cell) in row.iter().enumerate() {
            if skipped[row_index][cell_index] {
                continue;
            }
            html.push_str("    <td");
            if cell.colspan > 1 {
                html.push_str(&format!(" colspan=\"{}\"", cell.colspan));
            }
            if rowspans[row_index][cell_index] > 1 {
                html.push_str(&format!(" rowspan=\"{}\"", rowspans[row_index][cell_index]));
            }
            html.push('>');
            html.push_str(&xml_escape(&cell.content));
            html.push_str("</td>\n");
        }
        html.push_str("  </tr>\n");
    }
    html.push_str("</table>");
    html
}

fn handle_math_event(
    event: &quick_xml::events::Event<'_>,
    stack: &mut Vec<MathNode>,
    paragraph: &mut String,
    pending_run: &mut Option<PendingRun>,
) -> Result<bool, MarkoffError> {
    use quick_xml::events::Event;

    match event {
        Event::Start(event) => {
            let name = event.local_name().as_ref().to_owned();
            let starts_math = matches!(name.as_str(), "oMath" | "oMathPara");
            if stack.is_empty() && !starts_math {
                return Ok(false);
            }
            stack.push(MathNode::new(name, attribute_value(event, "val")?));
        }
        Event::Empty(event) => {
            let name = event.local_name().as_ref().to_owned();
            let starts_math = matches!(name.as_str(), "oMath" | "oMathPara");
            if stack.is_empty() && !starts_math {
                return Ok(false);
            }
            let node = MathNode::new(name, attribute_value(event, "val")?);
            if let Some(parent) = stack.last_mut() {
                parent.push_child(node);
            } else {
                flush_pending_run(paragraph, pending_run);
                let display = node.is_display() || paragraph.is_empty();
                paragraph.push_str(&node.into_markdown(display));
            }
        }
        Event::End(_) if !stack.is_empty() => {
            let Some(node) = stack.pop() else {
                return Ok(false);
            };
            if let Some(parent) = stack.last_mut() {
                parent.push_child(node);
            } else {
                flush_pending_run(paragraph, pending_run);
                let display = node.is_display() || paragraph.is_empty();
                paragraph.push_str(&node.into_markdown(display));
            }
        }
        Event::Text(event) if !stack.is_empty() => {
            let text = quick_xml::escape::unescape(event.as_ref()).map_err(invalid_data)?;
            if let Some(node) = stack.last_mut() {
                node.push_text(&text);
            }
        }
        Event::GeneralRef(event) if !stack.is_empty() => {
            let text = resolve_general_ref(event)?;
            if let Some(node) = stack.last_mut() {
                node.push_text(&text);
            }
        }
        Event::Eof if !stack.is_empty() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unterminated Office Math expression",
            )
            .into());
        }
        _ if !stack.is_empty() => {}
        _ => return Ok(false),
    }
    Ok(true)
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
    let endnotes = read_endnotes(&mut archive)?;
    let mut endnote_numbering = EndnoteNumbering::read(&mut archive, &document)?;
    let relationships = read_relationships(&mut archive)?;
    let svg_graphics = extract_svg_graphics(&document, &mut archive, &relationships)?;
    let image_dir = output
        .parent()
        .map_or_else(|| PathBuf::from("image"), |parent| parent.join("image"));
    let mut image_counter = 0usize;
    let mut drawing_index = 0usize;

    let mut reader = Reader::from_str(&document);
    reader.config_mut().trim_text(false);
    let mut markdown = Vec::new();
    let mut paragraph = String::new();
    let mut raw_paragraph = String::new();
    let mut math_stack = Vec::new();
    let mut run = String::new();
    let mut run_depth = 0usize;
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
    let mut hyperlink_target = None;
    let mut in_field_result = false;
    let mut in_instruction_text = false;
    let mut run_properties = RunProperties::default();
    let mut paragraph_has_code = false;
    let mut paragraph_has_plain_text = false;
    let mut in_paragraph = false;
    let mut paragraph_depth = 0usize;
    let mut referenced_footnotes = BTreeSet::new();
    let mut referenced_endnotes = BTreeSet::new();
    let mut table_rows = Vec::new();
    let mut table_row = Vec::new();
    let mut table_cell_paragraphs = Vec::new();
    let mut table_cell_colspan = 1usize;
    let mut table_cell_vertical_merge = None;
    let mut in_table = false;
    let mut in_table_cell = false;
    let mut list_counters = BTreeMap::new();
    let mut image_rel_id = None;
    let mut image_alt = String::new();

    loop {
        let event = reader.read_event().map_err(invalid_data)?;
        if handle_math_event(&event, &mut math_stack, &mut paragraph, &mut pending_run)? {
            continue;
        }
        match event {
            Event::Start(event) | Event::Empty(event) => match event.local_name().as_ref() {
                "tbl" => {
                    in_table = true;
                    table_rows.clear();
                }
                "tr" if in_table => table_row.clear(),
                "tc" if in_table => {
                    in_table_cell = true;
                    table_cell_paragraphs.clear();
                    table_cell_colspan = 1;
                    table_cell_vertical_merge = None;
                }
                "gridSpan" if in_table_cell => {
                    table_cell_colspan = attribute_value(&event, "val")?
                        .and_then(|value| value.parse().ok())
                        .filter(|span| *span > 0)
                        .unwrap_or(1);
                }
                "vMerge" if in_table_cell => {
                    table_cell_vertical_merge = Some(
                        if attribute_value(&event, "val")?.as_deref() == Some("restart") {
                            VerticalMerge::Restart
                        } else {
                            VerticalMerge::Continue
                        },
                    );
                }
                "p" => {
                    if in_paragraph {
                        paragraph_depth += 1;
                    } else {
                        in_paragraph = true;
                        paragraph_depth = 1;
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
                        hyperlink_target = None;
                        in_field_result = false;
                        paragraph_has_code = false;
                        paragraph_has_plain_text = false;
                    }
                }
                "r" => {
                    if run_depth == 0 {
                        run.clear();
                        run_properties.start_run();
                    }
                    run_depth += 1;
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
                "hyperlink" if in_paragraph => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    hyperlink_target = if let Some(anchor) = attribute_value(&event, "anchor")? {
                        Some(format!("anchor:{anchor}"))
                    } else {
                        attribute_value(&event, "id")?
                            .and_then(|id| relationships.get(&id))
                            .map(|target| format!("external:{target}"))
                    };
                }
                "rStyle" => {
                    if attribute_value(&event, "val")?
                        .is_some_and(|value| value == "CodeChar" || value == "CodeBlockChar")
                    {
                        run_properties.code = true;
                    }
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
                            hyperlink_target.clone().or_else(|| {
                                in_field_result.then(|| page_reference.clone()).flatten()
                            }),
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
                "endnoteReference" => {
                    if let Some(id) =
                        attribute_value(&event, "id")?.and_then(|value| value.parse::<i64>().ok())
                    {
                        flush_pending_run(&mut paragraph, &mut pending_run);
                        let label = endnote_numbering.label(id);
                        paragraph.push_str(&format!("[^{label}]"));
                        referenced_endnotes.insert(id);
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
                    let mut wrote_image = false;
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
                            wrote_image = true;
                        }
                    }
                    if !wrote_image && let Some(Some(svg)) = svg_graphics.get(drawing_index) {
                        std::fs::create_dir_all(&image_dir)?;
                        let file_name = format!("graphic{}.svg", drawing_index + 1);
                        std::fs::write(image_dir.join(&file_name), svg)?;
                        flush_pending_run(&mut paragraph, &mut pending_run);
                        let alt = image_alt.trim();
                        paragraph.push_str(&format!("![{alt}](image/{file_name})"));
                    }
                    drawing_index += 1;
                    image_alt.clear();
                }
                "r" if run_depth > 1 => run_depth -= 1,
                "r" => {
                    run_depth = 0;
                    if !run.is_empty() {
                        raw_paragraph.push_str(&run);
                        paragraph_has_code |= run_properties.code;
                        paragraph_has_plain_text |= !run_properties.code;
                        queue_docx_run(
                            &mut paragraph,
                            &mut pending_run,
                            &run,
                            run_properties.style(code_block),
                            hyperlink_target.clone().or_else(|| {
                                in_field_result.then(|| page_reference.clone()).flatten()
                            }),
                        );
                    }
                    run_properties = RunProperties::default();
                }
                "hyperlink" if in_paragraph => {
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    hyperlink_target = None;
                }
                "p" if paragraph_depth > 1 => paragraph_depth -= 1,
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
                            let generated_heading_anchor =
                                heading_level.and_then(|_| heading_anchor(&paragraph));
                            let anchors = bookmarks
                                .iter()
                                .filter(|bookmark| {
                                    !(heading_level.is_some()
                                        && (bookmark.starts_with("_markoff_")
                                            || generated_heading_anchor
                                                .as_ref()
                                                .is_some_and(|anchor| anchor == *bookmark)))
                                })
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
                    paragraph_depth = 0;
                    in_paragraph = false;
                }
                "tc" if in_table_cell => {
                    table_row.push(TableCell {
                        content: table_cell_paragraphs.join("<br>"),
                        colspan: table_cell_colspan,
                        vertical_merge: table_cell_vertical_merge,
                    });
                    in_table_cell = false;
                }
                "tr" if in_table => {
                    if !table_row.is_empty() {
                        table_rows.push(std::mem::take(&mut table_row));
                    }
                }
                "tbl" if in_table => {
                    if table_rows
                        .iter()
                        .flatten()
                        .any(|cell: &TableCell| !cell.content.trim().is_empty())
                    {
                        markdown.push(render_table(&table_rows));
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
    for id in referenced_endnotes {
        if let Some(content) = endnotes.get(&id) {
            let label = endnote_numbering.label(id);
            let mut paragraphs = content.split("\n\n");
            if let Some(first) = paragraphs.next() {
                let mut definition = format!("[^{label}]: {}", first.trim());
                for paragraph in paragraphs {
                    definition.push_str("\n\n    ");
                    definition.push_str(paragraph.trim());
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
