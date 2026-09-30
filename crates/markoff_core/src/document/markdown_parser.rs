use super::inline_parser::{ProtectedInline, parse_inline_events, protect_inline_syntax};
use crate::MarkoffError;
use crate::document_model::{Block, Document, Inline, ListItem, TableAlignment};
use crate::docx_inline::markdown_list_item;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

const MAX_DOCUMENT_NESTING_DEPTH: usize = 128;

struct ParseContext<'a> {
    offset_source: &'a str,
    original_source: &'a str,
    base_dir: &'a Path,
    placeholders: &'a HashMap<String, ProtectedInline>,
}

pub(super) fn markdown_to_document(
    markdown: &str,
    base_dir: &Path,
) -> Result<Document, MarkoffError> {
    let (source, placeholders) = protect_inline_syntax(markdown, false);
    let options = markdown_options();
    let events = Parser::new_ext(&source, options)
        .into_offset_iter()
        .collect::<Vec<_>>();
    let mut cursor = 0;
    let context = ParseContext {
        offset_source: &source,
        original_source: markdown,
        base_dir,
        placeholders: &placeholders,
    };
    Ok(Document {
        blocks: parse_blocks(&events, &mut cursor, None, &context, 0)?,
    })
}

pub(super) fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_MATH);
    options.insert(Options::ENABLE_SUPERSCRIPT);
    options.insert(Options::ENABLE_SUBSCRIPT);
    options
}

fn parse_blocks<'a>(
    events: &[(Event<'a>, Range<usize>)],
    cursor: &mut usize,
    stop: Option<TagEnd>,
    context: &ParseContext<'_>,
    depth: usize,
) -> Result<Vec<Block>, MarkoffError> {
    if depth > MAX_DOCUMENT_NESTING_DEPTH {
        return Err(MarkoffError::InvalidInput {
            path: format!(
                "Markdown nesting exceeds the supported depth of {MAX_DOCUMENT_NESTING_DEPTH}"
            ),
        });
    }
    let mut blocks = Vec::new();
    let mut pending_task_marker = None;
    while let Some((event, range)) = events.get(*cursor) {
        let event = event.clone();
        let range = range.clone();
        *cursor += 1;

        match event {
            Event::End(end) if stop == Some(end) => break,
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    let mut content = parse_inline_events(
                        events,
                        cursor,
                        Some(TagEnd::Paragraph),
                        context.base_dir,
                        context.placeholders,
                    )?;
                    if let Some(checked) = pending_task_marker.take() {
                        content.insert(0, Inline::TaskListMarker { checked });
                    }
                    blocks.push(Block::Paragraph {
                        text: None,
                        content,
                    });
                }
                Tag::Heading { level, .. } => blocks.push(Block::Heading {
                    level: level as u8,
                    text: None,
                    content: parse_inline_events(
                        events,
                        cursor,
                        Some(TagEnd::Heading(level)),
                        context.base_dir,
                        context.placeholders,
                    )?,
                }),
                Tag::BlockQuote(_) => blocks.push(Block::Quote {
                    text: None,
                    blocks: parse_blocks(
                        events,
                        cursor,
                        Some(TagEnd::BlockQuote(None)),
                        context,
                        depth + 1,
                    )?,
                }),
                Tag::List(start) => {
                    blocks.push(parse_list(events, cursor, start, context, depth + 1)?)
                }
                Tag::CodeBlock(kind) => {
                    blocks.push(parse_code_block(events, cursor, kind));
                }
                Tag::FootnoteDefinition(label) => blocks.push(Block::FootnoteDefinition {
                    label: label.to_string(),
                    blocks: parse_blocks(
                        events,
                        cursor,
                        Some(TagEnd::FootnoteDefinition),
                        context,
                        depth + 1,
                    )?,
                }),
                Tag::Table(alignments) => blocks.push(parse_table(
                    events,
                    cursor,
                    alignments,
                    context.base_dir,
                    context.placeholders,
                )?),
                Tag::HtmlBlock => blocks.push(parse_html_block(events, cursor)),
                unsupported => {
                    return Err(MarkoffError::InvalidInput {
                        path: format!("unsupported Markdown block: {unsupported:?}"),
                    });
                }
            },
            Event::Rule => blocks.push(Block::HorizontalRule),
            Event::DisplayMath(text) => blocks.push(Block::Math {
                text: text.to_string(),
            }),
            Event::Html(html) | Event::InlineHtml(html) => {
                blocks.push(Block::Html {
                    html: html.to_string(),
                });
            }
            Event::Text(text) => blocks.push(Block::Paragraph {
                text: None,
                content: vec![Inline::Text {
                    text: text.to_string(),
                }],
            }),
            Event::Code(code) => blocks.push(Block::Paragraph {
                text: None,
                content: vec![Inline::Code {
                    text: code.to_string(),
                }],
            }),
            Event::FootnoteReference(label) => blocks.push(Block::Paragraph {
                text: None,
                content: vec![Inline::FootnoteReference {
                    label: label.to_string(),
                }],
            }),
            Event::TaskListMarker(checked) => {
                if let Some(Block::Paragraph { content, .. }) = blocks.last_mut() {
                    content.insert(0, Inline::TaskListMarker { checked });
                } else {
                    pending_task_marker = Some(checked);
                }
            }
            Event::InlineMath(text) => blocks.push(Block::Paragraph {
                text: None,
                content: vec![Inline::Math {
                    text: text.to_string(),
                    display: false,
                }],
            }),
            Event::End(_) | Event::SoftBreak | Event::HardBreak => {}
        }

        if stop.is_some() && *cursor >= events.len() {
            break;
        }
        let _ = range;
    }
    Ok(blocks)
}

fn parse_list<'a>(
    events: &[(Event<'a>, Range<usize>)],
    cursor: &mut usize,
    start: Option<u64>,
    context: &ParseContext<'_>,
    depth: usize,
) -> Result<Block, MarkoffError> {
    let ordered = start.is_some();
    let mut items = Vec::new();
    while let Some((event, range)) = events.get(*cursor) {
        match event {
            Event::End(TagEnd::List(_)) => {
                *cursor += 1;
                break;
            }
            Event::Start(Tag::Item) => {
                let item_offset = range.start;
                *cursor += 1;
                let blocks = parse_blocks(events, cursor, Some(TagEnd::Item), context, depth)?;
                let number = ordered
                    .then(|| {
                        list_item_number(
                            context.offset_source,
                            context.original_source,
                            item_offset,
                        )
                    })
                    .flatten();
                let task_checked = list_item_task_marker(
                    context.offset_source,
                    context.original_source,
                    item_offset,
                );
                let mut blocks = blocks;
                if let Some(checked) = task_checked
                    && let Some(Block::Paragraph { content, .. }) = blocks.first_mut()
                    && !content
                        .iter()
                        .any(|inline| matches!(inline, Inline::TaskListMarker { .. }))
                {
                    content.insert(0, Inline::TaskListMarker { checked });
                }
                items.push(ListItem { number, blocks });
            }
            _ => *cursor += 1,
        }
    }
    Ok(Block::List {
        ordered,
        start,
        items,
    })
}

fn list_item_number(offset_source: &str, original_source: &str, offset: usize) -> Option<u64> {
    let line_index = offset_source[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();
    let line = original_source.lines().nth(line_index)?;
    let line = line.trim_start();
    let (_, _, _) = markdown_list_item(line)?;
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    (digits > 0).then(|| line[..digits].parse().ok()).flatten()
}

fn list_item_task_marker(
    offset_source: &str,
    original_source: &str,
    offset: usize,
) -> Option<bool> {
    let line_index = offset_source[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();
    let line = original_source.lines().nth(line_index)?.trim_start();
    let (_, _, content) = markdown_list_item(line)?;
    let content = content.trim_start();
    let checked = match content.as_bytes().get(..3)? {
        b"[x]" | b"[X]" => true,
        b"[ ]" => false,
        _ => return None,
    };
    (content.len() == 3
        || content
            .as_bytes()
            .get(3)
            .is_some_and(u8::is_ascii_whitespace))
    .then_some(checked)
}

fn parse_code_block<'a>(
    events: &[(Event<'a>, Range<usize>)],
    cursor: &mut usize,
    kind: CodeBlockKind<'a>,
) -> Block {
    let info = match kind {
        CodeBlockKind::Fenced(info) if !info.is_empty() => Some(info.to_string()),
        CodeBlockKind::Fenced(_) | CodeBlockKind::Indented => None,
    };
    let mut code = String::new();
    while let Some((event, _)) = events.get(*cursor) {
        let event = event.clone();
        *cursor += 1;
        match event {
            Event::End(TagEnd::CodeBlock) => break,
            Event::Text(text) => code.push_str(&text),
            Event::SoftBreak | Event::HardBreak => code.push('\n'),
            _ => {}
        }
    }
    let code = code.strip_suffix('\n').unwrap_or(&code).to_string();
    Block::Code { code, info }
}

fn parse_table<'a>(
    events: &[(Event<'a>, Range<usize>)],
    cursor: &mut usize,
    alignments: Vec<Alignment>,
    base_dir: &Path,
    placeholders: &HashMap<String, ProtectedInline>,
) -> Result<Block, MarkoffError> {
    let mut rows = Vec::new();
    while let Some((event, _)) = events.get(*cursor) {
        match event {
            Event::End(TagEnd::Table) => {
                *cursor += 1;
                break;
            }
            Event::Start(Tag::TableHead) => {
                *cursor += 1;
                rows.push(parse_table_row(
                    events,
                    cursor,
                    TagEnd::TableHead,
                    base_dir,
                    placeholders,
                )?);
            }
            Event::Start(Tag::TableRow) => {
                *cursor += 1;
                rows.push(parse_table_row(
                    events,
                    cursor,
                    TagEnd::TableRow,
                    base_dir,
                    placeholders,
                )?);
            }
            _ => *cursor += 1,
        }
    }
    let alignments = alignments
        .into_iter()
        .map(table_alignment)
        .collect::<Vec<_>>();
    Ok(Block::Table {
        rows: None,
        cells: rows,
        alignments: if alignments
            .iter()
            .all(|alignment| *alignment == TableAlignment::None)
        {
            Vec::new()
        } else {
            alignments
        },
    })
}

fn parse_table_row<'a>(
    events: &[(Event<'a>, Range<usize>)],
    cursor: &mut usize,
    stop: TagEnd,
    base_dir: &Path,
    placeholders: &HashMap<String, ProtectedInline>,
) -> Result<Vec<Vec<Inline>>, MarkoffError> {
    let mut cells = Vec::new();
    while let Some((event, _)) = events.get(*cursor) {
        match event {
            Event::End(end) if *end == stop => {
                *cursor += 1;
                break;
            }
            Event::Start(Tag::TableCell) => {
                *cursor += 1;
                cells.push(parse_inline_events(
                    events,
                    cursor,
                    Some(TagEnd::TableCell),
                    base_dir,
                    placeholders,
                )?);
            }
            _ => *cursor += 1,
        }
    }
    Ok(cells)
}

fn table_alignment(alignment: Alignment) -> TableAlignment {
    match alignment {
        Alignment::None => TableAlignment::None,
        Alignment::Left => TableAlignment::Left,
        Alignment::Center => TableAlignment::Center,
        Alignment::Right => TableAlignment::Right,
    }
}

fn parse_html_block<'a>(events: &[(Event<'a>, Range<usize>)], cursor: &mut usize) -> Block {
    let mut html = String::new();
    while let Some((event, _)) = events.get(*cursor) {
        let event = event.clone();
        *cursor += 1;
        match event {
            Event::End(TagEnd::HtmlBlock) => break,
            Event::Html(value) | Event::InlineHtml(value) | Event::Text(value) => {
                html.push_str(&value);
            }
            _ => {}
        }
    }
    Block::Html { html }
}
