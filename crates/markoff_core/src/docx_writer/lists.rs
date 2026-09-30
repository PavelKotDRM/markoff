use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NumberingStyle {
    Bullet,
    Decimal,
}

#[derive(Clone, Copy)]
pub(super) struct ListParagraph {
    pub(super) num_id: u32,
    pub(super) level: usize,
}

#[derive(Clone, Copy)]
pub(super) struct ListDefinition {
    pub(super) num_id: u32,
    pub(super) style: NumberingStyle,
    pub(super) level: usize,
    pub(super) start: u64,
}

#[derive(Default)]
pub(super) struct MarkdownLists {
    pub(super) items_by_line: Vec<Option<ListParagraph>>,
    pub(super) definitions: Vec<ListDefinition>,
}

/// Gives each Markdown list block its own DOCX numbering instance.
pub(super) fn parse_markdown_lists(source: &str) -> MarkdownLists {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let mut lists = MarkdownLists {
        items_by_line: vec![None; source.lines().count()],
        ..MarkdownLists::default()
    };
    let mut list_stack = Vec::new();
    let mut next_num_id = 1;

    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Start(Tag::List(start)) => {
                let definition = ListDefinition {
                    num_id: next_num_id,
                    style: if start.is_some() {
                        NumberingStyle::Decimal
                    } else {
                        NumberingStyle::Bullet
                    },
                    level: list_stack.len(),
                    start: start.unwrap_or(1),
                };
                next_num_id += 1;
                lists.definitions.push(definition);
                list_stack.push(definition);
            }
            Event::Start(Tag::Item) => {
                if let Some(list) = list_stack.last() {
                    let line_index = source[..range.start]
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .count();
                    if let Some(item) = lists.items_by_line.get_mut(line_index) {
                        *item = Some(ListParagraph {
                            num_id: list.num_id,
                            level: list.level,
                        });
                    }
                }
            }
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            _ => {}
        }
    }

    lists
}
