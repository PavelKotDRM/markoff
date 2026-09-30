mod from_docx;
mod to_docx;

pub(crate) use from_docx::{markdown_from_docx_run, pageref_target};
pub(crate) use to_docx::{
    markdown_code_block_to_docx_runs, markdown_inline_to_docx_runs_with_links, markdown_list_item,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerticalAlign {
    Baseline,
    Superscript,
    Subscript,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocxRunStyle {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) strikethrough: bool,
    pub(crate) underline: bool,
    pub(crate) code: bool,
    pub(crate) vertical_align: VerticalAlign,
}

#[derive(Clone)]
pub(crate) enum DocxHyperlink {
    Relationship(String),
    Anchor(String),
}
