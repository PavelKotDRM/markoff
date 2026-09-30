use super::{DocxRunStyle, VerticalAlign};
use crate::xml_utils::{MarkdownEscapeContext, markdown_escape};

fn latex_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

pub(crate) fn pageref_target(instruction: &str) -> Option<String> {
    let mut tokens = instruction.split_whitespace();
    while let Some(token) = tokens.next() {
        if token.eq_ignore_ascii_case("PAGEREF") {
            return tokens.next().map(str::to_string);
        }
    }
    None
}

pub(crate) fn markdown_from_docx_run(
    text: &str,
    style: DocxRunStyle,
    page_reference: Option<&str>,
) -> String {
    // Emphasis markers must hug non-whitespace content on both sides, or a
    // CommonMark parser won't treat them as opening/closing delimiters and
    // formatting will bleed into neighboring runs. Keep any leading/trailing
    // whitespace outside the markers instead of wrapping it.
    let core = text.trim_matches(char::is_whitespace);
    if core.is_empty() {
        return text.to_string();
    }
    let leading = &text[..text.len() - text.trim_start_matches(char::is_whitespace).len()];
    let trailing = &text[text.trim_end_matches(char::is_whitespace).len()..];
    let mut rendered = match style.vertical_align {
        VerticalAlign::Superscript => format!("$^{{{}}}$", latex_escape(core)),
        VerticalAlign::Subscript => format!("$_{{{}}}$", latex_escape(core)),
        VerticalAlign::Baseline => {
            let mut rendered = markdown_escape(core, MarkdownEscapeContext::Docx);
            match (style.bold, style.italic) {
                (true, true) => rendered = format!("***{rendered}***"),
                (true, false) => rendered = format!("**{rendered}**"),
                (false, true) => rendered = format!("*{rendered}*"),
                (false, false) => {}
            }
            if style.strikethrough {
                rendered = format!("~~{rendered}~~");
            }
            if style.underline {
                rendered = format!("<u>{rendered}</u>");
            }
            if style.code {
                rendered = format!("`{rendered}`");
            }
            rendered
        }
    };
    if let Some(target) = page_reference {
        let destination = if let Some(target) = target.strip_prefix("external:") {
            target.to_string()
        } else if let Some(target) = target.strip_prefix("anchor:") {
            format!("#{target}")
        } else {
            format!("#{target}")
        };
        rendered = format!("[{rendered}]({destination})");
    }
    format!("{leading}{rendered}{trailing}")
}
