use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Block {
    Heading {
        level: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        content: Vec<Inline>,
    },
    ListItem {
        ordered: bool,
        level: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        number: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        content: Vec<Inline>,
    },
    List {
        ordered: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Table {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rows: Option<Vec<Vec<String>>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        cells: Vec<Vec<Vec<Inline>>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        alignments: Vec<TableAlignment>,
    },
    #[serde(rename = "code_block")]
    Code {
        code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        info: Option<String>,
    },
    Math {
        text: String,
    },
    Quote {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        blocks: Vec<Block>,
    },
    HorizontalRule,
    Image {
        alt: String,
        format: String,
        data: String,
    },
    Paragraph {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        content: Vec<Inline>,
    },
    FootnoteDefinition {
        label: String,
        blocks: Vec<Block>,
    },
    Html {
        html: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Inline {
    Text {
        text: String,
    },
    Emphasis {
        content: Vec<Inline>,
    },
    Strong {
        content: Vec<Inline>,
    },
    Strikethrough {
        content: Vec<Inline>,
    },
    Underline {
        content: Vec<Inline>,
    },
    Superscript {
        content: Vec<Inline>,
    },
    Subscript {
        content: Vec<Inline>,
    },
    Code {
        text: String,
    },
    Math {
        text: String,
        #[serde(default)]
        display: bool,
    },
    Link {
        destination: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        content: Vec<Inline>,
    },
    Image {
        alt: String,
        destination: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<String>,
    },
    FootnoteReference {
        label: String,
    },
    Footnote {
        content: Vec<Inline>,
    },
    Bookmark {
        name: String,
    },
    SoftBreak,
    HardBreak,
    TaskListMarker {
        checked: bool,
    },
    Html {
        html: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TableAlignment {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ListItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) number: Option<u64>,
    pub(super) blocks: Vec<Block>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Document {
    pub(super) blocks: Vec<Block>,
}
