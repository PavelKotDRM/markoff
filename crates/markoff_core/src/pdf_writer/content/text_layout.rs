use super::super::*;

pub(in crate::pdf_writer) fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '_' {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            separator = false;
        } else if character == '-' {
            if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
            separator = false;
        } else {
            separator = true;
        }
    }
    slug.trim_matches('-').to_string()
}

pub(in crate::pdf_writer) fn normalize_anchor(anchor: &str) -> String {
    let anchor = anchor.strip_prefix('#').unwrap_or(anchor);
    slugify(anchor)
}

fn estimate_character_width(character: char, style: &InlineStyle, font_size: f32) -> f32 {
    if style.code {
        return font_size * 0.61;
    }
    let factor = if character.is_whitespace() {
        0.31
    } else if character.is_uppercase() {
        0.64
    } else if character.is_ascii_punctuation() {
        0.42
    } else {
        0.54
    };
    font_size * factor
}

pub(in crate::pdf_writer) fn estimate_run_width(run: &TextRun, font_size: f32) -> f32 {
    let size = if run.style.superscript || run.style.subscript {
        font_size * 0.72
    } else {
        font_size
    };
    run.text
        .chars()
        .map(|character| estimate_character_width(character, &run.style, size))
        .sum()
}

#[derive(Clone)]
struct StyledCharacter {
    character: char,
    style: InlineStyle,
}

pub(in crate::pdf_writer) fn wrap_runs(
    runs: &[TextRun],
    limit: usize,
    preserve_whitespace: bool,
) -> Vec<Vec<TextRun>> {
    let limit = limit.max(1);
    let mut lines = Vec::<Vec<StyledCharacter>>::new();
    let mut line = Vec::<StyledCharacter>::new();
    let mut word = Vec::<StyledCharacter>::new();
    let mut pending_space: Option<StyledCharacter> = None;

    for run in runs {
        for character in run.text.chars() {
            let styled = StyledCharacter {
                character,
                style: run.style.clone(),
            };
            if preserve_whitespace {
                if character == '\n' {
                    lines.push(std::mem::take(&mut line));
                } else {
                    if line.len() >= limit {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.push(styled);
                }
            } else if character == '\n' {
                place_word(&mut line, &mut lines, &mut word, &mut pending_space, limit);
                lines.push(std::mem::take(&mut line));
                pending_space = None;
            } else if character.is_whitespace() {
                place_word(&mut line, &mut lines, &mut word, &mut pending_space, limit);
                if !line.is_empty() {
                    pending_space = Some(styled);
                }
            } else {
                word.push(styled);
            }
        }
    }
    place_word(&mut line, &mut lines, &mut word, &mut pending_space, limit);
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines.into_iter().map(characters_to_runs).collect()
}

fn place_word(
    line: &mut Vec<StyledCharacter>,
    lines: &mut Vec<Vec<StyledCharacter>>,
    word: &mut Vec<StyledCharacter>,
    pending_space: &mut Option<StyledCharacter>,
    limit: usize,
) {
    if word.is_empty() {
        return;
    }
    let space_count = usize::from(pending_space.is_some() && !line.is_empty());
    if line.len() + space_count + word.len() > limit && !line.is_empty() {
        if let Some(space_index) = line.iter().rposition(|item| item.character.is_whitespace()) {
            let remaining = line.split_off(space_index + 1);
            while line
                .last()
                .is_some_and(|item| item.character.is_whitespace())
            {
                line.pop();
            }
            lines.push(std::mem::take(line));
            *line = remaining;
        } else {
            lines.push(std::mem::take(line));
        }
        *pending_space = None;
    }
    if let Some(space) = pending_space.take()
        && !line.is_empty()
        && line.len() < limit
    {
        line.push(space);
    }

    for character in word.drain(..) {
        if line.len() >= limit {
            lines.push(std::mem::take(line));
        }
        line.push(character);
    }
}

fn characters_to_runs(characters: Vec<StyledCharacter>) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = Vec::new();
    for item in characters {
        if let Some(last) = runs.last_mut()
            && last.style == item.style
        {
            last.text.push(item.character);
        } else {
            runs.push(TextRun {
                text: item.character.to_string(),
                style: item.style,
            });
        }
    }
    runs
}
