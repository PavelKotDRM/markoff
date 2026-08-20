fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn docx_run(text: &str, bold: bool, italic: bool, strikethrough: bool, underline: bool) -> String {
    let mut properties = String::new();
    if bold || italic || strikethrough || underline {
        properties.push_str("<w:rPr>");
        if bold {
            properties.push_str("<w:b/>");
        }
        if italic {
            properties.push_str("<w:i/>");
        }
        if strikethrough {
            properties.push_str("<w:strike/>");
        }
        if underline {
            properties.push_str("<w:u w:val=\"single\"/>");
        }
        properties.push_str("</w:rPr>");
    }
    format!(
        "<w:r>{properties}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
        xml_escape(text)
    )
}

pub(crate) fn markdown_inline_to_docx_runs(value: &str) -> String {
    let mut runs = String::new();
    let mut remaining = value;
    let mut bold = false;
    let mut italic = false;
    let mut strikethrough = false;
    let mut underline = false;
    let mut pending: Option<(bool, bool, bool, bool, String)> = None;

    while !remaining.is_empty() {
        let delimiter = if remaining.starts_with("**") || remaining.starts_with("__") {
            let marker = &remaining[..2];
            let marker_count = remaining
                .as_bytes()
                .chunks_exact(2)
                .take_while(|chunk| *chunk == marker.as_bytes())
                .count();
            if marker_count % 2 == 1 {
                bold = !bold;
            }
            remaining = &remaining[marker_count * 2..];
            continue;
        } else if remaining.starts_with("~~") {
            strikethrough = !strikethrough;
            remaining = &remaining[2..];
            continue;
        } else if remaining.starts_with("<u>") {
            underline = true;
            remaining = &remaining[3..];
            continue;
        } else if remaining.starts_with("</u>") {
            underline = false;
            remaining = &remaining[4..];
            continue;
        } else if remaining.starts_with('*') || remaining.starts_with('_') {
            italic = !italic;
            remaining = &remaining[1..];
            continue;
        } else {
            remaining.char_indices().find_map(|(index, _)| {
                (remaining[index..].starts_with("**")
                    || remaining[index..].starts_with("__")
                    || remaining[index..].starts_with("~~")
                    || remaining[index..].starts_with("<u>")
                    || remaining[index..].starts_with("</u>")
                    || remaining[index..].starts_with('*')
                    || remaining[index..].starts_with('_'))
                .then_some(index)
            })
        };
        let length = delimiter.unwrap_or(remaining.len());
        let (text, rest) = remaining.split_at(length);
        if !text.is_empty() {
            if let Some((
                previous_bold,
                previous_italic,
                previous_strikethrough,
                previous_underline,
                previous_text,
            )) = pending.as_mut()
                && (
                    *previous_bold,
                    *previous_italic,
                    *previous_strikethrough,
                    *previous_underline,
                ) == (bold, italic, strikethrough, underline)
            {
                previous_text.push_str(text);
            } else {
                if let Some((bold, italic, strikethrough, underline, text)) = pending.take() {
                    runs.push_str(&docx_run(&text, bold, italic, strikethrough, underline));
                }
                pending = Some((bold, italic, strikethrough, underline, text.to_string()));
            }
        }
        remaining = rest;
    }
    if let Some((bold, italic, strikethrough, underline, text)) = pending {
        runs.push_str(&docx_run(&text, bold, italic, strikethrough, underline));
    }
    runs
}

pub(crate) fn markdown_list_item(line: &str) -> Option<(u32, usize, &str)> {
    let indentation = line.len() - line.trim_start_matches([' ', '\t']).len();
    let level = indentation / 4;
    let value = line.trim_start_matches([' ', '\t']);
    if let Some(content) = value
        .strip_prefix("- ")
        .or_else(|| value.strip_prefix("* "))
    {
        return Some((1, level, content));
    }

    let (number, content) = value.split_once(". ")?;
    number
        .chars()
        .all(|character| character.is_ascii_digit())
        .then_some((2, level, content))
}

pub(crate) fn pageref_target(instruction: &str) -> Option<String> {
    let mut tokens = instruction.split_whitespace();
    while let Some(token) = tokens.next() {
        if token.eq_ignore_ascii_case("PAGEREF") {
            return tokens.next().map(str::to_string);
        }
    }
    None
}

pub(crate) fn markdown_from_docx_run(
    text: &str,
    bold: bool,
    italic: bool,
    strikethrough: bool,
    underline: bool,
    page_reference: Option<&str>,
) -> String {
    let mut rendered = text.to_string();
    match (bold, italic) {
        (true, true) => rendered = format!("**_*{rendered}*_**"),
        (true, false) => rendered = format!("**{rendered}**"),
        (false, true) => rendered = format!("*{rendered}*"),
        (false, false) => {}
    }
    if strikethrough {
        rendered = format!("~~{rendered}~~");
    }
    if underline {
        rendered = format!("<u>{rendered}</u>");
    }
    if let Some(target) = page_reference {
        rendered = format!("[{rendered}](#{target})");
    }
    rendered
}
