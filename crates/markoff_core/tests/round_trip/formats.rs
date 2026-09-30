use super::{remove_files, temporary_path};
use markoff_core::{Format, convert_file};
use std::fs;

#[test]
fn markdown_json_yaml_toml_docx_html_chain_preserves_document_elements() {
    let markdown = temporary_path("format_chain_input", "md");
    let json = temporary_path("format_chain", "json");
    let yaml = temporary_path("format_chain", "yaml");
    let toml = temporary_path("format_chain", "toml");
    let docx = temporary_path("format_chain", "docx");
    let html = temporary_path("format_chain", "html");
    let restored_markdown = temporary_path("format_chain_restored", "md");
    fs::write(
        &markdown,
        "# Report\n\nA paragraph.\n\n- First item\n- Second item\n\n| Name | Score |\n| --- | --- |\n| Ada | 42 |\n",
    )
    .unwrap();

    convert_file(&markdown, &json, Format::Markdown, Format::Json).unwrap();
    convert_file(&json, &yaml, Format::Json, Format::Yaml).unwrap();
    convert_file(&yaml, &toml, Format::Yaml, Format::Toml).unwrap();

    let json_document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    let yaml_document: serde_json::Value =
        serde_yaml::from_str(&fs::read_to_string(&yaml).unwrap()).unwrap();
    let toml_document: serde_json::Value =
        toml::from_str(&fs::read_to_string(&toml).unwrap()).unwrap();
    assert_eq!(yaml_document, json_document);
    assert_eq!(toml_document, json_document);

    convert_file(&toml, &docx, Format::Toml, Format::Docx).unwrap();
    convert_file(&docx, &html, Format::Docx, Format::Html).unwrap();

    let rendered = fs::read_to_string(&html).unwrap();
    for expected in [
        "<h1>Report</h1>",
        "<p>A paragraph.</p>",
        "<ul>",
        "<p>First item</p>",
        "<p>Second item</p>",
        "<table>",
        "<th>Name</th>",
        "<td>Ada</td>",
        "<td>42</td>",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in final HTML: {rendered:?}"
        );
    }

    convert_file(&html, &restored_markdown, Format::Html, Format::Markdown).unwrap();
    let restored = fs::read_to_string(&restored_markdown).unwrap();
    assert_eq!(restored, fs::read_to_string(&markdown).unwrap());

    remove_files(&[
        &markdown,
        &json,
        &yaml,
        &toml,
        &docx,
        &html,
        &restored_markdown,
    ]);
}

#[test]
fn html_round_trip_preserves_headings_lists_and_links() {
    let markdown = temporary_path("html_roundtrip_input", "md");
    let html = temporary_path("html_roundtrip", "html");
    let restored = temporary_path("html_roundtrip_output", "md");
    fs::write(
        &markdown,
        "# Report\n\nA **bold** claim with a [link](https://example.com).\n\n- Alpha\n- Beta\n",
    )
    .unwrap();

    convert_file(&markdown, &html, Format::Markdown, Format::Html).unwrap();
    convert_file(&html, &restored, Format::Html, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&restored).unwrap();
    for expected in [
        "# Report",
        "**bold**",
        "[link](https://example.com)",
        "- Alpha",
        "- Beta",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }

    remove_files(&[&markdown, &html, &restored]);
}

#[test]
fn html_to_markdown_preserves_link_destinations_with_markdown_delimiters() {
    let html = temporary_path("html_link_destination_input", "html");
    let markdown = temporary_path("html_link_destination_output", "md");
    fs::write(
        &html,
        r#"<p><a href="https://example.com/a b_(c)">Docs</a> <img src="images/a b_(c).png" alt="Chart"></p>"#,
    )
    .unwrap();

    convert_file(&html, &markdown, Format::Html, Format::Markdown).unwrap();

    assert_eq!(
        fs::read_to_string(&markdown).unwrap(),
        "[Docs](<https://example.com/a b_(c)>) ![Chart](<images/a b_(c).png>)\n"
    );

    remove_files(&[&html, &markdown]);
}

#[test]
fn pptx_round_trip_preserves_slide_titles_and_bullets() {
    let markdown = temporary_path("pptx_roundtrip_input", "md");
    let pptx = temporary_path("pptx_roundtrip", "pptx");
    let restored = temporary_path("pptx_roundtrip_output", "md");
    fs::write(
        &markdown,
        "# Introduction\n\nWelcome note.\n\n## Agenda\n\n- Topic one\n- Topic two\n",
    )
    .unwrap();

    convert_file(&markdown, &pptx, Format::Markdown, Format::Pptx).unwrap();
    convert_file(&pptx, &restored, Format::Pptx, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&restored).unwrap();
    for expected in [
        "## Introduction",
        "Welcome note.",
        "## Agenda",
        "- Topic one",
        "- Topic two",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }

    remove_files(&[&markdown, &pptx, &restored]);
}

#[test]
fn markdown_to_pptx_converts_inline_markup_to_equivalent_plain_text() {
    let markdown = temporary_path("pptx_inline_markup_input", "md");
    let pptx = temporary_path("pptx_inline_markup", "pptx");
    let restored = temporary_path("pptx_inline_markup_output", "md");
    fs::write(
        &markdown,
        "# **Welcome**\n\nRead [the docs](https://example.com) and use `markoff`.\n\n- *First* topic\n",
    )
    .unwrap();

    convert_file(&markdown, &pptx, Format::Markdown, Format::Pptx).unwrap();
    convert_file(&pptx, &restored, Format::Pptx, Format::Markdown).unwrap();

    assert_eq!(
        fs::read_to_string(&restored).unwrap(),
        "## Welcome\n\nRead the docs and use markoff.\n\n- First topic\n\n"
    );

    remove_files(&[&markdown, &pptx, &restored]);
}
