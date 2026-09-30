use super::{convert_html_to_markdown, convert_markdown_to_html};
use crate::test_support::unique_temp_path;
use std::fs;

#[test]
fn converts_markdown_to_html_and_back() {
    let markdown_in = unique_temp_path("html_roundtrip_input", "md");
    let html = unique_temp_path("html_roundtrip_page", "html");
    let markdown_out = unique_temp_path("html_roundtrip_output", "md");
    fs::write(
        &markdown_in,
        "# Title\n\nA **bold** and *italic* [link](https://example.com) paragraph.\n\n- First\n- Second\n",
    )
    .unwrap();

    convert_markdown_to_html(&markdown_in, &html).unwrap();
    let rendered_html = fs::read_to_string(&html).unwrap();
    assert!(rendered_html.contains("<h1>Title</h1>"));
    assert!(rendered_html.contains("<strong>bold</strong>"));

    convert_html_to_markdown(&html, &markdown_out).unwrap();
    let rendered_markdown = fs::read_to_string(&markdown_out).unwrap();
    assert!(rendered_markdown.contains("# Title"));
    assert!(rendered_markdown.contains("**bold**"));
    assert!(rendered_markdown.contains("*italic*"));
    assert!(rendered_markdown.contains("[link](https://example.com)"));
    assert!(rendered_markdown.contains("- First"));
    assert!(rendered_markdown.contains("- Second"));

    fs::remove_file(markdown_in).ok();
    fs::remove_file(html).ok();
    fs::remove_file(markdown_out).ok();
}

#[test]
fn parses_hand_written_html_with_void_elements_and_table() {
    let html = unique_temp_path("html_handwritten", "html");
    let markdown_out = unique_temp_path("html_handwritten_output", "md");
    fs::write(
        &html,
        "<html><head><title>Ignore</title></head><body>\n<h2>Report</h2>\n<p>Line one<br>Line two</p>\n<table><tr><th>Name</th><th>Score</th></tr><tr><td>Ada</td><td>42</td></tr></table>\n<hr>\n</body></html>",
    )
    .unwrap();

    convert_html_to_markdown(&html, &markdown_out).unwrap();
    let rendered = fs::read_to_string(&markdown_out).unwrap();
    assert!(rendered.contains("## Report"));
    assert!(rendered.contains("Line one"));
    assert!(rendered.contains("Line two"));
    assert!(rendered.contains("| Name | Score |"));
    assert!(rendered.contains("| Ada | 42 |"));
    assert!(rendered.contains("---"));
    assert!(!rendered.contains("Ignore"));

    fs::remove_file(html).ok();
    fs::remove_file(markdown_out).ok();
}
