pub(crate) fn markdown_list_item(line: &str) -> Option<(u32, usize, &str)> {
    let indentation = line.len() - line.trim_start_matches([' ', '\t']).len();
    let level = indentation / 4;
    let value = line.trim_start_matches([' ', '\t']);
    if let Some(marker) = value.as_bytes().first()
        && matches!(marker, b'-' | b'*' | b'+')
    {
        let content = &value[1..];
        if content.is_empty() || content.starts_with(' ') || content.starts_with('\t') {
            return Some((1, level, content.trim_start_matches([' ', '\t'])));
        }
    }

    let digit_count = value.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=9).contains(&digit_count) {
        return None;
    }
    let marker = value.as_bytes().get(digit_count)?;
    if !matches!(marker, b'.' | b')') {
        return None;
    }
    let content = &value[digit_count + 1..];
    if !content.is_empty() && !content.starts_with(' ') && !content.starts_with('\t') {
        return None;
    }

    Some((2, level, content.trim_start_matches([' ', '\t'])))
}
