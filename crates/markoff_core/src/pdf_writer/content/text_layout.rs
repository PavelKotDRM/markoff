use super::super::*;
use epaint_default_fonts::HACK_REGULAR;
use std::sync::OnceLock;
use unicode_bidi::BidiInfo;

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

fn primary_face(style: &InlineStyle) -> &'static ttf_parser::Face<'static> {
    static REGULAR: OnceLock<ttf_parser::Face<'static>> = OnceLock::new();
    static BOLD: OnceLock<ttf_parser::Face<'static>> = OnceLock::new();
    static ITALIC: OnceLock<ttf_parser::Face<'static>> = OnceLock::new();
    static BOLD_ITALIC: OnceLock<ttf_parser::Face<'static>> = OnceLock::new();
    static CODE: OnceLock<ttf_parser::Face<'static>> = OnceLock::new();

    if style.code {
        CODE.get_or_init(|| {
            ttf_parser::Face::parse(HACK_REGULAR, 0).expect("embedded Hack font is valid")
        })
    } else if style.bold && style.italic {
        BOLD_ITALIC.get_or_init(|| {
            ttf_parser::Face::parse(dejavu::sans::bold_oblique(), 0)
                .expect("embedded DejaVu bold oblique font is valid")
        })
    } else if style.bold {
        BOLD.get_or_init(|| {
            ttf_parser::Face::parse(dejavu::sans::bold(), 0)
                .expect("embedded DejaVu bold font is valid")
        })
    } else if style.italic {
        ITALIC.get_or_init(|| {
            ttf_parser::Face::parse(dejavu::sans::oblique(), 0)
                .expect("embedded DejaVu oblique font is valid")
        })
    } else {
        REGULAR.get_or_init(|| {
            ttf_parser::Face::parse(dejavu::sans::regular(), 0)
                .expect("embedded DejaVu font is valid")
        })
    }
}

fn font_face(style: &InlineStyle) -> &'static ttf_parser::Face<'static> {
    primary_face(style)
}

pub(in crate::pdf_writer) fn uses_emoji_font(character: char, code: bool) -> bool {
    let style = InlineStyle {
        code,
        ..InlineStyle::default()
    };
    primary_face(&style).glyph_index(character).is_none()
        && twemoji_assets::png::PngTwemojiAsset::from_emoji(&character.to_string()).is_some()
}

fn character_width(character: char, style: &InlineStyle, font_size: f32) -> f32 {
    if uses_emoji_font(character, style.code) {
        return font_size;
    }
    let face = font_face(style);
    let Some(glyph) = face.glyph_index(character) else {
        return font_size * 0.6;
    };
    let advance = face.glyph_hor_advance(glyph).unwrap_or(face.units_per_em());
    font_size * f32::from(advance) / f32::from(face.units_per_em())
}

pub(in crate::pdf_writer) fn estimate_run_width(run: &TextRun, font_size: f32) -> f32 {
    let size = if run.style.superscript || run.style.subscript {
        font_size * 0.72
    } else {
        font_size
    };
    run.text
        .chars()
        .map(|character| character_width(character, &run.style, size))
        .sum()
}

#[derive(Clone)]
struct StyledCharacter {
    character: char,
    style: InlineStyle,
}

pub(in crate::pdf_writer) fn wrap_runs(
    runs: &[TextRun],
    available_width: f32,
    font_size: f32,
    preserve_whitespace: bool,
) -> Vec<Vec<TextRun>> {
    let available_width = available_width.max(1.0);
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
                    if characters_width(&line, font_size)
                        + character_width(character, &styled.style, font_size)
                        > available_width
                        && !line.is_empty()
                    {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.push(styled);
                }
            } else if character == '\n' {
                place_word(
                    &mut line,
                    &mut lines,
                    &mut word,
                    &mut pending_space,
                    available_width,
                    font_size,
                );
                lines.push(std::mem::take(&mut line));
                pending_space = None;
            } else if character.is_whitespace() {
                place_word(
                    &mut line,
                    &mut lines,
                    &mut word,
                    &mut pending_space,
                    available_width,
                    font_size,
                );
                if !line.is_empty() {
                    pending_space = Some(styled);
                }
            } else {
                word.push(styled);
            }
        }
    }
    place_word(
        &mut line,
        &mut lines,
        &mut word,
        &mut pending_space,
        available_width,
        font_size,
    );
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
    available_width: f32,
    font_size: f32,
) {
    if word.is_empty() {
        return;
    }
    let space_width = pending_space
        .as_ref()
        .filter(|_| !line.is_empty())
        .map_or(0.0, |space| {
            character_width(space.character, &space.style, font_size)
        });
    if characters_width(line, font_size) + space_width + characters_width(word, font_size)
        > available_width
        && !line.is_empty()
    {
        lines.push(std::mem::take(line));
        *pending_space = None;
    }
    if let Some(space) = pending_space.take()
        && !line.is_empty()
        && characters_width(line, font_size)
            + character_width(space.character, &space.style, font_size)
            <= available_width
    {
        line.push(space);
    }

    for character in word.drain(..) {
        if characters_width(line, font_size)
            + character_width(character.character, &character.style, font_size)
            > available_width
            && !line.is_empty()
        {
            lines.push(std::mem::take(line));
        }
        line.push(character);
    }
}

fn characters_width(characters: &[StyledCharacter], font_size: f32) -> f32 {
    characters
        .iter()
        .map(|item| character_width(item.character, &item.style, font_size))
        .sum()
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

pub(in crate::pdf_writer) fn reorder_runs_for_display(runs: &[TextRun]) -> Vec<TextRun> {
    let characters = runs
        .iter()
        .flat_map(|run| {
            run.text.chars().map(|character| StyledCharacter {
                character,
                style: run.style.clone(),
            })
        })
        .collect::<Vec<_>>();
    let text = characters
        .iter()
        .map(|item| item.character)
        .collect::<String>();
    let bidi = BidiInfo::new(&text, None);
    if !bidi.has_rtl() {
        return runs.to_vec();
    }
    let Some(paragraph) = bidi.paragraphs.first() else {
        return runs.to_vec();
    };
    let levels = bidi.reordered_levels_per_char(paragraph, paragraph.range.clone());
    let visual_order = BidiInfo::reorder_visual(&levels);
    characters_to_runs(
        visual_order
            .into_iter()
            .filter_map(|index| characters.get(index).cloned())
            .collect(),
    )
}
