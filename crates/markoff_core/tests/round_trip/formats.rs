use super::{remove_files, temporary_path};
use markoff_core::{Format, convert_file};
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
