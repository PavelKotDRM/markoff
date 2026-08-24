use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Block {
    Heading {
        level: u8,
        text: String,
    },
    ListItem {
        ordered: bool,
        level: usize,
        text: String,
    },
    Table {
        rows: Vec<Vec<String>>,
    },
    #[serde(rename = "code_block")]
    Code {
        code: String,
    },
    Quote {
        text: String,
    },
    HorizontalRule,
    Image {
        alt: String,
        format: String,
        data: String,
    },
    Paragraph {
        text: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Document {
    pub(super) blocks: Vec<Block>,
}
