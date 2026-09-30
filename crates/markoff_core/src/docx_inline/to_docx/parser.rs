pub(super) fn latex_unescape(value: &str) -> String {
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

pub(super) fn markdown_unescape(value: &str) -> String {
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

pub(super) fn markdown_delimiter_index(value: &str, allow_links: bool) -> Option<usize> {
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

pub(super) fn markdown_link_at_start(value: &str) -> Option<(&str, String, usize)> {
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

pub(super) fn markdown_link_destination(value: &str) -> Option<String> {
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
