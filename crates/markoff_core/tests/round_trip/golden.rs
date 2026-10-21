use super::{TEMP_FILE_SEQUENCE, remove_files, temporary_path};
use markoff_core::{Format, convert_file};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

#[test]
fn golden_document_round_trip_preserves_core_markdown_content() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("golden/Golden_Word_Test_Document_v2.docx");
    let markdown = temporary_path("golden_document", "md");
    let restored_document = temporary_path("golden_document", "docx");
    let restored_markdown = temporary_path("golden_document_restored", "md");

    convert_file(&source, &markdown, Format::Docx, Format::Markdown).unwrap();
    convert_file(
        &markdown,
        &restored_document,
        Format::Markdown,
        Format::Docx,
    )
    .unwrap();
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
    assert!(
        !source_markdown.contains("|  |  |  |  |  |  |  |  |  |  |"),
        "empty placeholder tables should not be emitted"
    );
    for expected in [
        "Текст со сноской номер 1[^1].",
        "Текст со второй сноской[^2].",
        "[^1]:  Сноска 1: Это тестовая сноска.",
        "[^2]:  Сноска 2: Вторая тестовая сноска с ссылкой на https://example.com",
        "Текст с концевой сноской.[^endnote-1]",
        "[^endnote-1]: Концевая сноска 1: Это тестовая концевая сноска.",
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
        "Текст с концевой сноской.[^3]",
        "[^3]: Концевая сноска 1: Это тестовая концевая сноска.",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in golden round trip"
        );
    }

    remove_files(&[&markdown, &restored_document, &restored_markdown]);
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
