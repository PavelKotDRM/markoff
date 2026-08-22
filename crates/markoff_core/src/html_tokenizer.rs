const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

pub(super) enum Token {
    Start(String, Vec<(String, String)>, bool),
    End(String),
    Text(String),
}

pub(super) fn tokenize(html: &str) -> Vec<Token> {
    let len = html.len();
    let mut index = 0;
    let mut tokens = Vec::new();
    let mut text_buffer = String::new();

    while index < len {
        if html.as_bytes()[index] == b'<' {
            if !text_buffer.is_empty() {
                tokens.push(Token::Text(std::mem::take(&mut text_buffer)));
            }
            let rest = &html[index..];
            if rest.starts_with("<!--") {
                index += rest.find("-->").map_or(rest.len(), |end| end + 3);
                continue;
            }
            if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case("<!doctype") {
                index += rest.find('>').map_or(rest.len(), |end| end + 1);
                continue;
            }
            let (tag_content, consumed) = scan_tag(rest);
            index += consumed;

            if let Some(name) = tag_content.strip_prefix('/') {
                tokens.push(Token::End(name.trim().to_ascii_lowercase()));
                continue;
            }

            let trimmed_end = tag_content.trim_end();
            let self_closing_slash = trimmed_end.ends_with('/');
            let body = trimmed_end.trim_end_matches('/');
            let (name, attributes) = parse_tag(body);
            let lower_name = name.to_ascii_lowercase();

            if lower_name == "script" || lower_name == "style" {
                let close_pattern = format!("</{lower_name}");
                let remaining = &html[index..];
                if let Some(position) = remaining.to_ascii_lowercase().find(&close_pattern) {
                    index += position;
                    let after = &html[index..];
                    index += after.find('>').map_or(after.len(), |end| end + 1);
                } else {
                    index = len;
                }
                continue;
            }

            let is_void = self_closing_slash || VOID_ELEMENTS.contains(&lower_name.as_str());
            tokens.push(Token::Start(lower_name, attributes, is_void));
        } else {
            let next = html[index..]
                .find('<')
                .map_or(len, |position| index + position);
            text_buffer.push_str(&decode_entities(&html[index..next]));
            index = next;
        }
    }
    if !text_buffer.is_empty() {
        tokens.push(Token::Text(text_buffer));
    }
    tokens
}

fn scan_tag(rest: &str) -> (String, usize) {
    let bytes = rest.as_bytes();
    let mut index = 1;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' if !in_double_quote => in_single_quote = !in_single_quote,
            b'"' if !in_single_quote => in_double_quote = !in_double_quote,
            b'>' if !in_single_quote && !in_double_quote => break,
            _ => {}
        }
        index += 1;
    }
    (rest[1..index].to_string(), index + 1)
}

fn parse_tag(content: &str) -> (String, Vec<(String, String)>) {
    let content = content.trim();
    let name_end = content
        .find(|character: char| character.is_whitespace())
        .unwrap_or(content.len());
    let name = content[..name_end].to_string();
    let rest = content[name_end..].trim();

    let bytes = rest.as_bytes();
    let mut index = 0;
    let mut attributes = Vec::new();
    while index < bytes.len() {
        while index < bytes.len() && (bytes[index] as char).is_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let key_start = index;
        while index < bytes.len() && bytes[index] != b'=' && !(bytes[index] as char).is_whitespace()
        {
            index += 1;
        }
        let key = rest[key_start..index].to_ascii_lowercase();
        while index < bytes.len() && (bytes[index] as char).is_whitespace() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'=' {
            index += 1;
            while index < bytes.len() && (bytes[index] as char).is_whitespace() {
                index += 1;
            }
            if index < bytes.len() && (bytes[index] == b'"' || bytes[index] == b'\'') {
                let quote = bytes[index];
                index += 1;
                let value_start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                let value = decode_entities(&rest[value_start..index]);
                if index < bytes.len() {
                    index += 1;
                }
                if !key.is_empty() {
                    attributes.push((key, value));
                }
            } else {
                let value_start = index;
                while index < bytes.len() && !(bytes[index] as char).is_whitespace() {
                    index += 1;
                }
                let value = decode_entities(&rest[value_start..index]);
                if !key.is_empty() {
                    attributes.push((key, value));
                }
            }
        } else if !key.is_empty() {
            attributes.push((key, String::new()));
        }
    }
    (name, attributes)
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        result.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(end) = tail[..tail.len().min(12)].find(';') {
            let entity = &tail[1..end];
            if let Some(replacement) = decode_one_entity(entity) {
                result.push_str(&replacement);
                rest = &tail[end + 1..];
                continue;
            }
        }
        result.push('&');
        rest = &tail[1..];
    }
    result.push_str(rest);
    result
}

fn decode_one_entity(entity: &str) -> Option<String> {
    if let Some(hex) = entity
        .strip_prefix('x')
        .or_else(|| entity.strip_prefix('X'))
    {
        return u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .map(String::from);
    }
    if let Some(decimal) = entity.strip_prefix('#') {
        if let Some(hex) = decimal
            .strip_prefix('x')
            .or_else(|| decimal.strip_prefix('X'))
        {
            return u32::from_str_radix(hex, 16)
                .ok()
                .and_then(char::from_u32)
                .map(String::from);
        }
        return decimal
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(String::from);
    }
    Some(
        match entity {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "nbsp" => "\u{00A0}",
            "mdash" => "\u{2014}",
            "ndash" => "\u{2013}",
            "hellip" => "\u{2026}",
            "lsquo" => "\u{2018}",
            "rsquo" => "\u{2019}",
            "ldquo" => "\u{201C}",
            "rdquo" => "\u{201D}",
            "copy" => "\u{00A9}",
            "reg" => "\u{00AE}",
            "trade" => "\u{2122}",
            "laquo" => "\u{00AB}",
            "raquo" => "\u{00BB}",
            _ => return None,
        }
        .to_string(),
    )
}

pub(super) fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_was_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            if !last_was_space {
                result.push(' ');
            }
            last_was_space = true;
        } else {
            result.push(character);
            last_was_space = false;
        }
    }
    result
}
