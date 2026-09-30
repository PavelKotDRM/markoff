use std::collections::{HashMap, HashSet};

use crate::docx_inline::DocxHyperlink;
use crate::docx_markdown::heading_anchor;
use crate::xml_utils::xml_attribute_escape;

#[derive(Default)]
pub(super) struct HyperlinkAllocator {
    next_id: u32,
    next_bookmark_id: usize,
    relationships: Vec<(String, String)>,
    anchors: HashMap<String, String>,
    bookmark_names: HashSet<String>,
}

impl HyperlinkAllocator {
    pub(super) fn new(
        first_id: u32,
        first_bookmark_id: usize,
        anchors: &HashMap<String, String>,
    ) -> Self {
        Self {
            next_id: first_id,
            next_bookmark_id: first_bookmark_id,
            relationships: Vec::new(),
            anchors: anchors.clone(),
            bookmark_names: anchors.values().cloned().collect(),
        }
    }

    pub(super) fn resolve(&mut self, destination: &str) -> Option<DocxHyperlink> {
        if destination.is_empty() {
            return None;
        }
        if let Some(anchor) = destination.strip_prefix('#') {
            return self
                .anchors
                .get(anchor)
                .cloned()
                .or_else(|| (!anchor.is_empty()).then(|| anchor.to_string()))
                .map(DocxHyperlink::Anchor);
        }

        let id = format!("rId{}", self.next_id);
        self.next_id += 1;
        self.relationships
            .push((id.clone(), destination.to_string()));
        Some(DocxHyperlink::Relationship(id))
    }

    pub(super) fn relationship_entries(&self) -> String {
        self.relationships
            .iter()
            .map(|(id, target)| {
                format!(
                    "<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
                    xml_attribute_escape(id),
                    xml_attribute_escape(target)
                )
            })
            .collect()
    }

    pub(super) fn has_relationships(&self) -> bool {
        !self.relationships.is_empty()
    }

    pub(super) fn wrap_bookmarks(&mut self, mut content: String, bookmarks: Vec<String>) -> String {
        for name in bookmarks.into_iter().rev() {
            if self.anchors.values().any(|heading| heading == &name)
                || !self.bookmark_names.insert(name.clone())
            {
                continue;
            }
            let id = self.next_bookmark_id;
            self.next_bookmark_id += 1;
            let name = if is_word_bookmark_name(&name) {
                name
            } else {
                format!("_markoff_bookmark_{id}")
            };
            let start = format!(
                "<w:bookmarkStart w:id=\"{id}\" w:name=\"{}\"/>",
                xml_attribute_escape(&name)
            );
            let end = format!("<w:bookmarkEnd w:id=\"{id}\"/>");
            content = format!("{start}{content}{end}");
        }
        content
    }
}

pub(super) struct Bookmark {
    pub(super) id: usize,
    pub(super) name: String,
}

pub(super) fn build_heading_bookmarks(
    lines: &[&str],
) -> (Vec<Option<Bookmark>>, HashMap<String, String>) {
    let mut used_anchors = HashMap::new();
    let mut bookmarks = Vec::with_capacity(lines.len());
    let mut anchors = HashMap::new();

    for (line_index, line) in lines.iter().enumerate() {
        let Some((_level, content)) = markdown_heading(line) else {
            bookmarks.push(None);
            continue;
        };
        let Some(base_anchor) = heading_anchor(content) else {
            bookmarks.push(None);
            continue;
        };
        let occurrence = used_anchors.entry(base_anchor.clone()).or_insert(0usize);
        let anchor = if *occurrence == 0 {
            base_anchor
        } else {
            format!("{base_anchor}-{}", *occurrence)
        };
        *occurrence += 1;
        let name = if is_word_bookmark_name(&anchor) {
            anchor.clone()
        } else {
            format!("_markoff_{}", line_index + 1)
        };
        anchors.insert(anchor, name.clone());
        bookmarks.push(Some(Bookmark {
            id: line_index + 1,
            name,
        }));
    }

    (bookmarks, anchors)
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&level) || line.as_bytes().get(level) != Some(&b' ') {
        return None;
    }
    Some((level, line[level + 1..].trim()))
}

fn is_word_bookmark_name(name: &str) -> bool {
    name.len() <= 40
        && name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}
