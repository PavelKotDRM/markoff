use super::{remove_files, temporary_path};
use markoff_core::{ConversionRequest, Format, convert_document, convert_file};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn assert_odf_package(path: &Path, expected_mime: &str) {
    use std::io::Read;

    let file = fs::File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let first = archive.by_index(0).unwrap();
    assert_eq!(first.name(), "mimetype");
    assert_eq!(first.compression(), zip::CompressionMethod::Stored);
    drop(first);

    let mut mime = String::new();
    archive
        .by_name("mimetype")
        .unwrap()
        .read_to_string(&mut mime)
        .unwrap();
    assert_eq!(mime, expected_mime);
    assert!(archive.by_name("content.xml").is_ok());
    assert!(archive.by_name("META-INF/manifest.xml").is_ok());
}

fn odf_parts(path: &Path) -> BTreeMap<String, Vec<u8>> {
    use std::io::Read;

    let file = fs::File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        parts.insert(entry.name().to_string(), data);
    }
    parts
}

fn append_package_part(path: &Path, name: &str, data: &[u8]) {
    use std::io::Write;

    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    let mut writer = zip::ZipWriter::new_append(file).unwrap();
    writer
        .start_file(name, zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(data).unwrap();
    writer.finish().unwrap();
}

#[test]
fn optional_toml_theme_applies_to_document_outputs() {
    use std::io::Read;

    let markdown = temporary_path("style_theme_input", "md");
    let theme = temporary_path("style_theme", "toml");
    let html = temporary_path("style_theme_output", "html");
    let docx = temporary_path("style_theme_output", "docx");
    let odt = temporary_path("style_theme_output", "odt");
    fs::write(
        &markdown,
        "# Report\n\nText.\n\n| Name | Score |\n| --- | --- |\n| Ada | 42 |\n",
    )
    .unwrap();
    fs::write(
        &theme,
        r##"
[document]
font_family = "Aptos"
font_size_pt = 13
text_color = "#112233"
paragraph_spacing_after_pt = 9
line_height = 1.4

[headings]
font_family = "Georgia"
color = "#445566"
sizes_pt = [28, 22, 18, 15, 13, 11]
spacing_before_pt = 12
spacing_after_pt = 7

[page]
margin_top_pt = 50
margin_right_pt = 45
margin_bottom_pt = 55
margin_left_pt = 40

[table]
header_background = "#DDEEFF"
header_color = "#102030"
border_color = "#708090"

[code]
font_family = "Cascadia Mono"
background = "#F0F1F2"
"##,
    )
    .unwrap();

    for (output, format) in [
        (&html, Format::Html),
        (&docx, Format::Docx),
        (&odt, Format::Odt),
    ] {
        convert_document(&ConversionRequest {
            input: markdown.clone(),
            output: output.clone(),
            from: Format::Markdown,
            to: format,
            overwrite: true,
            csv_delimiter: b',',
            tables_only: false,
            style: Some(theme.clone()),
        })
        .unwrap();
    }

    let html_source = fs::read_to_string(&html).unwrap();
    assert!(html_source.contains("font-family:\"Aptos\""));
    assert!(html_source.contains("color:#112233"));
    assert!(html_source.contains("background:#DDEEFF"));

    let mut docx_archive = zip::ZipArchive::new(fs::File::open(&docx).unwrap()).unwrap();
    let mut docx_styles = String::new();
    docx_archive
        .by_name("word/styles.xml")
        .unwrap()
        .read_to_string(&mut docx_styles)
        .unwrap();
    assert!(docx_styles.contains("w:ascii=\"Aptos\""));
    assert!(docx_styles.contains("w:val=\"445566\""));
    drop(docx_archive);

    let mut odt_archive = zip::ZipArchive::new(fs::File::open(&odt).unwrap()).unwrap();
    let mut odt_content = String::new();
    odt_archive
        .by_name("content.xml")
        .unwrap()
        .read_to_string(&mut odt_content)
        .unwrap();
    assert!(odt_content.contains("fo:font-size=\"13pt\""));
    assert!(odt_content.contains("fo:background-color=\"#DDEEFF\""));

    remove_files(&[&markdown, &theme, &html, &docx, &odt]);
}

fn read_package_text(path: &Path, name: &str) -> String {
    use std::io::Read;

    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

#[test]
fn extended_theme_settings_reach_every_document_output() {
    let markdown = temporary_path("extended_theme_input", "md");
    let theme = temporary_path("extended_theme", "toml");
    let html = temporary_path("extended_theme_output", "html");
    let docx = temporary_path("extended_theme_output", "docx");
    let odt = temporary_path("extended_theme_output", "odt");
    let pdf = temporary_path("extended_theme_output", "pdf");
    let restored = temporary_path("extended_theme_restored", "md");
    fs::write(
        &markdown,
        "# Report\n\nText with [a link](https://example.com) and `code`.\n\n> Quoted text.\n\n- first\n- second\n\n| Name | Score |\n| --- | --- |\n| Ada | 42 |\n| Bob | 7 |\n| Eve | 1 |\n\n---\n\n```\nlet x = 1;\n```\n",
    )
    .unwrap();
    fs::write(
        &theme,
        r##"
[document]
text_align = "justify"
first_line_indent_pt = 12
paragraph_spacing_before_pt = 2

[headings]
level_colors = ["#AA0000", "#00AA00", "#0000AA", "#111111", "#222222", "#333333"]
bold = false
italic = true

[page]
size = "letter"
orientation = "landscape"
header_text = "Quarterly report"
footer_text = "Page {page} of {pages}"

[links]
color = "#FF6600"
underline = false

[blockquote]
text_color = "#555555"
background = "#FAFAFA"
border_color = "#CC0000"
border_width_pt = 3
indent_pt = 20
italic = true

[lists]
indent_pt = 24
bullet = "–"

[code]
font_family = "Cascadia Mono"
font_size_pt = 10
text_color = "#123123"
inline_background = "none"
padding_pt = 6

[table]
font_size_pt = 9
border_width_pt = 1
cell_padding_pt = 5
stripe_background = "#F5F5F5"

[horizontal_rule]
color = "#00FF00"
width_pt = 2

[footnotes]
font_size_pt = 8

[images]
max_width_percent = 60
"##,
    )
    .unwrap();

    for (output, format) in [
        (&html, Format::Html),
        (&docx, Format::Docx),
        (&odt, Format::Odt),
        (&pdf, Format::Pdf),
    ] {
        convert_document(&ConversionRequest {
            input: markdown.clone(),
            output: output.clone(),
            from: Format::Markdown,
            to: format,
            overwrite: true,
            csv_delimiter: b',',
            tables_only: false,
            style: Some(theme.clone()),
        })
        .unwrap();
    }

    let html_source = fs::read_to_string(&html).unwrap();
    for expected in [
        "size:792pt 612pt",
        "text-align:justify",
        "text-indent:12pt",
        "h1{font-family:\"Calibri\";font-size:16pt;color:#AA0000;font-weight:normal;font-style:italic",
        "a{color:#FF6600;text-decoration:none;}",
        "border-left:3pt solid #CC0000",
        "list-style-type:\"– \"",
        "tbody tr:nth-child(even) td{background:#F5F5F5;}",
        "hr{border:none;border-top:2pt solid #00FF00;}",
        "img{max-width:60%",
        "@bottom-center{content:\"Page \" counter(page) \" of \" counter(pages)",
    ] {
        assert!(
            html_source.contains(expected),
            "missing {expected:?} in {html_source}"
        );
    }

    let document = read_package_text(&docx, "word/document.xml");
    let styles = read_package_text(&docx, "word/styles.xml");
    let numbering = read_package_text(&docx, "word/numbering.xml");
    let footer = read_package_text(&docx, "word/footer1.xml");
    assert!(document.contains("<w:pgSz w:w=\"15840\" w:h=\"12240\" w:orient=\"landscape\"/>"));
    assert!(document.contains("<w:headerReference w:type=\"default\""));
    assert!(document.contains("<w:rStyle w:val=\"CodeChar\"/>"));
    assert!(document.contains("<w:shd w:val=\"clear\" w:fill=\"F5F5F5\"/>"));
    assert!(styles.contains("<w:jc w:val=\"both\"/>"));
    assert!(styles.contains("<w:color w:val=\"FF6600\"/><w:u w:val=\"none\"/>"));
    assert!(styles.contains("<w:color w:val=\"AA0000\"/>"));
    assert!(numbering.contains("w:lvlText w:val=\"–\""));
    assert!(footer.contains("w:instr=\" NUMPAGES \""));

    let odt_content = read_package_text(&odt, "content.xml");
    let odt_styles = read_package_text(&odt, "styles.xml");
    assert!(odt_content.contains("fo:text-align=\"justify\""));
    assert!(odt_content.contains("text:bullet-char=\"–\""));
    assert!(odt_content.contains("table:style-name=\"TableCellStripe\""));
    assert!(odt_content.contains("fo:border-left=\"3pt solid #CC0000\""));
    assert!(odt_styles.contains("fo:page-width=\"792pt\""));
    assert!(odt_styles.contains("<text:page-count>"));
    assert_eq!(
        odt_content.matches("style:name=\"Body\"").count(),
        1,
        "theme styles must not be duplicated"
    );

    let pdf_bytes = fs::read(&pdf).unwrap();
    assert!(pdf_bytes.starts_with(b"%PDF"));

    for (source, format) in [(&docx, Format::Docx), (&odt, Format::Odt)] {
        convert_file(source, &restored, format, Format::Markdown).unwrap();
        let restored_markdown = fs::read_to_string(&restored).unwrap();
        assert!(
            restored_markdown.contains("`code`"),
            "inline code lost after {format:?} round trip: {restored_markdown}"
        );
    }

    remove_files(&[&markdown, &theme, &html, &docx, &odt, &pdf, &restored]);
}

#[test]
fn invalid_style_theme_is_reported_without_output() {
    let markdown = temporary_path("invalid_style_input", "md");
    let theme = temporary_path("invalid_style", "toml");
    let output = temporary_path("invalid_style_output", "html");
    fs::write(&markdown, "# Report\n").unwrap();
    fs::write(&theme, "[document]\ntext_color = \"blue\"\n").unwrap();

    let error = convert_document(&ConversionRequest {
        input: markdown.clone(),
        output: output.clone(),
        from: Format::Markdown,
        to: Format::Html,
        overwrite: true,
        csv_delimiter: b',',
        tables_only: false,
        style: Some(theme.clone()),
    })
    .unwrap_err();
    assert!(error.to_string().contains("document.text_color"));
    assert!(!output.exists());

    remove_files(&[&markdown, &theme, &output]);
}

#[test]
fn edited_open_document_text_preserves_all_other_package_content() {
    for (format, extension, input, old, new) in [
        (
            Format::Odt,
            "odt",
            "# Report\n\nAn **original** paragraph.\n",
            "original",
            "edited",
        ),
        (
            Format::Ods,
            "ods",
            "## Sheet\n\n| Name | Score |\n| --- | --- |\n| Ada | 42 |\n",
            "Ada",
            "Grace",
        ),
        (
            Format::Odp,
            "odp",
            "## Introduction\n\nOriginal welcome text.\n",
            "Original",
            "Edited",
        ),
    ] {
        let markdown = temporary_path("odf_edit_input", "md");
        let document = temporary_path("odf_edit_document", extension);
        let extracted = temporary_path("odf_edit_extracted", "md");
        let edited = temporary_path("odf_edit_result", extension);
        fs::write(&markdown, input).unwrap();

        convert_file(&markdown, &document, Format::Markdown, format).unwrap();
        if format == Format::Odt {
            append_package_part(
                &document,
                "Configurations2/markoff-extension.xml",
                b"<extension:settings xmlns:extension=\"urn:markoff:test\"/>",
            );
            append_package_part(&document, "Pictures/opaque-resource.bin", &[0, 1, 2, 255]);
        }
        convert_file(&document, &extracted, format, Format::Markdown).unwrap();
        let service_markdown = fs::read_to_string(&extracted).unwrap();
        fs::write(&extracted, service_markdown.replacen(old, new, 1)).unwrap();
        convert_file(&extracted, &edited, Format::Markdown, format).unwrap();

        let original_parts = odf_parts(&document);
        let edited_parts = odf_parts(&edited);
        assert_eq!(
            edited_parts.keys().collect::<Vec<_>>(),
            original_parts.keys().collect::<Vec<_>>()
        );
        for (name, original_part) in &original_parts {
            let edited_part = &edited_parts[name];
            if name == "content.xml" {
                let edited_xml = String::from_utf8(edited_part.clone()).unwrap();
                assert!(edited_xml.contains(new));
                assert_eq!(edited_xml.replacen(new, old, 1).as_bytes(), original_part);
            } else {
                assert_eq!(edited_part, original_part, "package part changed: {name}");
            }
        }

        remove_files(&[&markdown, &document, &extracted, &edited]);
    }
}

#[test]
fn structural_open_document_edit_fails_instead_of_losing_package_objects() {
    let markdown = temporary_path("odf_structure_input", "md");
    let odt = temporary_path("odf_structure_document", "odt");
    let extracted = temporary_path("odf_structure_extracted", "md");
    let output = temporary_path("odf_structure_result", "odt");
    fs::write(&markdown, "# Report\n\nOriginal paragraph.\n").unwrap();

    convert_file(&markdown, &odt, Format::Markdown, Format::Odt).unwrap();
    append_package_part(&odt, "Pictures/preserved.bin", &[1, 2, 3]);
    convert_file(&odt, &extracted, Format::Odt, Format::Markdown).unwrap();
    let edited = fs::read_to_string(&extracted).unwrap().replacen(
        "Original paragraph.",
        "Original paragraph.\n\nNew paragraph.",
        1,
    );
    fs::write(&extracted, edited).unwrap();

    let error = convert_file(&extracted, &output, Format::Markdown, Format::Odt).unwrap_err();
    assert!(
        error.to_string().contains("cannot be mapped safely"),
        "unexpected error: {error}"
    );
    assert!(!output.exists());

    remove_files(&[&markdown, &odt, &extracted, &output]);
}

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

#[test]
fn odt_round_trip_preserves_supported_document_elements() {
    let markdown = temporary_path("odt_input", "md");
    let odt = temporary_path("odt_document", "odt");
    let restored = temporary_path("odt_output", "md");
    let restored_odt = temporary_path("odt_restored", "odt");
    fs::write(
        &markdown,
        "# Report\n\nA **bold**, *italic*, ~~deleted~~, <u>underlined</u> [link](https://example.com), `code`, x$^{2}$ and H$_{2}$O.\n\n- [x] First\n- [ ] Second\n\n| Name | Score |\n| --- | --- |\n| Ada | 42 |\n\n> Quoted text.\n\n---\n\n```rust\nfn main() {}\n```\n\n$$\nE = mc^2\n$$\n",
    )
    .unwrap();

    convert_file(&markdown, &odt, Format::Markdown, Format::Odt).unwrap();
    assert_odf_package(&odt, "application/vnd.oasis.opendocument.text");
    convert_file(&odt, &restored, Format::Odt, Format::Markdown).unwrap();
    convert_file(&restored, &restored_odt, Format::Markdown, Format::Odt).unwrap();
    assert_eq!(fs::read(&restored_odt).unwrap(), fs::read(&odt).unwrap());

    let rendered = fs::read_to_string(&restored).unwrap();
    for expected in [
        "# Report",
        "**bold**",
        "*italic*",
        "~~deleted~~",
        "<u>underlined</u>",
        "[link](https://example.com)",
        "`code`",
        "x$^{2}$",
        "H$_{2}$O",
        "- [x] First",
        "- [ ] Second",
        "| Name | Score |",
        "| Ada | 42 |",
        "> Quoted text.",
        "---",
        "```",
        "fn main() {}",
        "$$E = mc^2$$",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }

    remove_files(&[&markdown, &odt, &restored, &restored_odt]);
}

#[test]
fn ods_round_trip_preserves_sheets_and_cell_text() {
    let markdown = temporary_path("ods_input", "md");
    let ods = temporary_path("ods_workbook", "ods");
    let restored = temporary_path("ods_output", "md");
    let restored_ods = temporary_path("ods_restored", "ods");
    fs::write(
        &markdown,
        "## People\n\n| Name | Note |\n| --- | --- |\n| Ada | a \\| b |\n\n## Scores\n\n| Name | Score |\n| --- | --- |\n| Ada | 42 |\n",
    )
    .unwrap();

    convert_file(&markdown, &ods, Format::Markdown, Format::Ods).unwrap();
    assert_odf_package(&ods, "application/vnd.oasis.opendocument.spreadsheet");
    convert_file(&ods, &restored, Format::Ods, Format::Markdown).unwrap();
    convert_file(&restored, &restored_ods, Format::Markdown, Format::Ods).unwrap();
    assert_eq!(fs::read(&restored_ods).unwrap(), fs::read(&ods).unwrap());

    let rendered = fs::read_to_string(&restored).unwrap();
    for expected in [
        "## People",
        "| Ada | a \\| b |",
        "## Scores",
        "| Ada | 42 |",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }

    remove_files(&[&markdown, &ods, &restored, &restored_ods]);
}

#[test]
fn odp_round_trip_preserves_titles_body_text_and_bullets() {
    let markdown = temporary_path("odp_input", "md");
    let odp = temporary_path("odp_presentation", "odp");
    let restored = temporary_path("odp_output", "md");
    let restored_odp = temporary_path("odp_restored", "odp");
    fs::write(
        &markdown,
        "# **Introduction**\n\nWelcome [here](https://example.com).\n\n## Agenda\n\n- *First* topic\n- Second topic\n",
    )
    .unwrap();

    convert_file(&markdown, &odp, Format::Markdown, Format::Odp).unwrap();
    assert_odf_package(&odp, "application/vnd.oasis.opendocument.presentation");
    convert_file(&odp, &restored, Format::Odp, Format::Markdown).unwrap();
    convert_file(&restored, &restored_odp, Format::Markdown, Format::Odp).unwrap();
    assert_eq!(fs::read(&restored_odp).unwrap(), fs::read(&odp).unwrap());

    let rendered = fs::read_to_string(&restored).unwrap();
    for expected in [
        "## Introduction",
        "Welcome here.",
        "## Agenda",
        "- First topic",
        "- Second topic",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }

    remove_files(&[&markdown, &odp, &restored, &restored_odp]);
}
