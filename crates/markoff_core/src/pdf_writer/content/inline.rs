use super::super::*;
use crate::document_model::Inline;
use crate::error::invalid_data;
use base64::Engine as _;
use std::sync::Arc;

pub(super) fn append_inline_content(
    content: &[Inline],
    fallback: Option<&str>,
    initial_style: InlineStyle,
    options: ParagraphOptions,
    output: &mut Vec<PdfElement>,
) -> Result<(), MarkoffError> {
    append_inline_content_with_prefix(
        content,
        fallback,
        Vec::new(),
        initial_style,
        options,
        output,
    )
}

pub(super) fn append_inline_content_with_prefix(
    content: &[Inline],
    fallback: Option<&str>,
    prefix: Vec<TextRun>,
    initial_style: InlineStyle,
    options: ParagraphOptions,
    output: &mut Vec<PdfElement>,
) -> Result<(), MarkoffError> {
    let mut parts = Vec::new();
    if content.is_empty() {
        if let Some(text) = fallback {
            push_text_part(
                &mut parts,
                TextRun {
                    text: text.to_string(),
                    style: initial_style,
                },
            );
        }
    } else {
        collect_inline_parts(content, &initial_style, &mut parts)?;
    }
    append_parts(parts, prefix, options, output);
    Ok(())
}

fn collect_inline_parts(
    inlines: &[Inline],
    style: &InlineStyle,
    output: &mut Vec<InlinePart>,
) -> Result<(), MarkoffError> {
    for inline in inlines {
        match inline {
            Inline::Text { text } => push_text_part(
                output,
                TextRun {
                    text: text.clone(),
                    style: style.clone(),
                },
            ),
            Inline::Emphasis { content } => {
                let mut nested = style.clone();
                nested.italic = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Strong { content } => {
                let mut nested = style.clone();
                nested.bold = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Strikethrough { content } => {
                let mut nested = style.clone();
                nested.strikethrough = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Underline { content } => {
                let mut nested = style.clone();
                nested.underline = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Superscript { content } => {
                let mut nested = style.clone();
                nested.superscript = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Subscript { content } => {
                let mut nested = style.clone();
                nested.subscript = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Code { text } => {
                let mut nested = style.clone();
                nested.code = true;
                push_text_part(
                    output,
                    TextRun {
                        text: text.clone(),
                        style: nested,
                    },
                );
            }
            Inline::Math { text, .. } => {
                let mut nested = style.clone();
                nested.italic = true;
                push_text_part(
                    output,
                    TextRun {
                        text: text.clone(),
                        style: nested,
                    },
                );
            }
            Inline::Link {
                destination,
                content,
                ..
            } => {
                let mut nested = style.clone();
                let is_internal = destination.starts_with('#');
                let is_safe_external = is_safe_external_uri(destination);
                nested.link =
                    (is_internal || is_safe_external).then(|| Arc::from(destination.as_str()));
                nested.underline = nested.link.is_some();
                collect_inline_parts(content, &nested, output)?;
                if !destination.is_empty() && !is_internal && !is_safe_external {
                    push_text_part(
                        output,
                        TextRun {
                            text: format!(" ({destination})"),
                            style: style.clone(),
                        },
                    );
                }
            }
            Inline::Image {
                alt,
                destination,
                data,
                ..
            } => {
                let bytes = data
                    .as_ref()
                    .map(|data| {
                        base64::engine::general_purpose::STANDARD
                            .decode(data)
                            .map_err(invalid_data)
                    })
                    .transpose()?;
                if let Some(bytes) = bytes {
                    output.push(InlinePart::Image(PdfImage {
                        bytes,
                        alt: alt.clone(),
                        indent: 0.0,
                        link: style.link.clone(),
                    }));
                } else {
                    let label = if alt.is_empty() {
                        destination.as_str()
                    } else {
                        alt.as_str()
                    };
                    push_text_part(
                        output,
                        TextRun {
                            text: format!("[image: {label}]"),
                            style: style.clone(),
                        },
                    );
                }
            }
            Inline::FootnoteReference { label } => {
                let mut nested = style.clone();
                nested.superscript = true;
                nested.link = Some(Arc::from(format!("#fn-{label}")));
                push_text_part(
                    output,
                    TextRun {
                        text: format!("[{label}]"),
                        style: nested,
                    },
                );
            }
            Inline::Footnote { content } => {
                let mut nested = style.clone();
                nested.superscript = true;
                collect_inline_parts(content, &nested, output)?;
            }
            Inline::Bookmark { name } => output.push(InlinePart::Anchor(name.clone())),
            Inline::SoftBreak => push_text_part(
                output,
                TextRun {
                    text: " ".to_string(),
                    style: style.clone(),
                },
            ),
            Inline::HardBreak => push_text_part(
                output,
                TextRun {
                    text: "\n".to_string(),
                    style: style.clone(),
                },
            ),
            Inline::TaskListMarker { checked } => push_text_part(
                output,
                TextRun {
                    text: if *checked {
                        "[x] ".to_string()
                    } else {
                        "[ ] ".to_string()
                    },
                    style: style.clone(),
                },
            ),
            Inline::Html { html } => {
                let text = super::blocks::strip_html_tags(html);
                if !text.is_empty() {
                    push_text_part(
                        output,
                        TextRun {
                            text,
                            style: style.clone(),
                        },
                    );
                }
            }
        }
    }
    Ok(())
}

fn is_safe_external_uri(destination: &str) -> bool {
    destination
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .is_some_and(|scheme| matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel"))
}

fn push_text_part(output: &mut Vec<InlinePart>, run: TextRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(InlinePart::Run(previous)) = output.last_mut()
        && previous.style == run.style
    {
        previous.text.push_str(&run.text);
    } else {
        output.push(InlinePart::Run(run));
    }
}

fn push_run(output: &mut Vec<TextRun>, run: TextRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(previous) = output.last_mut()
        && previous.style == run.style
    {
        previous.text.push_str(&run.text);
    } else {
        output.push(run);
    }
}

fn append_parts(
    parts: Vec<InlinePart>,
    prefix: Vec<TextRun>,
    options: ParagraphOptions,
    output: &mut Vec<PdfElement>,
) {
    let mut runs = prefix;
    let mut has_layout_content = false;
    let mut has_paragraph = false;
    for part in parts {
        match part {
            InlinePart::Run(run) => push_run(&mut runs, run),
            InlinePart::Image(mut image) => {
                if flush_paragraph(
                    &mut runs,
                    &options,
                    has_layout_content,
                    has_paragraph,
                    false,
                    output,
                ) {
                    has_paragraph = true;
                }
                has_layout_content = true;
                image.indent += options.indent;
                output.push(PdfElement::Image(image));
            }
            InlinePart::Anchor(name) => {
                if flush_paragraph(
                    &mut runs,
                    &options,
                    has_layout_content,
                    has_paragraph,
                    false,
                    output,
                ) {
                    has_paragraph = true;
                }
                has_layout_content = true;
                output.push(PdfElement::Anchor(name));
            }
        }
    }
    flush_paragraph(
        &mut runs,
        &options,
        has_layout_content,
        has_paragraph,
        true,
        output,
    );
}

fn flush_paragraph(
    runs: &mut Vec<TextRun>,
    options: &ParagraphOptions,
    has_content: bool,
    has_paragraph: bool,
    last: bool,
    output: &mut Vec<PdfElement>,
) -> bool {
    if runs.is_empty() {
        return false;
    }
    let mut options = options.clone();
    if has_content {
        options.space_before = 0.0;
    }
    if has_paragraph {
        options.heading = None;
    }
    if !last {
        options.space_after = 0.0;
    }
    output.push(PdfElement::Paragraph(PdfParagraph {
        runs: std::mem::take(runs),
        options,
    }));
    true
}

pub(super) fn inline_runs_without_images(inlines: &[Inline]) -> Result<Vec<TextRun>, MarkoffError> {
    let mut parts = Vec::new();
    collect_inline_parts(inlines, &InlineStyle::default(), &mut parts)?;
    let mut runs: Vec<TextRun> = Vec::new();
    for part in parts {
        match part {
            InlinePart::Run(run) => {
                if let Some(previous) = runs.last_mut()
                    && previous.style == run.style
                {
                    previous.text.push_str(&run.text);
                } else {
                    runs.push(run);
                }
            }
            InlinePart::Image(image) => {
                let label = if image.alt.is_empty() {
                    "[image]".to_string()
                } else {
                    format!("[image: {}]", image.alt)
                };
                runs.push(TextRun {
                    text: label,
                    style: InlineStyle::default(),
                });
            }
            InlinePart::Anchor(_) => {}
        }
    }
    Ok(runs)
}

pub(super) fn plain_run(text: &str) -> TextRun {
    TextRun {
        text: text.to_string(),
        style: InlineStyle::default(),
    }
}

pub(super) fn inline_plain_text(
    inlines: &[Inline],
    fallback: Option<&str>,
) -> Result<String, MarkoffError> {
    if inlines.is_empty() {
        Ok(fallback.unwrap_or_default().to_string())
    } else {
        Ok(inline_runs_without_images(inlines)?
            .into_iter()
            .map(|run| run.text)
            .collect())
    }
}
