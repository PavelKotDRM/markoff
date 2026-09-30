use super::unique_temp_path;
use crate::{
    ConversionRequest, Format, MarkoffError, convert_document, convert_file, detect_format,
};
use std::fs;

#[test]
fn detects_known_formats() {
    assert!(matches!(detect_format("report.md"), Ok(Format::Markdown)));
    assert!(matches!(detect_format("report.pdf"), Ok(Format::Pdf)));
    assert!(matches!(detect_format("sheet.xlsx"), Ok(Format::Xlsx)));
    assert!(matches!(detect_format("records.csv"), Ok(Format::Csv)));
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
