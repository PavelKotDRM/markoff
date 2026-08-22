use crate::MarkoffError;
use crate::docx_inline::{DocxRunStyle, VerticalAlign, markdown_from_docx_run, pageref_target};
use crate::error::invalid_data;
use crate::xml_utils::{attribute_value, parse_relationships};
use quick_xml::XmlVersion;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

struct PendingRun {
    text: String,
    style: DocxRunStyle,
    target: Option<String>,
}

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
    let mut bold = false;
    let mut italic = false;
    let mut strikethrough = false;
    let mut underline = false;
    let mut code = false;
    let mut vert_align = VerticalAlign::Baseline;
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
                b"r" => {
                    run.clear();
                    code = false;
                    vert_align = VerticalAlign::Baseline;
                }
                b"b" => bold = word_property_enabled(&event, reader.decoder()),
                b"i" => italic = word_property_enabled(&event, reader.decoder()),
                b"strike" => strikethrough = word_property_enabled(&event, reader.decoder()),
                b"u" => underline = word_property_enabled(&event, reader.decoder()),
                b"vertAlign" => {
                    vert_align = attribute_value(&event, b"val", reader.decoder())?
                        .map(|value| match value.as_str() {
                            "superscript" => VerticalAlign::Superscript,
                            "subscript" => VerticalAlign::Subscript,
                            _ => VerticalAlign::Baseline,
                        })
                        .unwrap_or(VerticalAlign::Baseline);
                }
                b"drawing" => {
                    image_rel_id = None;
                    image_alt.clear();
                }
                b"docPr" => {
                    if let Some(description) = attribute_value(&event, b"descr", reader.decoder())?
                        .filter(|value| !value.is_empty())
                        .or(attribute_value(&event, b"name", reader.decoder())?)
                    {
                        image_alt = description;
                    }
                }
                b"blip" => {
                    image_rel_id = attribute_value(&event, b"embed", reader.decoder())?;
                }
                b"rFonts" => {
                    code = event.attributes().flatten().any(|attribute| {
                        attribute.key.local_name().as_ref() == b"ascii"
                            && attribute
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
                                .is_ok_and(|value| {
                                    value.eq_ignore_ascii_case("Consolas")
                                        || value.eq_ignore_ascii_case("Courier New")
                                })
                    });
                }
                b"tab" if in_paragraph => {
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
                b"br" if in_paragraph => {
                    if !run.is_empty() {
                        raw_paragraph.push_str(&run);
                        paragraph_has_code |= code;
                        paragraph_has_plain_text |= !code;
                        queue_docx_run(
                            &mut paragraph,
                            &mut pending_run,
                            &run,
                            DocxRunStyle {
                                bold,
                                italic,
                                strikethrough,
                                underline,
                                code: code && !code_block,
                                vertical_align: vert_align,
                            },
                            in_field_result.then(|| page_reference.clone()).flatten(),
                        );
                        run.clear();
                    }
                    flush_pending_run(&mut paragraph, &mut pending_run);
                    paragraph.push_str("  \n");
                    raw_paragraph.push('\n');
                }
                b"instrText" => in_instruction_text = true,
                b"bookmarkStart" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"name" {
                            bookmarks.push(
                                attribute
                                    .decoded_and_normalized_value(
                                        XmlVersion::Implicit1_0,
                                        reader.decoder(),
                                    )
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
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
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
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
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
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
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
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
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
                b"bottom" if in_paragraph => {
                    horizontal_rule = event.attributes().flatten().any(|attribute| {
                        attribute.key.local_name().as_ref() == b"val"
                            && attribute
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
                                .is_ok_and(|value| {
                                    !value.eq_ignore_ascii_case("nil")
                                        && !value.eq_ignore_ascii_case("none")
                                })
                    });
                }
                b"numId" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"val" {
                            list_numbering_id = Some(
                                attribute
                                    .decoded_and_normalized_value(
                                        XmlVersion::Implicit1_0,
                                        reader.decoder(),
                                    )
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
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
                                .map_err(invalid_data)?
                                .parse()
                                .unwrap_or(0);
                        }
                    }
                }
                _ => {}
            },
            Event::Text(event) if in_paragraph => {
                let decoded = event.decode().map_err(invalid_data)?;
                let text = quick_xml::escape::unescape(&decoded).map_err(invalid_data)?;
                if in_instruction_text {
                    field_instruction.push_str(&text);
                } else {
                    run.push_str(&text);
                }
            }
            Event::GeneralRef(event) if in_paragraph => {
                let text = resolve_general_ref(&event, reader.decoder())?;
                if in_instruction_text {
                    field_instruction.push_str(&text);
                } else {
                    run.push_str(&text);
                }
            }
            Event::End(event) => match event.local_name().as_ref() {
                b"instrText" => in_instruction_text = false,
                b"fldSimple" => in_field_result = false,
                b"drawing" if in_paragraph => {
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
                b"r" => {
                    if !run.is_empty() {
                        raw_paragraph.push_str(&run);
                        paragraph_has_code |= code;
                        paragraph_has_plain_text |= !code;
                        queue_docx_run(
                            &mut paragraph,
                            &mut pending_run,
                            &run,
                            DocxRunStyle {
                                bold,
                                italic,
                                strikethrough,
                                underline,
                                code: code && !code_block,
                                vertical_align: vert_align,
                            },
                            in_field_result.then(|| page_reference.clone()).flatten(),
                        );
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
                            .then(|| markdown_textual_list_item(&paragraph))
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
    convert_textual_footnotes(&mut markdown);
    convert_formula_section(&mut markdown);
    std::fs::write(output, markdown.join("\n\n") + "\n")?;
    Ok(())
}

/// Some documents simulate footnotes with plain text (e.g. `text.[1]` and a
/// separate `[1] Note text.` paragraph) instead of real Word footnotes. When
/// a bracketed marker is both referenced and defined this way, rewrite it as
/// Markdown footnote syntax (`[^1]` / `[^1]: Note text.`).
fn convert_textual_footnotes(markdown: &mut Vec<String>) {
    let definitions = markdown
        .iter()
        .filter_map(|paragraph| textual_footnote_definition(paragraph))
        .collect::<Vec<_>>();

    let mut footnotes = Vec::new();
    for (label, content) in definitions {
        let marker = format!("\\[{label}\\]");
        let is_referenced = markdown.iter().any(|paragraph| {
            !textual_footnote_definition(paragraph).is_some_and(|(other, _)| other == label)
                && paragraph.contains(&marker)
        });
        if !is_referenced {
            continue;
        }
        for paragraph in markdown.iter_mut() {
            if textual_footnote_definition(paragraph).is_none() {
                *paragraph = paragraph.replace(&marker, &format!("[^{label}]"));
            }
        }
        footnotes.push((label, content));
    }

    markdown.retain(|paragraph| textual_footnote_definition(paragraph).is_none());
    for (label, content) in footnotes {
        markdown.push(format!("[^{label}]: {content}"));
    }
}

/// Parses a paragraph of the form `\[label\] content` where `label` is a
/// plain footnote-style marker (digits or lowercase Roman numerals), as
/// produced by escaping a literal `[label] content` paragraph.
fn textual_footnote_definition(paragraph: &str) -> Option<(String, String)> {
    let remainder = paragraph.trim_start().strip_prefix("\\[")?;
    let (label, rest) = remainder.split_once("\\]")?;
    let content = rest.strip_prefix(' ')?;
    let is_valid_label = !label.is_empty()
        && (label.bytes().all(|byte| byte.is_ascii_digit())
            || label.chars().all(|character| "ivxlcdm".contains(character)));
    is_valid_label.then(|| (label.to_string(), content.to_string()))
}

/// Rewrites plain-text formulas, fractions, and matrices under a "Formulas"
/// heading (e.g. `# 10. Формулы`) as LaTeX math, since these documents type
/// math notation as plain Unicode text rather than an OOXML math object.
fn convert_formula_section(markdown: &mut [String]) {
    let mut in_formula_section = false;
    for paragraph in markdown.iter_mut() {
        if let Some(heading) = heading_text(paragraph) {
            let heading = heading.to_lowercase();
            in_formula_section = heading.contains("формул") || heading.contains("formula");
            continue;
        }
        if in_formula_section {
            *paragraph = convert_formula_paragraph(paragraph);
        }
    }
}

fn heading_text(paragraph: &str) -> Option<&str> {
    let hashes = paragraph
        .chars()
        .take_while(|character| *character == '#')
        .count();
    ((1..=6).contains(&hashes) && paragraph.as_bytes().get(hashes) == Some(&b' '))
        .then(|| paragraph[hashes + 1..].trim())
}

fn convert_formula_paragraph(paragraph: &str) -> String {
    if let Some((label, rest)) = paragraph.split_once("  \n")
        && label.trim_end().ends_with(':')
    {
        if rest.trim_start().starts_with('$') {
            // Already converted on a previous pass; Markdown's own escaping
            // of the literal backslash doubles it after a DOCX round trip.
            return format!("{label}  \n{}", unescape_math_backslashes(rest));
        }
        let body_lines = rest.split("  \n").collect::<Vec<_>>();
        if body_lines.len() > 1
            && let Some(rows) = body_lines
                .iter()
                .map(|line| matrix_row_to_latex(line))
                .collect::<Option<Vec<_>>>()
        {
            return format!(
                "{label}  \n$$\\begin{{matrix}} {} \\end{{matrix}}$$",
                rows.join(" \\\\ ")
            );
        }
        let latex = latex_math_from_plain_text(&body_lines.join(" "));
        return format!("{label}  \n$${latex}$$");
    }
    if let Some((label, formula)) = paragraph.split_once(": ") {
        if formula.trim_start().starts_with('$') {
            return format!("{label}: {}", unescape_math_backslashes(formula));
        }
        if looks_like_formula(formula) {
            return format!("{label}: ${}$", latex_math_from_plain_text(formula));
        }
    }
    paragraph.to_string()
}

fn looks_like_formula(text: &str) -> bool {
    text.chars().any(|character| character.is_ascii_digit())
        && text
            .chars()
            .all(|character| !character.is_alphabetic() || character.is_ascii())
}

/// Parses an escaped bracketed row like `\[ 1  2 \]` into a LaTeX matrix row
/// (`1 & 2`); returns `None` when the line is not a simple numeric row.
fn matrix_row_to_latex(line: &str) -> Option<String> {
    let inner = line.trim().strip_prefix("\\[")?.strip_suffix("\\]")?.trim();
    (!inner.is_empty()).then(|| inner.split_whitespace().collect::<Vec<_>>().join(" & "))
}

/// Converts common plain-text math notation into LaTeX, without guessing at
/// ambiguous structure (e.g. whether a trailing digit is meant as a power).
fn latex_math_from_plain_text(text: &str) -> String {
    let text = strip_embedded_math_delimiters(text);
    let text = text.replace("\\[", "[").replace("\\]", "]");
    let text = sqrt_to_latex(&text);
    // Whole-expression fraction "(a + b) / (c + d)" is unambiguous enough to
    // convert to \frac; anything less regular is left as translated symbols.
    let text = if let Some((numerator, denominator)) = fraction_parts(&text) {
        format!("\\frac{{{numerator}}}{{{denominator}}}")
    } else {
        text
    };
    [
        ("±", "\\pm"),
        ("≤", "\\le"),
        ("≥", "\\ge"),
        ("≠", "\\neq"),
        ("≈", "\\approx"),
        ("∞", "\\infty"),
        ("∑", "\\sum"),
        ("∏", "\\prod"),
        ("∫", "\\int"),
        ("×", "\\times"),
        ("÷", "\\div"),
        ("⇒", "\\Rightarrow"),
        ("⇔", "\\Leftrightarrow"),
        ("→", "\\rightarrow"),
        ("←", "\\leftarrow"),
        ("↔", "\\leftrightarrow"),
    ]
    .into_iter()
    .fold(text, |text, (symbol, latex)| text.replace(symbol, latex))
}

fn sqrt_to_latex(text: &str) -> String {
    let mut result = String::new();
    let mut remaining = text;
    while let Some(index) = remaining.find('√') {
        result.push_str(&remaining[..index]);
        let after = &remaining[index + '√'.len_utf8()..];
        if let Some(inner_end) = matching_paren_end(after) {
            result.push_str("\\sqrt{");
            result.push_str(&after[1..inner_end]);
            result.push('}');
            remaining = &after[inner_end + 1..];
        } else {
            result.push_str("\\sqrt");
            remaining = after;
        }
    }
    result.push_str(remaining);
    result
}

/// Given text starting with `(`, returns the index of its matching `)`.
fn matching_paren_end(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

fn fraction_parts(text: &str) -> Option<(String, String)> {
    let text = text.trim();
    if !text.starts_with('(') {
        return None;
    }
    let numerator_end = matching_paren_end(text)?;
    let numerator = &text[1..numerator_end];
    let rest = text[numerator_end + 1..]
        .trim_start()
        .strip_prefix('/')?
        .trim_start();
    let denominator = rest.strip_prefix('(')?.strip_suffix(')')?;
    (!denominator.contains('(')).then(|| (numerator.to_string(), denominator.to_string()))
}

/// Merges runs already rendered as `$^{...}$`/`$_{...}$` LaTeX spans into a
/// single surrounding math expression, dropping their own `$` delimiters.
fn strip_embedded_math_delimiters(text: &str) -> String {
    let mut result = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("$^{").or_else(|| remaining.find("$_{")) {
        result.push_str(&remaining[..start]);
        let after_dollar = &remaining[start + 1..];
        if let Some(end) = after_dollar.find("}$") {
            result.push_str(&after_dollar[..end + 1]);
            remaining = &after_dollar[end + 2..];
        } else {
            result.push('$');
            remaining = after_dollar;
        }
    }
    result.push_str(remaining);
    result
}

/// Reverses Markdown's own backslash escaping for LaTeX text produced on a
/// previous conversion pass: a raw DOCX run text keeps a single `\`, but
/// reading it back through `markdown_escape` doubles it to `\\`.
fn unescape_math_backslashes(text: &str) -> String {
    text.replace("\\\\", "\\")
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
    // Plain flat "N." text already reads as a Markdown list unmodified; only
    // extract the number when it is wrapped in formatting (which would
    // otherwise hide it from Markdown's list syntax) or nested/parenthesized.
    (!marker.is_empty() || parenthesized || !parents.is_empty()).then(|| {
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

fn read_relationships(
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

/// Resolves a `word/_rels/document.xml.rels` relationship target (relative to
/// the `word/` package part) and reads the referenced media bytes.
fn read_media_part(archive: &mut zip::ZipArchive<std::fs::File>, target: &str) -> Option<Vec<u8>> {
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

/// quick-xml reports character/general entity references (e.g. `&amp;`) as a
/// separate `Event::GeneralRef` rather than folding them into `Event::Text`;
/// resolve the reference back into its literal character(s).
fn resolve_general_ref(
    event: &quick_xml::events::BytesRef<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> Result<String, MarkoffError> {
    let name = decoder.decode(event).map_err(invalid_data)?;
    let escaped = format!("&{name};");
    Ok(quick_xml::escape::unescape(&escaped)
        .map_err(invalid_data)?
        .into_owned())
}

fn word_property_enabled(
    event: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> bool {
    event
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.local_name().as_ref() == b"val")
        .and_then(|attribute| {
            attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
                .ok()
        })
        .is_none_or(|value| !matches!(value.as_ref(), "0" | "false" | "off" | "none" | "nil"))
}

fn markdown_table_from_docx_rows(rows: &[Vec<String>]) -> String {
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if column_count == 0 {
        return String::new();
    }

    let format_row = |row: &[String]| {
        let cells = (0..column_count)
            .map(|index| {
                // Cell text is already Markdown-escaped (backslashes included)
                // by `markdown_from_docx_run`; only the table-specific pipe
                // delimiter and embedded newlines need handling here.
                row.get(index)
                    .map_or("", String::as_str)
                    .replace("  \n", "<br>")
                    .replace('\n', "<br>")
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

fn flush_pending_run(paragraph: &mut String, pending_run: &mut Option<PendingRun>) {
    if let Some(pending) = pending_run.take() {
        paragraph.push_str(&markdown_from_docx_run(
            &pending.text,
            pending.style,
            pending.target.as_deref(),
        ));
    }
}

fn queue_docx_run(
    paragraph: &mut String,
    pending_run: &mut Option<PendingRun>,
    text: &str,
    style: DocxRunStyle,
    target: Option<String>,
) {
    if let Some(previous) = pending_run.as_mut()
        && previous.style == style
        && previous.target.as_deref() == target.as_deref()
    {
        previous.text.push_str(text);
    } else {
        flush_pending_run(paragraph, pending_run);
        *pending_run = Some(PendingRun {
            text: text.to_string(),
            style,
            target,
        });
    }
}
