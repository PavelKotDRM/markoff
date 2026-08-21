fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn markdown_escape(value: &str) -> String {
    value.replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('`', "\\`")
        .replace('~', "\\~")
        .replace('<', "\\<")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerticalAlign {
    Baseline,
    Superscript,
    Subscript,
}

fn latex_escape(value: &str) -> String {
    value.replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

fn latex_unescape(value: &str) -> String {
    let mut unescaped = String::new();
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\'
            && let Some(&escaped) = characters.peek()
            && matches!(escaped, '\\' | '{' | '}')
        {
            characters.next();
            unescaped.push(escaped);
        } else {
            unescaped.push(character);
        }
    }
    unescaped
}

fn markdown_unescape(value: &str) -> String {
    let mut unescaped = String::new();
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\'
            && let Some(&escaped) = characters.peek()
            && matches!(escaped, '\\' | '*' | '_' | '`' | '~' | '<' | '[' | ']')
        {
            characters.next();
            unescaped.push(escaped);
        } else {
            unescaped.push(character);
        }
    }
    unescaped
}

fn markdown_delimiter_index(value: &str) -> Option<usize> {
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        let remaining = &value[index..];
        if remaining.starts_with("$$") && remaining[2..].contains("$$") {
            return Some(index);
        }
        if (remaining.starts_with("$^{") || remaining.starts_with("$_{"))
            && remaining[3..].contains("}$")
        {
            return Some(index);
        }
        if remaining.starts_with('$')
            && !remaining.starts_with("$^{")
            && !remaining.starts_with("$_{")
            && remaining[1..].contains('$')
        {
            return Some(index);
        }
        if character == '_'
            && !remaining.starts_with("__")
            && value[..index]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
            && remaining
                .strip_prefix('_')
                .and_then(|text| text.chars().next())
                .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        if remaining.starts_with("**")
            || remaining.starts_with("__")
            || remaining.starts_with("~~")
            || remaining.starts_with("<u>")
            || remaining.starts_with("</u>")
            || remaining.starts_with('`')
            || remaining.starts_with('*')
            || remaining.starts_with('_')
        {
            return Some(index);
        }
    }
    None
}

fn docx_run(
    text: &str,
    bold: bool,
    italic: bool,
    strikethrough: bool,
    underline: bool,
    code: bool,
    vertical_align: VerticalAlign,
) -> String {
    let mut properties = String::new();
    if bold || italic || strikethrough || underline || code || vertical_align != VerticalAlign::Baseline {
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
        if code {
            properties.push_str(
                "<w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/>",
            );
        }
        match vertical_align {
            VerticalAlign::Superscript => {
                properties.push_str("<w:vertAlign w:val=\"superscript\"/>")
            }
            VerticalAlign::Subscript => {
                properties.push_str("<w:vertAlign w:val=\"subscript\"/>")
            }
            VerticalAlign::Baseline => {}
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
    let mut code = false;
    let mut pending: Option<(bool, bool, bool, bool, bool, String)> = None;

    while !remaining.is_empty() {
        if (remaining.starts_with("$^{") || remaining.starts_with("$_{"))
            && let Some(end) = remaining[3..].find("}$")
        {
            if let Some((bold, italic, strikethrough, underline, code, text)) = pending.take() {
                runs.push_str(&docx_run(
                    &text,
                    bold,
                    italic,
                    strikethrough,
                    underline,
                    code,
                    VerticalAlign::Baseline,
                ));
            }
            let vertical_align = if remaining.starts_with("$^{") {
                VerticalAlign::Superscript
            } else {
                VerticalAlign::Subscript
            };
            let inner = latex_unescape(&remaining[3..3 + end]);
            runs.push_str(&docx_run(
                &inner,
                bold,
                italic,
                strikethrough,
                underline,
                code,
                vertical_align,
            ));
            remaining = &remaining[3 + end + 2..];
            continue;
        }
        // LaTeX math regions are treated as raw text: Markdown's own
        // backslash/entity handling would otherwise corrupt commands like
        // `\\` (matrix row break) or literal `&` (matrix column separator).
        if remaining.starts_with("$$")
            && let Some(end) = remaining[2..].find("$$")
        {
            if let Some((bold, italic, strikethrough, underline, code, text)) = pending.take() {
                runs.push_str(&docx_run(
                    &text,
                    bold,
                    italic,
                    strikethrough,
                    underline,
                    code,
                    VerticalAlign::Baseline,
                ));
            }
            runs.push_str(&docx_run(
                &remaining[..2 + end + 2],
                bold,
                italic,
                strikethrough,
                underline,
                code,
                VerticalAlign::Baseline,
            ));
            remaining = &remaining[2 + end + 2..];
            continue;
        }
        if remaining.starts_with('$')
            && let Some(end) = remaining[1..].find('$')
        {
            if let Some((bold, italic, strikethrough, underline, code, text)) = pending.take() {
                runs.push_str(&docx_run(
                    &text,
                    bold,
                    italic,
                    strikethrough,
                    underline,
                    code,
                    VerticalAlign::Baseline,
                ));
            }
            runs.push_str(&docx_run(
                &remaining[..1 + end + 1],
                bold,
                italic,
                strikethrough,
                underline,
                code,
                VerticalAlign::Baseline,
            ));
            remaining = &remaining[1 + end + 1..];
            continue;
        }
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
        } else if remaining.starts_with('`') {
            code = !code;
            remaining = &remaining[1..];
            continue;
        } else if remaining.starts_with('*') || remaining.starts_with('_') {
            italic = !italic;
            remaining = &remaining[1..];
            continue;
        } else {
            markdown_delimiter_index(remaining)
        };
        let length = delimiter.unwrap_or(remaining.len());
        let (text, rest) = remaining.split_at(length);
        let text = markdown_unescape(text);
        if !text.is_empty() {
            if let Some((
                previous_bold,
                previous_italic,
                previous_strikethrough,
                previous_underline,
                previous_code,
                previous_text,
            )) = pending.as_mut()
                && (
                    *previous_bold,
                    *previous_italic,
                    *previous_strikethrough,
                    *previous_underline,
                    *previous_code,
                ) == (bold, italic, strikethrough, underline, code)
            {
                previous_text.push_str(&text);
            } else {
                if let Some((bold, italic, strikethrough, underline, code, text)) = pending.take() {
                    runs.push_str(&docx_run(
                        &text,
                        bold,
                        italic,
                        strikethrough,
                        underline,
                        code,
                        VerticalAlign::Baseline,
                    ));
                }
                pending = Some((
                    bold,
                    italic,
                    strikethrough,
                    underline,
                    code,
                    text,
                ));
            }
        }
        remaining = rest;
    }
    if let Some((bold, italic, strikethrough, underline, code, text)) = pending {
        runs.push_str(&docx_run(
            &text,
            bold,
            italic,
            strikethrough,
            underline,
            code,
            VerticalAlign::Baseline,
        ));
    }
    runs
}

pub(crate) fn markdown_code_block_to_docx_runs(value: &str) -> String {
    value
        .lines()
        .map(|line| docx_run(line, false, false, false, false, true, VerticalAlign::Baseline))
        .collect::<Vec<_>>()
        .join("<w:r><w:br/></w:r>")
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
    code: bool,
    vertical_align: VerticalAlign,
    page_reference: Option<&str>,
) -> String {
    // Emphasis markers must hug non-whitespace content on both sides, or a
    // CommonMark parser won't treat them as opening/closing delimiters and
    // formatting will bleed into neighboring runs. Keep any leading/trailing
    // whitespace outside the markers instead of wrapping it.
    let core = text.trim_matches(char::is_whitespace);
    if core.is_empty() {
        return text.to_string();
    }
    let leading = &text[..text.len() - text.trim_start_matches(char::is_whitespace).len()];
    let trailing = &text[text.trim_end_matches(char::is_whitespace).len()..];
    let mut rendered = match vertical_align {
        VerticalAlign::Superscript => format!("$^{{{}}}$", latex_escape(core)),
        VerticalAlign::Subscript => format!("$_{{{}}}$", latex_escape(core)),
        VerticalAlign::Baseline => {
            let mut rendered = markdown_escape(core);
            match (bold, italic) {
                (true, true) => rendered = format!("***{rendered}***"),
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
            if code {
                rendered = format!("`{rendered}`");
            }
            rendered
        }
    };
    if let Some(target) = page_reference {
        rendered = format!("[{rendered}](#{target})");
    }
    format!("{leading}{rendered}{trailing}")
}
