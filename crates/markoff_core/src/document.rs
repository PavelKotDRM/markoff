use crate::Format;
use crate::MarkoffError;
use crate::document_model::{Block, Document, Inline, ListItem, TableAlignment};
use crate::docx_inline::markdown_list_item;
use crate::error::invalid_data;
use crate::html_tokenizer::{Token, tokenize};
use crate::tables::markdown_table_from_rows;
use crate::xml_utils::{MarkdownEscapeContext, markdown_escape, xml_attribute_escape};
use base64::Engine as _;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

pub(crate) fn convert_markdown_to_document(
    input: &Path,
    output: &Path,
    format: Format,
    tables_only: bool,
) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let base_dir = parent_directory(input);
    let mut document = markdown_to_document(&source, base_dir)?;
    if tables_only {
        retain_table_blocks(&mut document);
    }
    let rendered = render_document(&document, format)?;
    std::fs::write(output, rendered)?;
    Ok(())
}

pub(crate) fn convert_document_to_markdown(
    input: &Path,
    output: &Path,
    format: Format,
    tables_only: bool,
) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let mut document = parse_document(&source, format)?;
    if tables_only {
        retain_table_blocks(&mut document);
    }
    std::fs::write(
        output,
        document_to_markdown(&document, parent_directory(output))?,
    )?;
    Ok(())
}

pub(crate) fn convert_structured_data_format(
    input: &Path,
    output: &Path,
    from: Format,
    to: Format,
) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let value = parse_structured_value(&source, from)?;
    let rendered = match to {
        Format::Json => serde_json::to_string_pretty(&value).map_err(invalid_data)?,
        Format::Yaml => serde_yaml::to_string(&value).map_err(invalid_data)?,
        Format::Toml => toml::to_string_pretty(&value).map_err(invalid_data)?,
        _ => unreachable!("only JSON, YAML, and TOML use this helper"),
    };
    std::fs::write(output, rendered)?;
    Ok(())
}

fn parse_structured_value(source: &str, format: Format) -> Result<serde_json::Value, MarkoffError> {
    match format {
        Format::Json => Ok(serde_json::from_str(source).map_err(invalid_data)?),
        Format::Yaml => Ok(serde_yaml::from_str(source).map_err(invalid_data)?),
        Format::Toml => Ok(toml::from_str(source).map_err(invalid_data)?),
        _ => unreachable!("only JSON, YAML, and TOML use this helper"),
    }
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn retain_table_blocks(document: &mut Document) {
    let mut retained = Vec::new();
    for mut block in std::mem::take(&mut document.blocks) {
        let keep = match &mut block {
            Block::Table { .. } => true,
            Block::List { items, .. } => {
                for item in items.iter_mut() {
                    let mut nested = Document {
                        blocks: std::mem::take(&mut item.blocks),
                    };
                    retain_table_blocks(&mut nested);
                    item.blocks = nested.blocks;
                }
                items.retain(|item| !item.blocks.is_empty());
                !items.is_empty()
            }
            Block::Quote { blocks, .. } | Block::FootnoteDefinition { blocks, .. } => {
                let mut nested = Document {
                    blocks: std::mem::take(blocks),
                };
                retain_table_blocks(&mut nested);
                *blocks = nested.blocks;
                !blocks.is_empty()
            }
            _ => false,
        };
        if keep {
            retained.push(block);
        }
    }
    document.blocks = retained;
}

fn markdown_to_document(markdown: &str, base_dir: &Path) -> Result<Document, MarkoffError> {
    let (source, placeholders) = protect_inline_syntax(markdown, false);
    let options = markdown_options();
    let events = Parser::new_ext(&source, options)
        .into_offset_iter()
        .collect::<Vec<_>>();
    let mut cursor = 0;
    Ok(Document {
        blocks: parse_blocks(
            &events,
            &mut cursor,
            None,
            &source,
            markdown,
            base_dir,
            &placeholders,
        )?,
    })
}

fn markdown_options() -> Options {
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
    offset_source: &str,
    original_source: &str,
    base_dir: &Path,
    placeholders: &HashMap<String, ProtectedInline>,
) -> Result<Vec<Block>, MarkoffError> {
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
                        base_dir,
                        placeholders,
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
                        base_dir,
                        placeholders,
                    )?,
                }),
                Tag::BlockQuote(_) => blocks.push(Block::Quote {
                    text: None,
                    blocks: parse_blocks(
                        events,
                        cursor,
                        Some(TagEnd::BlockQuote(None)),
                        offset_source,
                        original_source,
                        base_dir,
                        placeholders,
                    )?,
                }),
                Tag::List(start) => blocks.push(parse_list(
                    events,
                    cursor,
                    start,
                    offset_source,
                    original_source,
                    base_dir,
                    placeholders,
                )?),
                Tag::CodeBlock(kind) => {
                    blocks.push(parse_code_block(events, cursor, kind));
                }
                Tag::FootnoteDefinition(label) => blocks.push(Block::FootnoteDefinition {
                    label: label.to_string(),
                    blocks: parse_blocks(
                        events,
                        cursor,
                        Some(TagEnd::FootnoteDefinition),
                        offset_source,
                        original_source,
                        base_dir,
                        placeholders,
                    )?,
                }),
                Tag::Table(alignments) => blocks.push(parse_table(
                    events,
                    cursor,
                    alignments,
                    base_dir,
                    placeholders,
                )?),
                Tag::HtmlBlock => blocks.push(parse_html_block(events, cursor)),
                _ => {}
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
        let _ = (range, offset_source, original_source);
    }
    Ok(blocks)
}

fn parse_list<'a>(
    events: &[(Event<'a>, Range<usize>)],
    cursor: &mut usize,
    start: Option<u64>,
    offset_source: &str,
    original_source: &str,
    base_dir: &Path,
    placeholders: &HashMap<String, ProtectedInline>,
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
                let blocks = parse_blocks(
                    events,
                    cursor,
                    Some(TagEnd::Item),
                    offset_source,
                    original_source,
                    base_dir,
                    placeholders,
                )?;
                let number = ordered
                    .then(|| list_item_number(offset_source, original_source, item_offset))
                    .flatten();
                let task_checked =
                    list_item_task_marker(offset_source, original_source, item_offset);
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

#[derive(Clone)]
enum ProtectedInline {
    InlineFootnote(String),
    FootnoteReference(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineKind {
    Emphasis,
    Strong,
    Strikethrough,
    Underline,
    Superscript,
    Subscript,
    Code,
    Link,
    Image,
}

enum InlineContainer {
    Emphasis,
    Strong,
    Strikethrough,
    Underline,
    Superscript,
    Subscript,
    Code,
    Link {
        destination: String,
        title: Option<String>,
    },
    Image {
        destination: String,
        title: Option<String>,
    },
}

impl InlineContainer {
    fn kind(&self) -> InlineKind {
        match self {
            Self::Emphasis => InlineKind::Emphasis,
            Self::Strong => InlineKind::Strong,
            Self::Strikethrough => InlineKind::Strikethrough,
            Self::Underline => InlineKind::Underline,
            Self::Superscript => InlineKind::Superscript,
            Self::Subscript => InlineKind::Subscript,
            Self::Code => InlineKind::Code,
            Self::Link { .. } => InlineKind::Link,
            Self::Image { .. } => InlineKind::Image,
        }
    }
}

struct InlineFrame {
    container: Option<InlineContainer>,
    content: Vec<Inline>,
}

fn parse_inline_events<'a>(
    events: &[(Event<'a>, Range<usize>)],
    cursor: &mut usize,
    stop: Option<TagEnd>,
    base_dir: &Path,
    placeholders: &HashMap<String, ProtectedInline>,
) -> Result<Vec<Inline>, MarkoffError> {
    let mut frames = vec![InlineFrame {
        container: None,
        content: Vec::new(),
    }];
    let mut bookmark_closings = 0usize;
    while let Some((event, _)) = events.get(*cursor) {
        let event = event.clone();
        *cursor += 1;
        match event {
            Event::End(end) if stop == Some(end) => break,
            Event::Start(Tag::Emphasis) => {
                push_inline_frame(&mut frames, InlineContainer::Emphasis)
            }
            Event::Start(Tag::Strong) => push_inline_frame(&mut frames, InlineContainer::Strong),
            Event::Start(Tag::Strikethrough) => {
                push_inline_frame(&mut frames, InlineContainer::Strikethrough)
            }
            Event::Start(Tag::Superscript) => {
                push_inline_frame(&mut frames, InlineContainer::Superscript)
            }
            Event::Start(Tag::Subscript) => {
                push_inline_frame(&mut frames, InlineContainer::Subscript)
            }
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => push_inline_frame(
                &mut frames,
                InlineContainer::Link {
                    destination: dest_url.to_string(),
                    title: (!title.is_empty()).then(|| title.to_string()),
                },
            ),
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => push_inline_frame(
                &mut frames,
                InlineContainer::Image {
                    destination: dest_url.to_string(),
                    title: (!title.is_empty()).then(|| title.to_string()),
                },
            ),
            Event::End(end) => {
                if let Some(kind) = inline_kind_for_end(end) {
                    close_inline_frame(&mut frames, kind, base_dir)?;
                }
            }
            Event::Text(text) => {
                append_text_with_placeholders(&mut frames, text.as_ref(), placeholders, base_dir)?
            }
            Event::Code(text) => push_inline(
                &mut frames,
                Inline::Code {
                    text: text.to_string(),
                },
            ),
            Event::InlineMath(text) => {
                let text = text.as_ref();
                if let Some(value) = text
                    .strip_prefix("^{")
                    .and_then(|text| text.strip_suffix('}'))
                {
                    push_inline(
                        &mut frames,
                        Inline::Superscript {
                            content: parse_inline_fragment(value, base_dir)?,
                        },
                    );
                } else if let Some(value) = text
                    .strip_prefix("_{")
                    .and_then(|text| text.strip_suffix('}'))
                {
                    push_inline(
                        &mut frames,
                        Inline::Subscript {
                            content: parse_inline_fragment(value, base_dir)?,
                        },
                    );
                } else {
                    push_inline(
                        &mut frames,
                        Inline::Math {
                            text: text.to_string(),
                            display: false,
                        },
                    );
                }
            }
            Event::DisplayMath(text) => push_inline(
                &mut frames,
                Inline::Math {
                    text: text.to_string(),
                    display: true,
                },
            ),
            Event::FootnoteReference(label) => push_inline(
                &mut frames,
                Inline::FootnoteReference {
                    label: label.to_string(),
                },
            ),
            Event::SoftBreak => push_inline(&mut frames, Inline::SoftBreak),
            Event::HardBreak => push_inline(&mut frames, Inline::HardBreak),
            Event::TaskListMarker(checked) => {
                push_inline(&mut frames, Inline::TaskListMarker { checked });
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                parse_inline_html(&mut frames, html.as_ref(), base_dir, &mut bookmark_closings)?;
            }
            Event::Rule => push_inline(
                &mut frames,
                Inline::Html {
                    html: "<hr>".to_string(),
                },
            ),
            Event::Start(_) => {}
        }
    }
    while frames.len() > 1 {
        let kind = frames
            .last()
            .and_then(|frame| frame.container.as_ref())
            .map(InlineContainer::kind);
        if let Some(kind) = kind {
            close_inline_frame(&mut frames, kind, base_dir)?;
        }
    }
    Ok(frames.pop().unwrap().content)
}

fn inline_kind_for_end(end: TagEnd) -> Option<InlineKind> {
    match end {
        TagEnd::Emphasis => Some(InlineKind::Emphasis),
        TagEnd::Strong => Some(InlineKind::Strong),
        TagEnd::Strikethrough => Some(InlineKind::Strikethrough),
        TagEnd::Superscript => Some(InlineKind::Superscript),
        TagEnd::Subscript => Some(InlineKind::Subscript),
        TagEnd::Link => Some(InlineKind::Link),
        TagEnd::Image => Some(InlineKind::Image),
        _ => None,
    }
}

fn push_inline_frame(frames: &mut Vec<InlineFrame>, container: InlineContainer) {
    frames.push(InlineFrame {
        container: Some(container),
        content: Vec::new(),
    });
}

fn close_inline_frame(
    frames: &mut Vec<InlineFrame>,
    kind: InlineKind,
    base_dir: &Path,
) -> Result<bool, MarkoffError> {
    let matches = frames
        .last()
        .and_then(|frame| frame.container.as_ref())
        .is_some_and(|container| container.kind() == kind);
    if !matches || frames.len() < 2 {
        return Ok(false);
    }
    let frame = frames.pop().unwrap();
    let content = frame.content;
    let inline = match frame.container.unwrap() {
        InlineContainer::Emphasis => Inline::Emphasis { content },
        InlineContainer::Strong => Inline::Strong { content },
        InlineContainer::Strikethrough => Inline::Strikethrough { content },
        InlineContainer::Underline => Inline::Underline { content },
        InlineContainer::Superscript => Inline::Superscript { content },
        InlineContainer::Subscript => Inline::Subscript { content },
        InlineContainer::Code => Inline::Code {
            text: inline_plain_text(&content),
        },
        InlineContainer::Link { destination, title } => Inline::Link {
            destination,
            title,
            content,
        },
        InlineContainer::Image { destination, title } => {
            let (format, data) = load_image_asset(&destination, base_dir)?;
            Inline::Image {
                alt: inline_plain_text(&content),
                destination,
                title,
                format,
                data,
            }
        }
    };
    push_inline(frames, inline);
    Ok(true)
}

fn push_inline(frames: &mut [InlineFrame], inline: Inline) {
    let content = &mut frames.last_mut().unwrap().content;
    match inline {
        Inline::Text { text } => {
            if let Some(Inline::Text {
                text: previous_text,
            }) = content.last_mut()
            {
                previous_text.push_str(&text);
            } else {
                content.push(Inline::Text { text });
            }
        }
        inline => content.push(inline),
    }
}

fn append_text_with_placeholders(
    frames: &mut [InlineFrame],
    text: &str,
    placeholders: &HashMap<String, ProtectedInline>,
    base_dir: &Path,
) -> Result<(), MarkoffError> {
    let mut remaining = text;
    while !remaining.is_empty() {
        let next = placeholders
            .iter()
            .filter_map(|(token, placeholder)| {
                remaining
                    .find(token)
                    .map(|position| (position, token.as_str(), placeholder))
            })
            .min_by_key(|(position, _, _)| *position);
        let Some((position, token, placeholder)) = next else {
            push_inline(
                frames,
                Inline::Text {
                    text: remaining.to_string(),
                },
            );
            break;
        };
        if position > 0 {
            push_inline(
                frames,
                Inline::Text {
                    text: remaining[..position].to_string(),
                },
            );
        }
        match placeholder {
            ProtectedInline::InlineFootnote(content) => push_inline(
                frames,
                Inline::Footnote {
                    content: parse_inline_fragment(content, base_dir)?,
                },
            ),
            ProtectedInline::FootnoteReference(label) => {
                push_inline(
                    frames,
                    Inline::FootnoteReference {
                        label: label.clone(),
                    },
                );
            }
        }
        remaining = &remaining[position + token.len()..];
    }
    Ok(())
}

fn parse_inline_fragment(source: &str, base_dir: &Path) -> Result<Vec<Inline>, MarkoffError> {
    let (source, placeholders) = protect_inline_syntax(source, true);
    let events = Parser::new_ext(&source, markdown_options())
        .into_offset_iter()
        .collect::<Vec<_>>();
    let mut cursor = 0;
    parse_inline_events(&events, &mut cursor, None, base_dir, &placeholders)
}

fn protect_inline_syntax(
    source: &str,
    protect_references: bool,
) -> (String, HashMap<String, ProtectedInline>) {
    let mut prefix = "MARKOFFINLINEPLACEHOLDER".to_string();
    while source.contains(&prefix) {
        prefix.push('_');
    }
    let mut protected = String::with_capacity(source.len());
    let mut placeholders = HashMap::new();
    let mut cursor = 0;
    let mut code_fence: Option<(u8, usize)> = None;
    while cursor < source.len() {
        let remaining = &source[cursor..];
        if remaining.starts_with('\\') {
            let escaped_length = remaining
                .get(1..)
                .and_then(|value| value.chars().next())
                .map_or(1, |character| 1 + character.len_utf8());
            protected.push_str(&remaining[..escaped_length]);
            cursor += escaped_length;
            continue;
        }
        if remaining.starts_with('`') || remaining.starts_with('~') {
            let marker = remaining.as_bytes()[0];
            let run = remaining.bytes().take_while(|byte| *byte == marker).count();
            if marker == b'`' || run >= 3 {
                if let Some((active_marker, active_run)) = code_fence {
                    if active_marker == marker && active_run == run {
                        code_fence = None;
                    }
                } else {
                    code_fence = Some((marker, run));
                }
                protected.push_str(&remaining[..run]);
                cursor += run;
                continue;
            }
        }
        if code_fence.is_none()
            && remaining.starts_with("^[")
            && let Some(end) = closing_square_bracket(&remaining[2..])
            && !remaining[2..2 + end].contains('\n')
        {
            let token = format!("{prefix}{}END", placeholders.len());
            let content = remaining[2..2 + end].to_string();
            protected.push_str(&token);
            placeholders.insert(token, ProtectedInline::InlineFootnote(content));
            cursor += 3 + end;
            continue;
        }
        if protect_references
            && code_fence.is_none()
            && remaining.starts_with("[^")
            && let Some(end) = closing_square_bracket(&remaining[2..])
        {
            let label = &remaining[2..2 + end];
            if !label.is_empty() {
                let token = format!("{prefix}{}END", placeholders.len());
                protected.push_str(&token);
                placeholders.insert(token, ProtectedInline::FootnoteReference(label.to_string()));
                cursor += 3 + end;
                continue;
            }
        }
        let character = remaining.chars().next().unwrap();
        protected.push(character);
        cursor += character.len_utf8();
    }
    (protected, placeholders)
}

pub(crate) fn expand_inline_footnotes(markdown: &str) -> String {
    let (mut expanded, placeholders) = protect_inline_syntax(markdown, false);
    let mut inline_footnotes = placeholders
        .iter()
        .filter_map(|(token, placeholder)| match placeholder {
            ProtectedInline::InlineFootnote(content) => expanded
                .find(token)
                .map(|position| (position, token.clone(), content.clone())),
            ProtectedInline::FootnoteReference(_) => None,
        })
        .collect::<Vec<_>>();
    inline_footnotes.sort_by_key(|(position, _, _)| *position);
    if inline_footnotes.is_empty() {
        return expanded;
    }

    let mut label_prefix = "markoff-inline".to_string();
    while markdown.contains(&format!("[^{label_prefix}")) {
        label_prefix.push('_');
    }
    let mut definitions = Vec::with_capacity(inline_footnotes.len());
    for (index, (_, token, content)) in inline_footnotes.into_iter().enumerate() {
        let label = format!("{label_prefix}-{index}");
        expanded = expanded.replacen(&token, &format!("[^{label}]"), 1);
        definitions.push(format!("[^{label}]: {content}"));
    }
    expanded = expanded.trim_end_matches('\n').to_string();
    expanded.push_str("\n\n");
    expanded.push_str(&definitions.join("\n\n"));
    expanded.push('\n');
    expanded
}

fn closing_square_bracket(source: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (index, character) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' if depth == 0 => return Some(index),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn parse_inline_html(
    frames: &mut Vec<InlineFrame>,
    html: &str,
    base_dir: &Path,
    bookmark_closings: &mut usize,
) -> Result<(), MarkoffError> {
    let tokens = tokenize(html);
    let Some(token) = tokens.first() else {
        push_inline(
            frames,
            Inline::Html {
                html: html.to_string(),
            },
        );
        return Ok(());
    };
    match token {
        Token::Start(name, attributes, self_closing) => {
            let attribute = |key: &str| {
                attributes
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value.clone())
            };
            if *self_closing {
                match name.as_str() {
                    "br" => push_inline(frames, Inline::HardBreak),
                    "img" => {
                        let destination = attribute("src").unwrap_or_default();
                        let (format, data) = load_image_asset(&destination, base_dir)?;
                        push_inline(
                            frames,
                            Inline::Image {
                                alt: attribute("alt").unwrap_or_default(),
                                destination,
                                title: attribute("title"),
                                format,
                                data,
                            },
                        );
                    }
                    _ => push_inline(
                        frames,
                        Inline::Html {
                            html: html.to_string(),
                        },
                    ),
                }
                return Ok(());
            }
            match name.as_str() {
                "u" => push_inline_frame(frames, InlineContainer::Underline),
                "sup" => push_inline_frame(frames, InlineContainer::Superscript),
                "sub" => push_inline_frame(frames, InlineContainer::Subscript),
                "strong" | "b" => push_inline_frame(frames, InlineContainer::Strong),
                "em" | "i" => push_inline_frame(frames, InlineContainer::Emphasis),
                "s" | "del" => push_inline_frame(frames, InlineContainer::Strikethrough),
                "code" => push_inline_frame(frames, InlineContainer::Code),
                "a" => {
                    let bookmark = attribute("id").or_else(|| attribute("name"));
                    if let Some(destination) = attribute("href") {
                        push_inline_frame(
                            frames,
                            InlineContainer::Link {
                                destination,
                                title: attribute("title"),
                            },
                        );
                        if let Some(name) = bookmark {
                            push_inline(
                                frames,
                                Inline::Bookmark {
                                    name: name.to_string(),
                                },
                            );
                        }
                    } else if let Some(name) = bookmark {
                        push_inline(
                            frames,
                            Inline::Bookmark {
                                name: name.to_string(),
                            },
                        );
                        *bookmark_closings += 1;
                    } else {
                        push_inline(
                            frames,
                            Inline::Html {
                                html: html.to_string(),
                            },
                        );
                    }
                }
                _ => push_inline(
                    frames,
                    Inline::Html {
                        html: html.to_string(),
                    },
                ),
            }
        }
        Token::End(name) => {
            let kind = match name.as_str() {
                "u" => Some(InlineKind::Underline),
                "sup" => Some(InlineKind::Superscript),
                "sub" => Some(InlineKind::Subscript),
                "strong" | "b" => Some(InlineKind::Strong),
                "em" | "i" => Some(InlineKind::Emphasis),
                "s" | "del" => Some(InlineKind::Strikethrough),
                "code" => Some(InlineKind::Code),
                "a" if *bookmark_closings > 0 => {
                    *bookmark_closings -= 1;
                    None
                }
                "a" => Some(InlineKind::Link),
                _ => None,
            };
            if let Some(kind) = kind
                && !close_inline_frame(frames, kind, base_dir)?
            {
                push_inline(
                    frames,
                    Inline::Html {
                        html: html.to_string(),
                    },
                );
            } else if kind.is_none() && name != "a" {
                push_inline(
                    frames,
                    Inline::Html {
                        html: html.to_string(),
                    },
                );
            }
        }
        Token::Text(_) => push_inline(
            frames,
            Inline::Html {
                html: html.to_string(),
            },
        ),
    }
    Ok(())
}

fn load_image_asset(
    destination: &str,
    base_dir: &Path,
) -> Result<(Option<String>, Option<String>), MarkoffError> {
    if let Some(data_uri) = destination.strip_prefix("data:image/")
        && let Some((media_type, data)) = data_uri.split_once(",")
        && media_type.ends_with(";base64")
    {
        let format = match media_type
            .strip_suffix(";base64")
            .unwrap_or(media_type)
            .to_ascii_lowercase()
            .as_str()
        {
            "image/jpeg" => Some("jpg".to_string()),
            "image/svg+xml" => Some("svg".to_string()),
            "image/png" => Some("png".to_string()),
            "image/gif" => Some("gif".to_string()),
            "image/webp" => Some("webp".to_string()),
            "image/bmp" => Some("bmp".to_string()),
            _ => None,
        };
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(invalid_data)?;
        return Ok((
            format,
            Some(base64::engine::general_purpose::STANDARD.encode(decoded)),
        ));
    }

    let path = destination.split(['?', '#']).next().unwrap_or(destination);
    if path.is_empty() || path.contains("://") || path.starts_with('#') {
        return Ok((None, None));
    }
    let image_path = base_dir.join(path);
    match std::fs::read(&image_path) {
        Ok(bytes) => {
            let format = Path::new(path)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("png")
                .to_ascii_lowercase();
            Ok((
                Some(format),
                Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((None, None)),
        Err(error) => Err(error.into()),
    }
}

fn inline_plain_text(content: &[Inline]) -> String {
    let mut text = String::new();
    for inline in content {
        match inline {
            Inline::Text { text: value }
            | Inline::Code { text: value }
            | Inline::Math { text: value, .. } => {
                text.push_str(value);
            }
            Inline::Emphasis { content }
            | Inline::Strong { content }
            | Inline::Strikethrough { content }
            | Inline::Underline { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Footnote { content }
            | Inline::Link { content, .. } => text.push_str(&inline_plain_text(content)),
            Inline::Image { alt, .. } => text.push_str(alt),
            Inline::FootnoteReference { label } => {
                text.push_str("[^");
                text.push_str(label);
                text.push(']');
            }
            Inline::Bookmark { .. } | Inline::TaskListMarker { .. } | Inline::Html { .. } => {}
            Inline::SoftBreak | Inline::HardBreak => text.push(' '),
        }
    }
    text
}

fn document_to_markdown(document: &Document, base_dir: &Path) -> Result<String, MarkoffError> {
    let mut image_index = 0usize;
    let mut rendered_blocks = Vec::with_capacity(document.blocks.len());
    for block in &document.blocks {
        rendered_blocks.push(render_block(block, base_dir, &mut image_index)?);
    }
    Ok(format!("{}\n", rendered_blocks.join("\n\n")))
}

fn render_document(document: &Document, format: Format) -> Result<String, MarkoffError> {
    match format {
        Format::Json => Ok(serde_json::to_string_pretty(document).map_err(invalid_data)?),
        Format::Yaml => Ok(serde_yaml::to_string(document).map_err(invalid_data)?),
        Format::Toml => Ok(toml::to_string_pretty(document).map_err(invalid_data)?),
        _ => unreachable!("only JSON, YAML, and TOML render a document"),
    }
}

fn parse_document(source: &str, format: Format) -> Result<Document, MarkoffError> {
    match format {
        Format::Json => Ok(serde_json::from_str(source).map_err(invalid_data)?),
        Format::Yaml => Ok(serde_yaml::from_str(source).map_err(invalid_data)?),
        Format::Toml => Ok(toml::from_str(source).map_err(invalid_data)?),
        _ => unreachable!("only JSON, YAML, and TOML parse into a document"),
    }
}

fn render_block(
    block: &Block,
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    match block {
        Block::Heading {
            level,
            text,
            content,
        } => Ok(format!(
            "{} {}",
            "#".repeat((*level).clamp(1, 6) as usize),
            render_content(content, text.as_deref(), base_dir, image_index)?
        )),
        Block::ListItem {
            ordered,
            level,
            number,
            text,
            content,
        } => {
            let indentation = "    ".repeat(*level);
            let marker = if *ordered {
                format!("{}. ", number.unwrap_or(1))
            } else {
                "- ".to_string()
            };
            Ok(format!(
                "{indentation}{marker}{}",
                render_content(content, text.as_deref(), base_dir, image_index)?
            ))
        }
        Block::List {
            ordered,
            start,
            items,
        } => render_list(*ordered, *start, items, base_dir, image_index, ""),
        Block::Table {
            rows,
            cells,
            alignments,
        } => render_table(rows.as_deref(), cells, alignments, base_dir, image_index),
        Block::Code { code, info } => {
            let fence = code_fence(code, 2);
            let info = info.as_deref().unwrap_or_default();
            Ok(format!("{fence}{info}\n{code}\n{fence}"))
        }
        Block::Math { text } => Ok(format!("$$\n{text}\n$$")),
        Block::Quote { text, blocks } => {
            let quoted = if blocks.is_empty() {
                text.clone().unwrap_or_default()
            } else {
                render_blocks(blocks, base_dir, image_index)?.join("\n\n")
            };
            Ok(quoted
                .lines()
                .map(|line| {
                    if line.is_empty() {
                        ">".to_string()
                    } else {
                        format!("> {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Block::HorizontalRule => Ok("---".to_string()),
        Block::Image { alt, format, data } => {
            let destination = write_image(data, format, base_dir, image_index)?;
            Ok(render_image_link(alt, &destination, None))
        }
        Block::Paragraph { text, content } => {
            render_content(content, text.as_deref(), base_dir, image_index)
        }
        Block::FootnoteDefinition { label, blocks } => {
            let rendered = render_blocks(blocks, base_dir, image_index)?;
            let body = rendered.join("\n\n");
            if body.is_empty() {
                return Ok(format!("[^{label}]:"));
            }
            let first_is_paragraph = matches!(blocks.first(), Some(Block::Paragraph { .. }));
            if first_is_paragraph {
                let mut lines = body.lines();
                let first_line = lines.next().unwrap_or_default();
                let mut output = format!("[^{label}]: {first_line}");
                for line in lines {
                    if line.is_empty() {
                        output.push('\n');
                    } else {
                        output.push_str("\n    ");
                        output.push_str(line);
                    }
                }
                Ok(output)
            } else {
                Ok(format!("[^{label}]:\n{}", indent_lines(&body, "    ")))
            }
        }
        Block::Html { html } => Ok(html.clone()),
    }
}

fn render_blocks(
    blocks: &[Block],
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<Vec<String>, MarkoffError> {
    blocks
        .iter()
        .map(|block| render_block(block, base_dir, image_index))
        .collect()
}

fn render_content(
    content: &[Inline],
    legacy_text: Option<&str>,
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    if content.is_empty() {
        return Ok(legacy_text.unwrap_or_default().to_string());
    }
    content
        .iter()
        .map(|inline| render_inline(inline, base_dir, image_index))
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.concat())
}

fn render_inline(
    inline: &Inline,
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    match inline {
        Inline::Text { text } => Ok(escape_plain_text(text)),
        Inline::Emphasis { content } => Ok(format!(
            "*{}*",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Strong { content } => Ok(format!(
            "**{}**",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Strikethrough { content } => Ok(format!(
            "~~{}~~",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Underline { content } => Ok(format!(
            "<u>{}</u>",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Superscript { content } => Ok(format!(
            "$^{{{}}}$",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Subscript { content } => Ok(format!(
            "$_{{{}}}$",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Code { text } => {
            let fence = code_fence(text, 0);
            let padding = !text.is_empty()
                && (text.starts_with(' ')
                    && text.ends_with(' ')
                    && text.chars().any(|character| character != ' ')
                    || text.starts_with('`')
                    || text.ends_with('`'));
            let padding = if padding { " " } else { "" };
            Ok(format!("{fence}{padding}{text}{padding}{fence}"))
        }
        Inline::Math { text, display } => {
            if *display {
                Ok(format!("$${text}$$"))
            } else {
                Ok(format!("${text}$"))
            }
        }
        Inline::Link {
            destination,
            title,
            content,
        } => Ok(render_link(
            &render_content(content, None, base_dir, image_index)?,
            destination,
            title.as_deref(),
        )),
        Inline::Image {
            alt,
            destination,
            title,
            format,
            data,
        } => {
            let destination = if let (Some(format), Some(data)) = (format, data) {
                write_image(data, format, base_dir, image_index)?
            } else {
                destination.clone()
            };
            Ok(render_image_link(alt, &destination, title.as_deref()))
        }
        Inline::FootnoteReference { label } => Ok(format!("[^{label}]")),
        Inline::Footnote { content } => Ok(format!(
            "^[{}]",
            render_content(content, None, base_dir, image_index)?
        )),
        Inline::Bookmark { name } => Ok(format!("<a id=\"{}\"></a>", xml_attribute_escape(name))),
        Inline::SoftBreak => Ok(" ".to_string()),
        Inline::HardBreak => Ok("  \n".to_string()),
        Inline::TaskListMarker { checked } => {
            Ok(if *checked { "[x] " } else { "[ ] " }.to_string())
        }
        Inline::Html { html } => Ok(html.clone()),
    }
}

fn escape_plain_text(text: &str) -> String {
    markdown_escape(text, MarkdownEscapeContext::Docx)
        .replace('_', "\\_")
        .replace('>', "\\>")
}

fn code_fence(text: &str, minimum: usize) -> String {
    let mut longest = 0;
    let mut current = 0;
    for character in text.chars() {
        if character == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    "`".repeat(longest.max(minimum) + 1)
}

fn render_link(label: &str, destination: &str, title: Option<&str>) -> String {
    let destination = if destination
        .chars()
        .any(|character| character.is_whitespace() || character == '<' || character == '>')
    {
        format!("<{}>", destination.replace('>', "\\>"))
    } else {
        destination.replace('\\', "\\\\").replace(')', "\\)")
    };
    let title = title.map_or_else(String::new, |title| {
        format!(" \"{}\"", title.replace('\\', "\\\\").replace('"', "\\\""))
    });
    format!("[{label}]({destination}{title})")
}

fn render_image_link(alt: &str, destination: &str, title: Option<&str>) -> String {
    let link = render_link(&escape_plain_text(alt), destination, title);
    format!("!{}", link)
}

fn write_image(
    data: &str,
    format: &str,
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    *image_index += 1;
    let file_name = format!("image{image_index}.{format}");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(invalid_data)?;
    let image_dir = base_dir.join("image");
    std::fs::create_dir_all(&image_dir)?;
    std::fs::write(image_dir.join(&file_name), bytes)?;
    Ok(format!("image/{file_name}"))
}

fn render_table(
    legacy_rows: Option<&[Vec<String>]>,
    cells: &[Vec<Vec<Inline>>],
    alignments: &[TableAlignment],
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    if cells.is_empty() {
        return Ok(legacy_rows.map_or_else(String::new, markdown_table_from_rows));
    }
    let rows = cells
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| render_content(cell, None, base_dir, image_index))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut rendered = markdown_table_from_rows(&rows);
    if alignments
        .iter()
        .any(|alignment| *alignment != TableAlignment::None)
    {
        let mut lines = rendered.lines().map(str::to_string).collect::<Vec<_>>();
        if let Some(header) = rows.first() {
            let separator = header
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    match alignments
                        .get(index)
                        .copied()
                        .unwrap_or(TableAlignment::None)
                    {
                        TableAlignment::None => "---".to_string(),
                        TableAlignment::Left => ":---".to_string(),
                        TableAlignment::Center => ":---:".to_string(),
                        TableAlignment::Right => "---:".to_string(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" | ");
            if lines.len() > 1 {
                lines[1] = format!("| {separator} |");
            }
        }
        rendered = lines.join("\n");
    }
    Ok(rendered)
}

fn render_list(
    ordered: bool,
    start: Option<u64>,
    items: &[ListItem],
    base_dir: &Path,
    image_index: &mut usize,
    indent: &str,
) -> Result<String, MarkoffError> {
    let mut next_number = start.unwrap_or(1);
    let mut rendered_items = Vec::with_capacity(items.len());
    for item in items {
        let number = item.number.unwrap_or(next_number);
        let marker = if ordered {
            format!("{number}. ")
        } else {
            "- ".to_string()
        };
        next_number = number.saturating_add(1);
        let item_prefix = format!("{indent}{marker}");
        let continuation_indent = " ".repeat(item_prefix.len());
        let nested_indent = format!("{indent}    ");
        let mut item_blocks = item.blocks.iter();
        let mut rendered = String::new();
        if let Some(first) = item_blocks.next() {
            if matches!(first, Block::Paragraph { .. }) {
                let paragraph = render_block(first, base_dir, image_index)?;
                rendered.push_str(&indent_lines_with_first(
                    &paragraph,
                    &item_prefix,
                    &continuation_indent,
                ));
            } else {
                rendered.push_str(item_prefix.trim_end());
                let child = indent_block(first, &nested_indent, base_dir, image_index)?;
                if !child.is_empty() {
                    rendered.push('\n');
                    rendered.push_str(&child);
                }
            }
        } else {
            rendered.push_str(item_prefix.trim_end());
        }
        for block in item_blocks {
            let child = match block {
                Block::List {
                    ordered,
                    start,
                    items,
                } => render_list(
                    *ordered,
                    *start,
                    items,
                    base_dir,
                    image_index,
                    &nested_indent,
                )?,
                _ => indent_block(block, &nested_indent, base_dir, image_index)?,
            };
            if !child.is_empty() {
                if !rendered.is_empty() {
                    rendered.push_str(if matches!(block, Block::List { .. }) {
                        "\n"
                    } else {
                        "\n\n"
                    });
                }
                rendered.push_str(&child);
            }
        }
        rendered_items.push(rendered);
    }
    Ok(rendered_items.join("\n"))
}

fn indent_block(
    block: &Block,
    indent: &str,
    base_dir: &Path,
    image_index: &mut usize,
) -> Result<String, MarkoffError> {
    let markdown = render_block(block, base_dir, image_index)?;
    Ok(indent_lines(&markdown, indent))
}

fn indent_lines(markdown: &str, indent: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn indent_lines_with_first(markdown: &str, first: &str, continuation: &str) -> String {
    let mut lines = markdown.lines();
    let Some(first_line) = lines.next() else {
        return first.trim_end().to_string();
    };
    let mut rendered = format!("{first}{first_line}");
    for line in lines {
        rendered.push('\n');
        if !line.is_empty() {
            rendered.push_str(continuation);
            rendered.push_str(line);
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::{document_to_markdown, markdown_to_document, parse_document, render_document};
    use crate::{
        Format,
        document_model::{Block, Inline},
    };
    use std::path::Path;

    #[test]
    fn parses_and_renders_block_and_inline_semantics() {
        let markdown = "# Title\n\nA **bold** and *italic* paragraph with [link](https://example.com).\n\n- [x] First\n- Second\n\n| Name | Score |\n| :--- | ---: |\n| Ada | 42 |\n\n[^note]: A footnote.\n";
        let document = markdown_to_document(markdown, Path::new(".")).unwrap();
        assert!(matches!(document.blocks[0], Block::Heading { .. }));
        assert!(matches!(document.blocks[1], Block::Paragraph { .. }));
        assert!(matches!(document.blocks[2], Block::List { .. }));
        assert!(matches!(document.blocks[3], Block::Table { .. }));
        assert!(matches!(
            document.blocks[4],
            Block::FootnoteDefinition { .. }
        ));
        assert!(
            serde_json::to_string(&document.blocks[2])
                .unwrap()
                .contains("task_list_marker"),
            "task-list marker missing from {:#?}",
            document.blocks[2]
        );
        let paragraph = serde_json::to_value(&document.blocks[1]).unwrap();
        assert!(
            paragraph["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|inline| inline["type"] == "strong")
        );
        let rendered = document_to_markdown(&document, Path::new(".")).unwrap();
        assert_eq!(rendered, markdown);
    }

    #[test]
    fn structured_formats_preserve_inline_nodes_and_legacy_documents() {
        let markdown = "# Report\n\nSee **bold** and [docs](https://example.com).\n\n[^note]: Footnote *text*.\n";
        let document = markdown_to_document(markdown, Path::new(".")).unwrap();
        let json = render_document(&document, Format::Json).unwrap();
        let yaml = render_document(&document, Format::Yaml).unwrap();
        let toml = render_document(&document, Format::Toml).unwrap();

        for (source, format) in [
            (json.as_str(), Format::Json),
            (yaml.as_str(), Format::Yaml),
            (toml.as_str(), Format::Toml),
        ] {
            let parsed = parse_document(source, format).unwrap();
            assert_eq!(
                document_to_markdown(&parsed, Path::new(".")).unwrap(),
                markdown
            );
        }

        let legacy = r#"{"blocks":[{"type":"paragraph","text":"Legacy **bold** text."},{"type":"table","rows":[["Name"],["Ada"]]},{"type":"list_item","ordered":true,"level":0,"text":"Step"}]}"#;
        let parsed = parse_document(legacy, Format::Json).unwrap();
        assert_eq!(
            document_to_markdown(&parsed, Path::new(".")).unwrap(),
            "Legacy **bold** text.\n\n| Name |\n| --- |\n| Ada |\n\n1. Step\n"
        );
    }

    #[test]
    fn parses_lists_with_ordering_nesting_and_start_numbers() {
        let markdown = "8. First\n9. Second\n\n- Parent\n    - Nested\n";
        let document = markdown_to_document(markdown, Path::new(".")).unwrap();
        let Block::List {
            ordered: true,
            start: Some(8),
            items,
        } = &document.blocks[0]
        else {
            panic!("expected an ordered list starting at eight");
        };
        assert_eq!(items[0].number, Some(8));
        assert_eq!(items[1].number, Some(9));

        let rendered = document_to_markdown(&document, Path::new(".")).unwrap();
        assert_eq!(rendered, markdown);
    }

    #[test]
    fn inline_footnote_syntax_inside_tilde_fences_stays_code() {
        let markdown = "~~~markdown\n^[literal code]\n~~~\n\nAn inline^[real **note**].\n";
        let document = markdown_to_document(markdown, Path::new(".")).unwrap();
        assert!(matches!(
            &document.blocks[0],
            Block::Code { code, .. } if code == "^[literal code]"
        ));
        assert!(matches!(
            &document.blocks[1],
            Block::Paragraph { content, .. }
                if content.iter().any(|inline| matches!(inline, Inline::Footnote { .. }))
        ));
    }
}
