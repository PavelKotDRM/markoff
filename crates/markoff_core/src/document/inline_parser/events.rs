use super::super::markdown_parser::markdown_options;
use super::ProtectedInline;
use super::assets::{inline_plain_text, load_image_asset};
use super::syntax::{parse_inline_html, protect_inline_syntax};
use crate::MarkoffError;
use crate::document_model::Inline;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InlineKind {
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

pub(super) enum InlineContainer {
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

pub(super) struct InlineFrame {
    container: Option<InlineContainer>,
    content: Vec<Inline>,
}

pub(crate) fn parse_inline_events<'a>(
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

pub(super) fn push_inline_frame(frames: &mut Vec<InlineFrame>, container: InlineContainer) {
    frames.push(InlineFrame {
        container: Some(container),
        content: Vec::new(),
    });
}

pub(super) fn close_inline_frame(
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

pub(super) fn push_inline(frames: &mut [InlineFrame], inline: Inline) {
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
