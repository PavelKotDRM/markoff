use std::collections::HashMap;

use super::hyperlinks::HyperlinkAllocator;
use crate::docx_inline::markdown_inline_to_docx_runs_with_links;

pub(super) struct Footnote {
    pub(super) id: usize,
    pub(super) label: String,
    paragraphs: Vec<String>,
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
            footnotes.push(Footnote {
                id: footnotes.len() + 1,
                label: label.to_string(),
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
            id: footnotes.len() + 1,
            label: label.clone(),
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

pub(super) fn markdown_inline_to_docx_runs_with_footnotes(
    value: &str,
    footnote_ids: &HashMap<&str, usize>,
    hyperlinks: &mut HyperlinkAllocator,
) -> String {
    let (value, bookmarks) = extract_bookmark_markers(value);
    let rendered = value
        .split('\n')
        .map(|line| markdown_inline_to_docx_runs_without_breaks(line, footnote_ids, hyperlinks))
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
    footnote_ids: &HashMap<&str, usize>,
    hyperlinks: &mut HyperlinkAllocator,
) -> String {
    let mut runs = String::new();
    let mut remaining = value;
    while let Some(start) = remaining.find("[^") {
        let before = &remaining[..start];
        let mut resolve = |destination: &str| hyperlinks.resolve(destination);
        runs.push_str(&markdown_inline_to_docx_runs_with_links(
            before,
            &mut resolve,
        ));
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find(']') else {
            let mut resolve = |destination: &str| hyperlinks.resolve(destination);
            runs.push_str(&markdown_inline_to_docx_runs_with_links(
                &remaining[start..],
                &mut resolve,
            ));
            return runs;
        };
        let label = &after_start[..end];
        if let Some(id) = footnote_ids.get(label) {
            runs.push_str(&format!("<w:r><w:footnoteReference w:id=\"{id}\"/></w:r>"));
        } else {
            let mut resolve = |destination: &str| hyperlinks.resolve(destination);
            runs.push_str(&markdown_inline_to_docx_runs_with_links(
                &remaining[start..start + end + 3],
                &mut resolve,
            ));
        }
        remaining = &after_start[end + 1..];
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
    hyperlinks: &mut HyperlinkAllocator,
) -> String {
    let entries = footnotes
        .iter()
        .map(|footnote| {
            let paragraphs = footnote
                .paragraphs
                .split(|paragraph| paragraph.is_empty())
                .filter(|paragraph| !paragraph.is_empty())
                .map(|paragraph| {
                    let mut resolve = |destination: &str| hyperlinks.resolve(destination);
                    format!(
                        "<w:p>{}</w:p>",
                        markdown_inline_to_docx_runs_with_links(
                            &paragraph.join("\n"),
                            &mut resolve
                        )
                    )
                })
                .collect::<String>();
            format!(
                "<w:footnote w:id=\"{}\">{paragraphs}</w:footnote>",
                footnote.id
            )
        })
        .collect::<String>();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:footnotes xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>{entries}</w:footnotes>"
    )
}
