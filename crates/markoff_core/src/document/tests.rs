use super::{document_to_markdown, markdown_to_document, parse_document, render_document};
use crate::{
    Format,
    document_model::{Block, Inline},
};
use std::path::Path;

#[test]
fn parses_and_renders_block_and_inline_semantics() {
    let markdown = "# Title\n\nA **bold** and *italic* paragraph with [link](https://example.com).\n\n- [x] First\n- Second\n\n| Name | Score |\n| :--- | ---: |\n| Ada | 42 |\n\n[^note]: A footnote.\n";
    let document = markdown_to_document(markdown, Path::new(".")).unwrap();
    assert!(matches!(document.blocks[0], Block::Heading { .. }));
    assert!(matches!(document.blocks[1], Block::Paragraph { .. }));
    assert!(matches!(document.blocks[2], Block::List { .. }));
    assert!(matches!(document.blocks[3], Block::Table { .. }));
    assert!(matches!(
        document.blocks[4],
        Block::FootnoteDefinition { .. }
    ));
    assert!(
        serde_json::to_string(&document.blocks[2])
            .unwrap()
            .contains("task_list_marker"),
        "task-list marker missing from {:#?}",
        document.blocks[2]
    );
    let paragraph = serde_json::to_value(&document.blocks[1]).unwrap();
    assert!(
        paragraph["content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|inline| inline["type"] == "strong")
    );
    let rendered = document_to_markdown(&document, Path::new(".")).unwrap();
    assert_eq!(rendered, markdown);
}

#[test]
fn structured_formats_preserve_inline_nodes_and_legacy_documents() {
    let markdown =
        "# Report\n\nSee **bold** and [docs](https://example.com).\n\n[^note]: Footnote *text*.\n";
    let document = markdown_to_document(markdown, Path::new(".")).unwrap();
    let json = render_document(&document, Format::Json).unwrap();
    let yaml = render_document(&document, Format::Yaml).unwrap();
    let toml = render_document(&document, Format::Toml).unwrap();

    for (source, format) in [
        (json.as_str(), Format::Json),
        (yaml.as_str(), Format::Yaml),
        (toml.as_str(), Format::Toml),
    ] {
        let parsed = parse_document(source, format).unwrap();
        assert_eq!(
            document_to_markdown(&parsed, Path::new(".")).unwrap(),
            markdown
        );
    }

    let legacy = r#"{"blocks":[{"type":"paragraph","text":"Legacy **bold** text."},{"type":"table","rows":[["Name"],["Ada"]]},{"type":"list_item","ordered":true,"level":0,"text":"Step"}]}"#;
    let parsed = parse_document(legacy, Format::Json).unwrap();
    assert_eq!(
        document_to_markdown(&parsed, Path::new(".")).unwrap(),
        "Legacy **bold** text.\n\n| Name |\n| --- |\n| Ada |\n\n1. Step\n"
    );
}

#[test]
fn parses_lists_with_ordering_nesting_and_start_numbers() {
    let markdown = "8. First\n9. Second\n\n- Parent\n    - Nested\n";
    let document = markdown_to_document(markdown, Path::new(".")).unwrap();
    let Block::List {
        ordered: true,
        start: Some(8),
        items,
    } = &document.blocks[0]
    else {
        panic!("expected an ordered list starting at eight");
    };
    assert_eq!(items[0].number, Some(8));
    assert_eq!(items[1].number, Some(9));

    let rendered = document_to_markdown(&document, Path::new(".")).unwrap();
    assert_eq!(rendered, markdown);
}

#[test]
fn inline_footnote_syntax_inside_tilde_fences_stays_code() {
    let markdown = "~~~markdown\n^[literal code]\n~~~\n\nAn inline^[real **note**].\n";
    let document = markdown_to_document(markdown, Path::new(".")).unwrap();
    assert!(matches!(
        &document.blocks[0],
        Block::Code { code, .. } if code == "^[literal code]"
    ));
    assert!(matches!(
        &document.blocks[1],
        Block::Paragraph { content, .. }
            if content.iter().any(|inline| matches!(inline, Inline::Footnote { .. }))
    ));
}
