use std::collections::HashMap;

use super::hyperlinks::HyperlinkAllocator;
use super::images::{ImageAllocator, parse_image_destination};
use crate::docx_inline::markdown_inline_to_docx_runs_with_links;

pub(super) struct Footnote {
    pub(super) id: usize,
    pub(super) label: String,
    pub(super) kind: NoteKind,
    paragraphs: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NoteKind {
    Footnote,
    Endnote,
}

#[derive(Clone, Copy)]
pub(super) struct NoteReference {
    pub(super) id: usize,
    pub(super) kind: NoteKind,
}

pub(super) fn extract_footnotes(source: &str) -> (String, Vec<Footnote>) {
    let lines = source.lines().collect::<Vec<_>>();
    let mut footnotes = Vec::new();
    let mut body = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        if let Some((label, content)) = footnote_definition(lines[index]) {
            let mut paragraphs = vec![content.to_string()];
            index += 1;
            while index < lines.len() {
                if let Some(content) = lines[index]
                    .strip_prefix("    ")
                    .or_else(|| lines[index].strip_prefix('\t'))
                {
                    paragraphs.push(content.to_string());
                    index += 1;
                } else if lines[index].trim().is_empty()
                    && lines
                        .get(index + 1)
                        .is_some_and(|line| line.starts_with("    ") || line.starts_with('\t'))
                {
                    paragraphs.push(String::new());
                    index += 1;
                } else {
                    break;
                }
            }
            let kind = note_kind(label);
            footnotes.push(Footnote {
                id: next_note_id(&footnotes, kind),
                label: label.to_string(),
                kind,
                paragraphs,
            });
            continue;
        }

        body.push(lines[index].to_string());
        index += 1;
    }

    let mut inline_sequence = 1;
    for line in &mut body {
        *line = replace_inline_footnotes(line, &mut footnotes, &mut inline_sequence);
    }
    (body.join("\n"), footnotes)
}

fn footnote_definition(line: &str) -> Option<(&str, &str)> {
    let remainder = line.strip_prefix("[^")?;
    let (label, content) = remainder.split_once("]:")?;
    (!label.is_empty()).then_some((label, content.trim_start()))
}

fn replace_inline_footnotes(
    line: &str,
    footnotes: &mut Vec<Footnote>,
    inline_sequence: &mut usize,
) -> String {
    let mut rendered = String::new();
    let mut remaining = line;
    while let Some(start) = remaining.find("^[") {
        rendered.push_str(&remaining[..start]);
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find(']') else {
            rendered.push_str(&remaining[start..]);
            return rendered;
        };
        let label = format!("inline-{}", *inline_sequence);
        *inline_sequence += 1;
        footnotes.push(Footnote {
            id: next_note_id(footnotes, NoteKind::Footnote),
            label: label.clone(),
            kind: NoteKind::Footnote,
            paragraphs: vec![after_start[..end].to_string()],
        });
        rendered.push_str("[^");
        rendered.push_str(&label);
        rendered.push(']');
        remaining = &after_start[end + 1..];
    }
    rendered.push_str(remaining);
    rendered
}

fn note_kind(label: &str) -> NoteKind {
    if label.starts_with("endnote-") {
        NoteKind::Endnote
    } else {
        NoteKind::Footnote
    }
}

fn next_note_id(footnotes: &[Footnote], kind: NoteKind) -> usize {
    footnotes.iter().filter(|note| note.kind == kind).count() + 1
}

pub(super) fn markdown_inline_to_docx_runs_with_footnotes(
    value: &str,
    footnote_ids: &HashMap<&str, NoteReference>,
    hyperlinks: &mut HyperlinkAllocator,
    images: &ImageAllocator,
) -> String {
    let (value, bookmarks) = extract_bookmark_markers(value);
    let rendered = value
        .split('\n')
        .map(|line| {
            markdown_inline_to_docx_runs_without_breaks(line, footnote_ids, hyperlinks, images)
        })
        .collect::<Vec<_>>()
        .join("<w:r><w:br/></w:r>");
    hyperlinks.wrap_bookmarks(rendered, bookmarks)
}

fn extract_bookmark_markers(value: &str) -> (String, Vec<String>) {
    let mut remaining = value;
    let mut text = String::with_capacity(value.len());
    let mut bookmarks = Vec::new();
    while let Some(start) = remaining.find("<a id=\"") {
        let marker_start = start + "<a id=\"".len();
        let Some(name_end) = remaining[marker_start..].find('"') else {
            break;
        };
        let name_end = marker_start + name_end;
        let marker_end = name_end + 1;
        let Some(closing) = remaining[marker_end..].strip_prefix("></a>") else {
            text.push_str(&remaining[..start + 1]);
            remaining = &remaining[start + 1..];
            continue;
        };
        text.push_str(&remaining[..start]);
        bookmarks.push(remaining[marker_start..name_end].to_string());
        remaining = closing;
    }
    text.push_str(remaining);
    (text, bookmarks)
}

fn markdown_inline_to_docx_runs_without_breaks(
    value: &str,
    footnote_ids: &HashMap<&str, NoteReference>,
    hyperlinks: &mut HyperlinkAllocator,
    images: &ImageAllocator,
) -> String {
    let mut runs = String::new();
    let mut remaining = value;
    loop {
        let image_start = remaining.find("![");
        let note_start = remaining.find("[^");
        let Some(start) = [image_start, note_start].into_iter().flatten().min() else {
            break;
        };
        let mut resolve = |destination: &str| hyperlinks.resolve(destination);
        runs.push_str(&markdown_inline_to_docx_runs_with_links(
            &remaining[..start],
            &mut resolve,
        ));

        if image_start == Some(start)
            && let Some((alt, destination, consumed)) = parse_image_destination(&remaining[start..])
            && let Some(drawing) = images.drawing_xml(destination, alt)
        {
            runs.push_str(&drawing);
            remaining = &remaining[start + consumed..];
            continue;
        }

        if note_start == Some(start) {
            let after_start = &remaining[start + 2..];
            if let Some(end) = after_start.find(']') {
                let label = &after_start[..end];
                if let Some(reference) = footnote_ids.get(label) {
                    let element = match reference.kind {
                        NoteKind::Footnote => "footnoteReference",
                        NoteKind::Endnote => "endnoteReference",
                    };
                    runs.push_str(&format!(
                        "<w:r><w:{element} w:id=\"{}\"/></w:r>",
                        reference.id
                    ));
                    remaining = &after_start[end + 1..];
                    continue;
                }
                let mut resolve = |destination: &str| hyperlinks.resolve(destination);
                runs.push_str(&markdown_inline_to_docx_runs_with_links(
                    &remaining[start..start + end + 3],
                    &mut resolve,
                ));
                remaining = &after_start[end + 1..];
                continue;
            }
        }

        let mut resolve = |destination: &str| hyperlinks.resolve(destination);
        runs.push_str(&markdown_inline_to_docx_runs_with_links(
            &remaining[start..start + 2],
            &mut resolve,
        ));
        remaining = &remaining[start + 2..];
    }
    let mut resolve = |destination: &str| hyperlinks.resolve(destination);
    runs.push_str(&markdown_inline_to_docx_runs_with_links(
        remaining,
        &mut resolve,
    ));
    runs
}

pub(super) fn render_footnotes(
    footnotes: &[Footnote],
    footnote_ids: &HashMap<&str, NoteReference>,
    hyperlinks: &mut HyperlinkAllocator,
    images: &ImageAllocator,
    paragraph_properties: &str,
) -> String {
    render_notes(
        footnotes,
        NoteKind::Footnote,
        footnote_ids,
        hyperlinks,
        images,
        paragraph_properties,
    )
}

pub(super) fn render_endnotes(
    footnotes: &[Footnote],
    footnote_ids: &HashMap<&str, NoteReference>,
    hyperlinks: &mut HyperlinkAllocator,
    images: &ImageAllocator,
    paragraph_properties: &str,
) -> String {
    render_notes(
        footnotes,
        NoteKind::Endnote,
        footnote_ids,
        hyperlinks,
        images,
        paragraph_properties,
    )
}

fn render_notes(
    footnotes: &[Footnote],
    kind: NoteKind,
    footnote_ids: &HashMap<&str, NoteReference>,
    hyperlinks: &mut HyperlinkAllocator,
    images: &ImageAllocator,
    paragraph_properties: &str,
) -> String {
    let (note_element, notes_element) = match kind {
        NoteKind::Footnote => ("footnote", "footnotes"),
        NoteKind::Endnote => ("endnote", "endnotes"),
    };
    let entries = footnotes
        .iter()
        .filter(|footnote| footnote.kind == kind)
        .map(|footnote| {
            let paragraphs = footnote
                .paragraphs
                .split(|paragraph| paragraph.is_empty())
                .filter(|paragraph| !paragraph.is_empty())
                .map(|paragraph| {
                    format!(
                        "<w:p>{paragraph_properties}{}</w:p>",
                        markdown_inline_to_docx_runs_with_footnotes(
                            &paragraph.join("\n"),
                            footnote_ids,
                            hyperlinks,
                            images,
                        )
                    )
                })
                .collect::<String>();
            let reference = if kind == NoteKind::Endnote {
                "<w:r><w:endnoteRef/></w:r>"
            } else {
                ""
            };
            format!(
                "<w:{note_element} w:id=\"{}\"><w:p>{paragraph_properties}{reference}</w:p>{paragraphs}</w:{note_element}>",
                footnote.id
            )
        })
        .collect::<String>();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:{notes_element} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\"><w:{note_element} w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:{note_element}><w:{note_element} w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{note_element}>{entries}</w:{notes_element}>"
    )
}
