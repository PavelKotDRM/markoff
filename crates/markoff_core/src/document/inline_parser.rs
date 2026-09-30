mod assets;
mod events;
mod syntax;

pub(super) use events::parse_inline_events;
pub(crate) use syntax::expand_inline_footnotes;
pub(super) use syntax::protect_inline_syntax;

#[derive(Clone)]
pub(super) enum ProtectedInline {
    InlineFootnote(String),
    FootnoteReference(String),
}
