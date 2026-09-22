use crate::xml_utils::{MarkdownEscapeContext, markdown_escape, xml_attribute_escape, xml_escape};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerticalAlign {
    Baseline,
    Superscript,
    Subscript,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocxRunStyle {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) strikethrough: bool,
    pub(crate) underline: bool,
    pub(crate) code: bool,
    pub(crate) vertical_align: VerticalAlign,
}

#[derive(Clone)]
pub(crate) enum DocxHyperlink {
    Relationship(String),
    Anchor(String),
}

fn latex_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
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

fn markdown_delimiter_index(value: &str, allow_links: bool) -> Option<usize> {
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
        if allow_links && character == '[' && markdown_link_at_start(remaining).is_some() {
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

fn markdown_link_at_start(value: &str) -> Option<(&str, String, usize)> {
    if !value.starts_with('[') {
        return None;
    }

    let mut depth = 0usize;
    let mut escaped = false;
    let label_end = value[1..].char_indices().find_map(|(index, character)| {
        let index = index + 1;
        if escaped {
            escaped = false;
            return None;
        }
        if character == '\\' {
            escaped = true;
            return None;
        }
        match character {
            '[' => depth += 1,
            ']' if depth == 0 => return Some(index),
            ']' => depth -= 1,
            _ => {}
        }
        None
    })?;

    let after_label = &value[label_end + 1..];
    let after_open = after_label.strip_prefix('(')?;
    let mut depth = 0usize;
    let mut escaped = false;
    let closing_parenthesis = after_open.char_indices().find_map(|(index, character)| {
        if escaped {
            escaped = false;
            return None;
        }
        if character == '\\' {
            escaped = true;
            return None;
        }
        match character {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(index),
            ')' => depth -= 1,
            _ => {}
        }
        None
    })?;
    let destination = markdown_link_destination(&after_open[..closing_parenthesis])?;
    let consumed = label_end + closing_parenthesis + 3;
    Some((&value[1..label_end], destination, consumed))
}

fn markdown_link_destination(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let destination = if let Some(value) = value.strip_prefix('<') {
        let end = value.find('>')?;
        let remainder = value[end + 1..].trim();
        if !remainder.is_empty() && !remainder.starts_with('"') && !remainder.starts_with('\'') {
            return None;
        }
        &value[..end]
    } else {
        let mut depth = 0usize;
        let mut escaped = false;
        let end = value
            .char_indices()
            .find_map(|(index, character)| {
                if escaped {
                    escaped = false;
                    return None;
                }
                if character == '\\' {
                    escaped = true;
                    return None;
                }
                match character {
                    '(' => depth += 1,
                    ')' if depth > 0 => depth -= 1,
                    character if character.is_whitespace() && depth == 0 => return Some(index),
                    _ => {}
                }
                None
            })
            .unwrap_or(value.len());
        &value[..end]
    };
    let mut unescaped = String::new();
    let mut escaped = false;
    for character in destination.chars() {
        if escaped {
            unescaped.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            unescaped.push(character);
        }
    }
    if escaped {
        unescaped.push('\\');
    }
    (!unescaped.is_empty()).then_some(unescaped)
}

#[derive(Clone, Copy, Default)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    underline: bool,
    code: bool,
}

fn docx_run(
    text: &str,
    style: InlineStyle,
    vertical_align: VerticalAlign,
    hyperlink: Option<&DocxHyperlink>,
) -> String {
    let mut properties = String::new();
    if style.bold
        || style.italic
        || style.strikethrough
        || style.underline
        || style.code
        || vertical_align != VerticalAlign::Baseline
        || hyperlink.is_some()
    {
        properties.push_str("<w:rPr>");
        if style.bold {
            properties.push_str("<w:b/>");
        }
        if style.italic {
            properties.push_str("<w:i/>");
        }
        if style.strikethrough {
            properties.push_str("<w:strike/>");
        }
        if style.underline {
            properties.push_str("<w:u w:val=\"single\"/>");
        }
        if style.code {
            properties.push_str(
                "<w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/>",
            );
        }
        match vertical_align {
            VerticalAlign::Superscript => {
                properties.push_str("<w:vertAlign w:val=\"superscript\"/>")
            }
            VerticalAlign::Subscript => properties.push_str("<w:vertAlign w:val=\"subscript\"/>"),
            VerticalAlign::Baseline => {}
        }
        if hyperlink.is_some() {
            properties.push_str("<w:rStyle w:val=\"Hyperlink\"/>");
        }
        properties.push_str("</w:rPr>");
    }
    let run = format!(
        "<w:r>{properties}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
        xml_escape(text)
    );
    match hyperlink {
        Some(DocxHyperlink::Relationship(id)) => format!(
            "<w:hyperlink r:id=\"{}\">{run}</w:hyperlink>",
            xml_attribute_escape(id)
        ),
        Some(DocxHyperlink::Anchor(anchor)) => format!(
            "<w:hyperlink w:anchor=\"{}\">{run}</w:hyperlink>",
            xml_attribute_escape(anchor)
        ),
        None => run,
    }
}

pub(crate) fn markdown_inline_to_docx_runs_with_links<F>(value: &str, resolver: &mut F) -> String
where
    F: FnMut(&str) -> Option<DocxHyperlink>,
{
    markdown_inline_to_docx_runs_with_context(value, InlineStyle::default(), None, resolver)
}

fn markdown_inline_to_docx_runs_with_context<F>(
    value: &str,
    mut style: InlineStyle,
    hyperlink: Option<&DocxHyperlink>,
    resolver: &mut F,
) -> String
where
    F: FnMut(&str) -> Option<DocxHyperlink>,
{
    let mut runs = String::new();
    let mut remaining = value;
    let mut pending: Option<(bool, bool, bool, bool, bool, String)> = None;

    while !remaining.is_empty() {
        if !style.code
            && let Some((label, destination, consumed)) = markdown_link_at_start(remaining)
            && let Some(link) = resolver(&destination)
        {
            flush_pending_inline_run(&mut runs, &mut pending, hyperlink);
            runs.push_str(&markdown_inline_to_docx_runs_with_context(
                label,
                style,
                Some(&link),
                resolver,
            ));
            remaining = &remaining[consumed..];
            continue;
        }
        if (remaining.starts_with("$^{") || remaining.starts_with("$_{"))
            && let Some(end) = remaining[3..].find("}$")
        {
            flush_pending_inline_run(&mut runs, &mut pending, hyperlink);
            let vertical_align = if remaining.starts_with("$^{") {
                VerticalAlign::Superscript
            } else {
                VerticalAlign::Subscript
            };
            let inner = latex_unescape(&remaining[3..3 + end]);
            runs.push_str(&docx_run(&inner, style, vertical_align, hyperlink));
            remaining = &remaining[3 + end + 2..];
            continue;
        }
        // LaTeX math regions are treated as raw text: Markdown's own
        // backslash/entity handling would otherwise corrupt commands like
        // `\\` (matrix row break) or literal `&` (matrix column separator).
        if remaining.starts_with("$$")
            && let Some(end) = remaining[2..].find("$$")
        {
            flush_pending_inline_run(&mut runs, &mut pending, hyperlink);
            runs.push_str(&docx_run(
                &remaining[..2 + end + 2],
                style,
                VerticalAlign::Baseline,
                hyperlink,
            ));
            remaining = &remaining[2 + end + 2..];
            continue;
        }
        if remaining.starts_with('$')
            && let Some(end) = remaining[1..].find('$')
        {
            flush_pending_inline_run(&mut runs, &mut pending, hyperlink);
            runs.push_str(&docx_run(
                &remaining[..1 + end + 1],
                style,
                VerticalAlign::Baseline,
                hyperlink,
            ));
            remaining = &remaining[1 + end + 1..];
            continue;
        }
        let delimiter = if remaining.starts_with("**") || remaining.starts_with("__") {
            let marker = &remaining[..2];
            let marker_count = remaining
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .take_while(|chunk| *chunk == marker.as_bytes())
                .count();
            if marker_count % 2 == 1 {
                style.bold = !style.bold;
            }
            remaining = &remaining[marker_count * 2..];
            continue;
        } else if remaining.starts_with("~~") {
            style.strikethrough = !style.strikethrough;
            remaining = &remaining[2..];
            continue;
        } else if remaining.starts_with("<u>") {
            style.underline = true;
            remaining = &remaining[3..];
            continue;
        } else if remaining.starts_with("</u>") {
            style.underline = false;
            remaining = &remaining[4..];
            continue;
        } else if remaining.starts_with('`') {
            style.code = !style.code;
            remaining = &remaining[1..];
            continue;
        } else if remaining.starts_with('*') || remaining.starts_with('_') {
            style.italic = !style.italic;
            remaining = &remaining[1..];
            continue;
        } else {
            markdown_delimiter_index(remaining, !style.code)
        };
        let delimiter =
            if !style.code && delimiter == Some(0) && markdown_link_at_start(remaining).is_some() {
                Some(1)
            } else {
                delimiter
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
                ) == (
                    style.bold,
                    style.italic,
                    style.strikethrough,
                    style.underline,
                    style.code,
                )
            {
                previous_text.push_str(&text);
            } else {
                flush_pending_inline_run(&mut runs, &mut pending, hyperlink);
                pending = Some((
                    style.bold,
                    style.italic,
                    style.strikethrough,
                    style.underline,
                    style.code,
                    text,
                ));
            }
        }
        remaining = rest;
    }
    flush_pending_inline_run(&mut runs, &mut pending, hyperlink);
    runs
}

fn flush_pending_inline_run(
    runs: &mut String,
    pending: &mut Option<(bool, bool, bool, bool, bool, String)>,
    hyperlink: Option<&DocxHyperlink>,
) {
    if let Some((bold, italic, strikethrough, underline, code, text)) = pending.take() {
        runs.push_str(&docx_run(
            &text,
            InlineStyle {
                bold,
                italic,
                strikethrough,
                underline,
                code,
            },
            VerticalAlign::Baseline,
            hyperlink,
        ));
    }
}

pub(crate) fn markdown_code_block_to_docx_runs(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            docx_run(
                line,
                InlineStyle {
                    code: true,
                    ..InlineStyle::default()
                },
                VerticalAlign::Baseline,
                None,
            )
        })
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
    style: DocxRunStyle,
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
    let mut rendered = match style.vertical_align {
        VerticalAlign::Superscript => format!("$^{{{}}}$", latex_escape(core)),
        VerticalAlign::Subscript => format!("$_{{{}}}$", latex_escape(core)),
        VerticalAlign::Baseline => {
            let mut rendered = markdown_escape(core, MarkdownEscapeContext::Docx);
            match (style.bold, style.italic) {
                (true, true) => rendered = format!("***{rendered}***"),
                (true, false) => rendered = format!("**{rendered}**"),
                (false, true) => rendered = format!("*{rendered}*"),
                (false, false) => {}
            }
            if style.strikethrough {
                rendered = format!("~~{rendered}~~");
            }
            if style.underline {
                rendered = format!("<u>{rendered}</u>");
            }
            if style.code {
                rendered = format!("`{rendered}`");
            }
            rendered
        }
    };
    if let Some(target) = page_reference {
        let destination = if let Some(target) = target.strip_prefix("external:") {
            target.to_string()
        } else if let Some(target) = target.strip_prefix("anchor:") {
            format!("#{target}")
        } else {
            format!("#{target}")
        };
        rendered = format!("[{rendered}]({destination})");
    }
    format!("{leading}{rendered}{trailing}")
}
