//! Bidirectional HTML <-> Markdown conversion.
//!
//! Markdown -> HTML reuses `pulldown-cmark`'s CommonMark renderer. HTML ->
//! Markdown uses a small tolerant hand-written tokenizer (real-world HTML is
//! often not well-formed XML, so `quick-xml`'s strict parser is not a good
//! fit here) supporting headings, paragraphs, emphasis, links, images, lists,
//! blockquotes, code blocks, and tables. Layout/CSS, forms, and scripts are
//! not preserved.

mod from_html;
mod to_html;

pub(crate) use from_html::convert_html_to_markdown;
pub(crate) use to_html::convert_markdown_to_html;

#[cfg(test)]
mod tests;
