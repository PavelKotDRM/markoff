mod blocks;
mod inline;
mod text_layout;

pub(super) use blocks::{BlockContext, append_blocks};
pub(super) use text_layout::{
    estimate_run_width, normalize_anchor, reorder_runs_for_display, slugify, uses_emoji_font,
    wrap_runs, wrap_runs_with_first_line_indent,
};
