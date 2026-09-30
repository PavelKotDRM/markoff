use super::super::*;
use epaint_default_fonts::HACK_REGULAR;
use std::sync::OnceLock;
use unicode_bidi::BidiInfo;
use unicode_segmentation::UnicodeSegmentation;

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

pub(in crate::pdf_writer) fn uses_emoji_font(grapheme: &str, code: bool) -> bool {
    let style = InlineStyle {
        code,
        ..InlineStyle::default()
    };
    let unavailable_in_primary_font = grapheme
        .chars()
        .any(|character| primary_face(&style).glyph_index(character).is_none());
    unavailable_in_primary_font
        && twemoji_assets::png::PngTwemojiAsset::from_emoji(grapheme).is_some()
}

fn grapheme_width(grapheme: &str, style: &InlineStyle, font_size: f32) -> f32 {
    if uses_emoji_font(grapheme, style.code) {
        return font_size;
    }
    grapheme
        .chars()
        .map(|character| {
            let face = font_face(style);
            let Some(glyph) = face.glyph_index(character) else {
                return font_size * 0.6;
            };
            let advance = face.glyph_hor_advance(glyph).unwrap_or(face.units_per_em());
            font_size * f32::from(advance) / f32::from(face.units_per_em())
        })
        .sum()
}

pub(in crate::pdf_writer) fn estimate_run_width(run: &TextRun, font_size: f32) -> f32 {
    let size = if run.style.superscript || run.style.subscript {
        font_size * 0.72
    } else {
        font_size
    };
    run.text
        .graphemes(true)
        .map(|grapheme| grapheme_width(grapheme, &run.style, size))
        .sum()
}

#[derive(Clone)]
struct StyledGrapheme {
    text: String,
    style: InlineStyle,
}

pub(in crate::pdf_writer) fn wrap_runs(
    runs: &[TextRun],
    available_width: f32,
    font_size: f32,
    preserve_whitespace: bool,
) -> Vec<Vec<TextRun>> {
    let available_width = available_width.max(1.0);
    let mut lines = Vec::<Vec<StyledGrapheme>>::new();
    let mut line = Vec::<StyledGrapheme>::new();
    let mut word = Vec::<StyledGrapheme>::new();
    let mut pending_space: Option<StyledGrapheme> = None;

    for run in runs {
        for grapheme in run.text.graphemes(true) {
            let styled = StyledGrapheme {
                text: grapheme.to_string(),
                style: run.style.clone(),
            };
            if preserve_whitespace {
                if matches!(grapheme, "\n" | "\r\n") {
                    lines.push(std::mem::take(&mut line));
                } else {
                    if graphemes_width(&line, font_size)
                        + grapheme_width(grapheme, &styled.style, font_size)
                        > available_width
                        && !line.is_empty()
                    {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.push(styled);
                }
            } else if matches!(grapheme, "\n" | "\r\n") {
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
            } else if grapheme.chars().all(char::is_whitespace) {
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
    lines.into_iter().map(graphemes_to_runs).collect()
}

fn place_word(
    line: &mut Vec<StyledGrapheme>,
    lines: &mut Vec<Vec<StyledGrapheme>>,
    word: &mut Vec<StyledGrapheme>,
    pending_space: &mut Option<StyledGrapheme>,
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
            grapheme_width(&space.text, &space.style, font_size)
        });
    if graphemes_width(line, font_size) + space_width + graphemes_width(word, font_size)
        > available_width
        && !line.is_empty()
    {
        lines.push(std::mem::take(line));
        *pending_space = None;
    }
    if let Some(space) = pending_space.take()
        && !line.is_empty()
        && graphemes_width(line, font_size) + grapheme_width(&space.text, &space.style, font_size)
            <= available_width
    {
        line.push(space);
    }

    for grapheme in word.drain(..) {
        if graphemes_width(line, font_size)
            + grapheme_width(&grapheme.text, &grapheme.style, font_size)
            > available_width
            && !line.is_empty()
        {
            lines.push(std::mem::take(line));
        }
        line.push(grapheme);
    }
}

fn graphemes_width(graphemes: &[StyledGrapheme], font_size: f32) -> f32 {
    graphemes
        .iter()
        .map(|item| grapheme_width(&item.text, &item.style, font_size))
        .sum()
}

fn graphemes_to_runs(graphemes: Vec<StyledGrapheme>) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = Vec::new();
    for item in graphemes {
        if let Some(last) = runs.last_mut()
            && last.style == item.style
        {
            last.text.push_str(&item.text);
        } else {
            runs.push(TextRun {
                text: item.text,
                style: item.style,
            });
        }
    }
    runs
}

pub(in crate::pdf_writer) fn reorder_runs_for_display(runs: &[TextRun]) -> Vec<TextRun> {
    let graphemes = runs
        .iter()
        .flat_map(|run| {
            run.text.graphemes(true).map(|text| StyledGrapheme {
                text: text.to_string(),
                style: run.style.clone(),
            })
        })
        .collect::<Vec<_>>();
    let text = graphemes
        .iter()
        .map(|item| item.text.as_str())
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
    let mut character_to_grapheme = Vec::new();
    for (index, grapheme) in graphemes.iter().enumerate() {
        character_to_grapheme.extend(std::iter::repeat_n(index, grapheme.text.chars().count()));
    }
    let mut previous = None;
    let reordered = visual_order
        .into_iter()
        .filter_map(|character_index| character_to_grapheme.get(character_index).copied())
        .filter_map(|grapheme_index| {
            if previous.replace(grapheme_index) == Some(grapheme_index) {
                None
            } else {
                graphemes.get(grapheme_index).cloned()
            }
        })
        .collect();
    graphemes_to_runs(reordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compound_emoji_is_measured_and_wrapped_as_one_grapheme() {
        let emoji = "👩🏽‍💻";
        assert!(uses_emoji_font(emoji, false));
        let run = TextRun {
            text: format!("{emoji}{emoji}"),
            style: InlineStyle::default(),
        };
        assert_eq!(estimate_run_width(&run, 12.0), 24.0);
        let lines = wrap_runs(&[run], 12.1, 12.0, false);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| line[0].text == emoji));
    }
}
