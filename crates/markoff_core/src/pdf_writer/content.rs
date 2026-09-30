mod blocks;
mod inline;
mod text_layout;

pub(super) use blocks::append_blocks;
pub(super) use text_layout::{estimate_run_width, normalize_anchor, slugify, wrap_runs};
