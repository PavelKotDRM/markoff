use super::unique_temp_path;
use crate::{
    ConversionRequest, Format, MarkoffError, convert_document, convert_file, detect_format,
};
use std::fs;

fn extract_pdf_text(input: &std::path::Path) -> String {
    let output = unique_temp_path("pdf_text_extract");
    convert_file(input, &output, Format::Pdf, Format::Markdown).unwrap();
    let text = fs::read_to_string(&output).unwrap();
    fs::remove_file(output).ok();
    text
}

#[test]
fn detects_known_formats() {
    assert!(matches!(detect_format("report.md"), Ok(Format::Markdown)));
    assert!(matches!(detect_format("report.pdf"), Ok(Format::Pdf)));
    assert!(matches!(detect_format("sheet.xlsx"), Ok(Format::Xlsx)));
    assert!(matches!(detect_format("records.csv"), Ok(Format::Csv)));
}

#[test]
fn pdf_is_a_document_target_for_tables_only_conversions() {
    assert!(crate::supports_tables_only(Format::Json, Format::Pdf));
    assert!(crate::supports_tables_only(Format::Yaml, Format::Pdf));
    assert!(crate::supports_tables_only(Format::Toml, Format::Pdf));
    assert!(!crate::supports_tables_only(Format::Markdown, Format::Pdf));
}

#[test]
fn rejects_unknown_formats() {
    assert!(detect_format("archive.bin").is_err());
}

#[test]
fn rejects_existing_output_without_overwrite() {
    let input = unique_temp_path("overwrite_input");
    let output = unique_temp_path("overwrite_output");
    fs::write(&input, r#"{"name":"Ada"}"#).unwrap();
    fs::write(&output, "pre-existing content").unwrap();

    let request = ConversionRequest {
        input: input.clone(),
        output: output.clone(),
        from: Format::Json,
        to: Format::Markdown,
        overwrite: false,
        csv_delimiter: b',',
        tables_only: false,
    };

    assert!(matches!(
        convert_document(&request),
        Err(MarkoffError::OutputExists { .. })
    ));
    assert_eq!(fs::read_to_string(&output).unwrap(), "pre-existing content");

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn overwrites_existing_output_when_requested() {
    let input = unique_temp_path("overwrite_allowed_input");
    let output = unique_temp_path("overwrite_allowed_output");
    fs::write(&input, r#"{"blocks":[{"type":"paragraph","text":"Ada"}]}"#).unwrap();
    fs::write(&output, "pre-existing content").unwrap();

    let request = ConversionRequest {
        input: input.clone(),
        output: output.clone(),
        from: Format::Json,
        to: Format::Markdown,
        overwrite: true,
        csv_delimiter: b',',
        tables_only: false,
    };

    convert_document(&request).unwrap();
    assert!(fs::read_to_string(&output).unwrap().contains("Ada"));

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn converts_json_to_markdown() {
    let input = unique_temp_path("json_to_markdown_input");
    let output = unique_temp_path("json_to_markdown_output");
    fs::write(
        &input,
        r#"{"blocks":[{"type":"heading","level":1,"text":"Ada"},{"type":"paragraph","text":"count 42"}]}"#,
    )
    .unwrap();

    convert_file(&input, &output, Format::Json, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&output).unwrap();
    assert!(rendered.contains("# Ada"));
    assert!(rendered.contains("count 42"));

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn converts_pdf_to_markdown() {
    let input = unique_temp_path("pdf_to_markdown_input");
    let output = unique_temp_path("pdf_to_markdown_output");
    let content = b"BT /F1 12 Tf 72 720 Td (Hello from PDF) Tj ET";
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".as_slice(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".as_slice(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".as_slice(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    offsets.push(pdf.len());
    pdf.extend_from_slice(b"5 0 obj\n<< /Length ");
    pdf.extend_from_slice(content.len().to_string().as_bytes());
    pdf.extend_from_slice(b" >>\nstream\n");
    pdf.extend_from_slice(content);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let xref_offset = pdf.len();
    pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes(),
    );
    fs::write(&input, pdf).unwrap();

    convert_file(&input, &output, Format::Pdf, Format::Markdown).unwrap();

    assert!(
        fs::read_to_string(&output)
            .unwrap()
            .contains("Hello from PDF")
    );

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn converts_markdown_to_json() {
    let input = unique_temp_path("markdown_to_json_input");
    let output = unique_temp_path("markdown_to_json_output");
    fs::write(&input, "# Example\n\nThis is a markdown note.").unwrap();

    convert_file(&input, &output, Format::Markdown, Format::Json).unwrap();

    let rendered = fs::read_to_string(&output).unwrap();
    assert!(rendered.contains("Example"));
    assert!(rendered.contains("markdown note"));

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn converts_csv_to_markdown() {
    let input = unique_temp_path("csv_to_markdown_input");
    let output = unique_temp_path("csv_to_markdown_output");
    fs::write(&input, "name,age\nAda,42\nBob,30\n").unwrap();

    convert_file(&input, &output, Format::Csv, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&output).unwrap();
    assert!(rendered.contains("| name | age |"));
    assert!(rendered.contains("| Ada | 42 |"));

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn converts_markdown_to_csv() {
    let input = unique_temp_path("markdown_to_csv_input");
    let output = unique_temp_path("markdown_to_csv_output");
    fs::write(
        &input,
        "| name | age |\n| --- | --- |\n| Ada | 42 |\n| Bob | 30 |\n",
    )
    .unwrap();

    convert_file(&input, &output, Format::Markdown, Format::Csv).unwrap();

    let rendered = fs::read_to_string(&output).unwrap();
    assert!(rendered.contains("name,age"));
    assert!(rendered.contains("Ada,42"));

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn converts_yaml_to_markdown() {
    let input = unique_temp_path("yaml_to_markdown_input");
    let output = unique_temp_path("yaml_to_markdown_output");
    fs::write(
        &input,
        "blocks:\n  - type: paragraph\n    text: Ada count 42\n",
    )
    .unwrap();

    convert_file(&input, &output, Format::Yaml, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&output).unwrap();
    assert!(rendered.contains("Ada count 42"));

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn converts_toml_to_markdown() {
    let input = unique_temp_path("toml_to_markdown_input");
    let output = unique_temp_path("toml_to_markdown_output");
    fs::write(
        &input,
        "[[blocks]]\ntype = \"paragraph\"\ntext = \"Ada count 42\"\n",
    )
    .unwrap();

    convert_file(&input, &output, Format::Toml, Format::Markdown).unwrap();

    let rendered = fs::read_to_string(&output).unwrap();
    assert!(rendered.contains("Ada count 42"));

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn converts_markdown_to_pdf_with_cyrillic_text() {
    let input = unique_temp_path("markdown_to_pdf_input");
    let output = unique_temp_path("markdown_to_pdf_output");
    fs::write(
        &input,
        "# Отчёт\n\nПривет, **мир**!\n\n| Имя | Значение |\n| --- | --- |\n| Алиса | 42 |\n",
    )
    .unwrap();

    convert_file(&input, &output, Format::Markdown, Format::Pdf).unwrap();

    assert!(fs::read(&output).unwrap().starts_with(b"%PDF-"));
    let extracted = extract_pdf_text(&output);
    assert!(extracted.contains("Отчёт"));
    assert!(extracted.contains("Привет, мир!"));
    assert!(extracted.contains("Алиса"));

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn paginates_long_markdown_documents_to_pdf() {
    let input = unique_temp_path("long_markdown_to_pdf_input");
    let output = unique_temp_path("long_markdown_to_pdf_output");
    let paragraphs = (0..100)
        .map(|index| format!("Paragraph {index}: page layout is preserved."))
        .collect::<Vec<_>>()
        .join("\n\n");
    fs::write(&input, paragraphs).unwrap();

    convert_file(&input, &output, Format::Markdown, Format::Pdf).unwrap();

    let document = lopdf::Document::load(&output).unwrap();
    assert!(document.get_pages().len() > 1);
    assert!(extract_pdf_text(&output).contains("Paragraph 99: page layout is preserved."));

    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn converts_docx_to_pdf() {
    let markdown = unique_temp_path("docx_to_pdf_source");
    let docx = unique_temp_path("docx_to_pdf_input");
    let pdf = unique_temp_path("docx_to_pdf_output");
    fs::write(
        &markdown,
        "# Word report\n\nDOCX text is rendered in PDF.\n",
    )
    .unwrap();
    convert_file(&markdown, &docx, Format::Markdown, Format::Docx).unwrap();

    convert_file(&docx, &pdf, Format::Docx, Format::Pdf).unwrap();

    let extracted = extract_pdf_text(&pdf);
    assert!(extracted.contains("Word report"));
    assert!(extracted.contains("DOCX text is rendered in PDF."));

    fs::remove_file(markdown).ok();
    fs::remove_file(docx).ok();
    fs::remove_file(pdf).ok();
}

#[test]
fn converts_structured_document_schemas_to_pdf() {
    let cases = [
        (
            "json",
            Format::Json,
            r#"{"blocks":[{"type":"heading","level":1,"text":"JSON документ"},{"type":"paragraph","text":"Содержимое JSON"}]}"#,
            "Содержимое JSON",
        ),
        (
            "yaml",
            Format::Yaml,
            "blocks:\n  - type: paragraph\n    text: Содержимое YAML\n",
            "Содержимое YAML",
        ),
        (
            "toml",
            Format::Toml,
            "[[blocks]]\ntype = \"paragraph\"\ntext = \"Содержимое TOML\"\n",
            "Содержимое TOML",
        ),
    ];

    for (name, format, source, expected) in cases {
        let input = unique_temp_path(&format!("{name}_document_to_pdf_input"));
        let output = unique_temp_path(&format!("{name}_document_to_pdf_output"));
        fs::write(&input, source).unwrap();

        convert_file(&input, &output, format, Format::Pdf).unwrap();

        assert!(
            extract_pdf_text(&output).contains(expected),
            "PDF from {name} did not contain {expected:?}"
        );
        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }
}

#[test]
fn converts_only_table_blocks_from_structured_data_to_pdf() {
    let input = unique_temp_path("tables_only_to_pdf_input");
    let output = unique_temp_path("tables_only_to_pdf_output");
    fs::write(
        &input,
        r#"{"blocks":[{"type":"heading","level":1,"text":"Report title"},{"type":"table","rows":[["Name","Value"],["Ada","42"]]}]}"#,
    )
    .unwrap();
    let request = ConversionRequest {
        input: input.clone(),
        output: output.clone(),
        from: Format::Json,
        to: Format::Pdf,
        overwrite: false,
        csv_delimiter: b',',
        tables_only: true,
    };

    convert_document(&request).unwrap();

    let extracted = extract_pdf_text(&output);
    assert!(extracted.contains("Ada"));
    assert!(!extracted.contains("Report title"));
    fs::remove_file(input).ok();
    fs::remove_file(output).ok();
}

#[test]
fn converts_raw_structured_data_to_pdf_as_source_code() {
    let cases = [
        ("json", Format::Json, r#"{"name":"Ада","score":42}"#, "Ада"),
        ("yaml", Format::Yaml, "name: Грейс\nscore: 30\n", "Грейс"),
        (
            "toml",
            Format::Toml,
            "name = \"Алан\"\nscore = 25\n",
            "Алан",
        ),
    ];

    for (name, format, source, expected) in cases {
        let input = unique_temp_path(&format!("{name}_data_to_pdf_input"));
        let output = unique_temp_path(&format!("{name}_data_to_pdf_output"));
        fs::write(&input, source).unwrap();

        convert_file(&input, &output, format, Format::Pdf).unwrap();

        assert!(
            extract_pdf_text(&output).contains(expected),
            "PDF from raw {name} data did not contain {expected:?}"
        );
        fs::remove_file(input).ok();
        fs::remove_file(output).ok();
    }
}
