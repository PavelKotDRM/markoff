use super::ProtectedInline;
use super::assets::load_image_asset;
use super::events::{
    InlineContainer, InlineFrame, InlineKind, close_inline_frame, push_inline, push_inline_frame,
};
use crate::MarkoffError;
use crate::document_model::Inline;
use crate::html_tokenizer::{Token, tokenize};
use std::collections::HashMap;
use std::path::Path;

pub(crate) fn protect_inline_syntax(
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

pub(super) fn parse_inline_html(
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
