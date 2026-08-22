pub(crate) fn convert_textual_footnotes(markdown: &mut Vec<String>) {
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

fn textual_footnote_definition(paragraph: &str) -> Option<(String, String)> {
    let remainder = paragraph.trim_start().strip_prefix("\\[")?;
    let (label, rest) = remainder.split_once("\\]")?;
    let content = rest.strip_prefix(' ')?;
    let is_valid_label = !label.is_empty()
        && (label.bytes().all(|byte| byte.is_ascii_digit())
            || label.chars().all(|character| "ivxlcdm".contains(character)));
    is_valid_label.then(|| (label.to_string(), content.to_string()))
}

pub(crate) fn convert_formula_section(markdown: &mut [String]) {
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

fn matrix_row_to_latex(line: &str) -> Option<String> {
    let inner = line.trim().strip_prefix("\\[")?.strip_suffix("\\]")?.trim();
    (!inner.is_empty()).then(|| inner.split_whitespace().collect::<Vec<_>>().join(" & "))
}

fn latex_math_from_plain_text(text: &str) -> String {
    let text = strip_embedded_math_delimiters(text);
    let text = text.replace("\\[", "[").replace("\\]", "]");
    let text = sqrt_to_latex(&text);
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

fn unescape_math_backslashes(text: &str) -> String {
    text.replace("\\\\", "\\")
}
