use super::TEMP_FILE_SEQUENCE;
use markoff_core::{Format, convert_file};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

#[test]
fn golden_document_round_trip_preserves_core_markdown_content() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("golden/Golden_Word_Test_Document_v2.docx");
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "markoff_golden_docx_{}_{}",
        std::process::id(),
        sequence
    ));
    fs::create_dir_all(&directory).unwrap();
    let markdown = directory.join("document.md");
    let restored_document = directory.join("restored.docx");
    let restored_markdown = directory.join("restored.md");

    convert_file(&source, &markdown, Format::Docx, Format::Markdown).unwrap();
    convert_file(
        &markdown,
        &restored_document,
        Format::Markdown,
        Format::Docx,
    )
    .unwrap();
    {
        use std::io::Read;
        use zip::ZipArchive;

        let file = fs::File::open(&restored_document).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let svg_parts = archive
            .file_names()
            .filter_map(Result::ok)
            .filter(|name| name.starts_with("word/media/") && name.ends_with(".svg"))
            .count();
        assert_eq!(svg_parts, 4, "SVG previews should be embedded in the DOCX");
        let mut document_xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut document_xml)
            .unwrap();
        assert!(
            document_xml.contains("<asvg:svgBlip"),
            "DOCX drawings should reference their embedded SVG previews"
        );
        let mut content_types = String::new();
        archive
            .by_name("[Content_Types].xml")
            .unwrap()
            .read_to_string(&mut content_types)
            .unwrap();
        assert!(content_types.contains("Extension=\"svg\""));
    }
    convert_file(
        &restored_document,
        &restored_markdown,
        Format::Docx,
        Format::Markdown,
    )
    .unwrap();

    let source_markdown = fs::read_to_string(&markdown).unwrap();
    assert!(
        source_markdown.contains("[1. Heading Level 1 / Заголовок уровня 1 4](#_Toc000000002)"),
        "updated table of contents entry should retain its title, page, and bookmark: {source_markdown:?}"
    );
    assert_eq!(
        source_markdown.matches("This is a text box").count(),
        1,
        "text box content should be emitted once"
    );
    for graphic in [
        "image/graphic3.svg",
        "image/graphic4.svg",
        "image/graphic5.svg",
        "image/graphic6.svg",
    ] {
        assert!(
            source_markdown.contains(graphic),
            "missing SVG preview {graphic:?} in golden DOCX output"
        );
        let svg = fs::read_to_string(directory.join(graphic)).unwrap();
        assert!(svg.starts_with("<svg "), "invalid SVG preview: {svg}");
        assert!(svg.contains("</svg>"), "unterminated SVG preview: {svg}");
    }
    assert!(
        fs::read_to_string(directory.join("image/graphic3.svg"))
            .unwrap()
            .contains("This is a text box"),
        "textbox text should be included in its SVG preview"
    );
    assert!(
        fs::read_to_string(directory.join("image/graphic5.svg"))
            .unwrap()
            .contains("Lead"),
        "SmartArt labels should be included in its SVG preview"
    );
    assert!(
        fs::read_to_string(directory.join("image/graphic6.svg"))
            .unwrap()
            .contains("<rect"),
        "chart data should be rendered as SVG bars"
    );
    assert!(
        !source_markdown.contains("|  |  |  |  |  |  |  |  |  |  |"),
        "empty placeholder tables should not be emitted"
    );
    for expected in [
        "Текст со сноской номер 1[^1].",
        "Текст со второй сноской[^2].",
        "[^1]:  Сноска 1: Это тестовая сноска.",
        "[^2]:  Сноска 2: Вторая тестовая сноска с ссылкой на https://example.com",
        "Текст с концевой сноской.[^endnote-i]",
        "[^endnote-i]: Концевая сноска 1: Это тестовая концевая сноска.",
        "Встроенная формула: $E=mc^{2}$",
        "$$x=\\frac{\\left(-b\\pm\\sqrt{b2-4ac}\\right)}{2a}$$",
        "$$\\left[\\begin{matrix}1 & 2 \\\\ 3 & 4\\end{matrix}\\right]$$",
        "$$\\frac{a+b}{c+d}$$",
        "$$\\int_{\\infty}^{0}e-xdx=1$$",
    ] {
        assert!(
            source_markdown.contains(expected),
            "missing {expected:?} in golden docx-to-markdown output: {source_markdown:?}"
        );
    }

    let rendered = fs::read_to_string(&restored_markdown).unwrap();
    for expected in [
        "# 1. Heading Level 1 / Заголовок уровня 1",
        "[1. Heading Level 1 / Заголовок уровня 1 4](#_Toc000000002)",
        "**полужирный текст,** *курсив,* ***полужирный курсив,*** <u>подчеркнутый текст,</u> ~~зачеркнутый текст,~~ верхний индекс x$^{2}$, нижний индекс H$_{2}$O",
        "Встроенная формула: $E=mc^{2}$",
        "$$\\left[\\begin{matrix}1 & 2 \\\\ 3 & 4\\end{matrix}\\right]$$",
        "**Полужирный текст.**",
        "***Полужирный курсивный текст.***",
        "И пример имени файла: report_final_v2.docx",
        "- Элемент 1",
        "1. Первый",
        "8. Шаг первый",
        "9. Шаг второй",
        "Inline code: `const answer = 42;`",
        "```\nfunction greet(name) {\n    console.log(\"Hello, \" + name);",
        "Строка с ручным разрывом строки.  \nСледующая строка после soft line break.",
        "| ID | Name | Role | Active |",
        "\\<td>Q4\\</td>",
        "\\<td>160\\</td>",
        "\\<td>110\\</td>",
        "\\<td>50\\</td>",
        "Текст со сноской номер 1[^1].",
        "[^1]: Сноска 1: Это тестовая сноска.",
        "Текст с концевой сноской.[^endnote-i]",
        "[^endnote-i]: Концевая сноска 1: Это тестовая концевая сноска.",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in golden round trip"
        );
    }
    assert!(
        rendered.contains(".svg)"),
        "SVG previews should survive a DOCX round trip"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn golden_docx_graphics_are_embedded_in_json_yaml_and_toml() {
    use base64::Engine as _;

    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("golden/Golden_Word_Test_Document_v2.docx");
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "markoff_golden_graphics_{}_{}",
        std::process::id(),
        sequence
    ));
    fs::create_dir_all(&directory).unwrap();

    for (format, extension) in [
        (Format::Json, "json"),
        (Format::Yaml, "yaml"),
        (Format::Toml, "toml"),
    ] {
        let format_directory = directory.join(extension);
        fs::create_dir_all(&format_directory).unwrap();
        let structured = format_directory.join(format!("document.{extension}"));
        let restored_markdown = format_directory.join("restored.md");
        convert_file(&source, &structured, Format::Docx, format).unwrap();
        let source = fs::read_to_string(&structured).unwrap();
        let value: serde_json::Value = match format {
            Format::Json => serde_json::from_str(&source).unwrap(),
            Format::Yaml => serde_yaml::from_str(&source).unwrap(),
            Format::Toml => {
                serde_json::to_value(toml::from_str::<toml::Value>(&source).unwrap()).unwrap()
            }
            _ => unreachable!(),
        };
        let mut svg_data = Vec::new();
        collect_svg_data(&value, &mut svg_data);
        assert_eq!(svg_data.len(), 4, "wrong SVG image count in {extension}");
        for data in svg_data {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap();
            assert!(decoded.starts_with(b"<svg "));
            assert!(decoded.ends_with(b"</svg>"));
        }

        convert_file(&structured, &restored_markdown, format, Format::Markdown).unwrap();
        let rendered = fs::read_to_string(&restored_markdown).unwrap();
        assert!(
            rendered.contains("image/image"),
            "restored Markdown lost SVG image links from {extension}"
        );
        let restored_images = fs::read_dir(format_directory.join("image"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
            })
            .count();
        assert_eq!(
            restored_images, 4,
            "wrong restored SVG count in {extension}"
        );
    }

    fs::remove_dir_all(directory).unwrap();
}

fn collect_svg_data(value: &serde_json::Value, data: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(object) => {
            if object.get("format").and_then(serde_json::Value::as_str) == Some("svg")
                && let Some(svg) = object.get("data").and_then(serde_json::Value::as_str)
            {
                data.push(svg.to_string());
            }
            for value in object.values() {
                collect_svg_data(value, data);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_svg_data(value, data);
            }
        }
        _ => {}
    }
}

#[test]
fn golden_pdf_extracts_embedded_images_alongside_markdown() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("golden/golden_test_document.pdf");
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "markoff_golden_pdf_images_{}_{}",
        std::process::id(),
        sequence
    ));
    fs::create_dir_all(&directory).unwrap();
    let markdown = directory.join("golden.md");

    convert_file(&source, &markdown, Format::Pdf, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&markdown).unwrap();
    assert!(
        rendered
            .lines()
            .any(|line| !line.trim().is_empty() && !line.starts_with("![](")),
        "missing extracted text in {rendered:?}"
    );
    assert!(
        rendered.contains("![](image/image1.png)"),
        "missing extracted image reference in {rendered:?}"
    );

    let image_bytes = fs::read(directory.join("image").join("image1.png")).unwrap();
    assert_eq!(
        &image_bytes[..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "extracted file is not a valid PNG"
    );

    fs::remove_dir_all(directory).ok();
}
