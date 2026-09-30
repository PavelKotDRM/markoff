use crate::data::{read_data_rows, write_data_sheets};
use crate::document::parse_markdown_document;
use crate::document_model::{Block, Inline, ListItem};
use crate::error::invalid_data;
use crate::pptx::markdown_inline_to_plain_text;
use crate::tables::{markdown_table_from_rows, parse_markdown_tables};
use crate::xlsx::CellValue;
use crate::xml_utils::{attribute_value, xml_attribute_escape, xml_escape};
use crate::{Format, MarkoffError};
use base64::Engine as _;
use pulldown_cmark::{Event as MarkdownEvent, Options as MarkdownOptions, Parser};
use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesText, Event};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;

const ODT_MIME: &str = "application/vnd.oasis.opendocument.text";
const ODS_MIME: &str = "application/vnd.oasis.opendocument.spreadsheet";
const ODP_MIME: &str = "application/vnd.oasis.opendocument.presentation";
const ROUND_TRIP_BEGIN: &str = "<!-- markoff:opendocument:v1:";
const ROUND_TRIP_END: &str = ":end -->";

#[derive(Serialize, Deserialize)]
struct OpenDocumentPayload {
    format: Format,
    markdown: String,
    package: String,
}

pub(crate) fn convert_markdown_to_odt(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let (source, payload) = split_open_document_payload(&source)?;
    if restore_unchanged_package(source, payload.as_ref(), Format::Odt, output)? {
        return Ok(());
    }
    if patch_edited_package(source, payload.as_ref(), Format::Odt, output)? {
        return Ok(());
    }
    let base_dir = input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let document = parse_markdown_document(source, base_dir)?;
    let mut body = String::new();
    render_odt_blocks(&document.blocks, &mut body)?;
    let content = odf_document("text", &body, ODT_AUTOMATIC_STYLES);
    write_odf_package(output, ODT_MIME, &content)
}

pub(crate) fn convert_odt_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let content = read_odf_content(input, ODT_MIME)?;
    let markdown = odt_to_markdown(&content)?;
    std::fs::write(
        output,
        append_open_document_payload(&markdown, input, Format::Odt)?,
    )?;
    Ok(())
}

pub(crate) fn convert_markdown_to_ods(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let (source, payload) = split_open_document_payload(&source)?;
    if restore_unchanged_package(source, payload.as_ref(), Format::Ods, output)? {
        return Ok(());
    }
    if patch_edited_package(source, payload.as_ref(), Format::Ods, output)? {
        return Ok(());
    }
    let sheets = parse_markdown_tables(source)?;
    if sheets.is_empty() {
        return Err(MarkoffError::NotImplemented {
            from: Format::Markdown,
            to: Format::Ods,
        });
    }
    let sheets = sheets
        .into_iter()
        .map(|(name, rows)| {
            (
                name,
                rows.into_iter()
                    .map(|row| row.into_iter().map(CellValue::String).collect())
                    .collect(),
            )
        })
        .collect();
    write_ods_value_sheets(output, &sheets)
}

pub(crate) fn convert_data_to_ods(
    input: &Path,
    output: &Path,
    format: Format,
    delimiter: u8,
) -> Result<(), MarkoffError> {
    let mut sheets = BTreeMap::new();
    sheets.insert(
        "Sheet1".to_string(),
        read_data_rows(input, format, delimiter)?,
    );
    write_ods_value_sheets(output, &sheets)
}

pub(crate) fn convert_ods_to_data(
    input: &Path,
    output: &Path,
    format: Format,
    delimiter: u8,
) -> Result<(), MarkoffError> {
    let content = read_odf_content(input, ODS_MIME)?;
    let sheets = read_ods_value_sheets(&content)?;
    write_data_sheets(output, format, delimiter, &sheets)
}

fn write_ods_value_sheets(
    output: &Path,
    sheets: &BTreeMap<String, Vec<Vec<CellValue>>>,
) -> Result<(), MarkoffError> {
    let mut body = String::new();
    for (name, rows) in sheets {
        body.push_str(&format!(
            "<table:table table:name=\"{}\">",
            xml_attribute_escape(name)
        ));
        for row in rows {
            body.push_str("<table:table-row>");
            for cell in row {
                match cell {
                    CellValue::Empty => body.push_str("<table:table-cell/>"),
                    CellValue::Integer(value) => body.push_str(&format!(
                        "<table:table-cell office:value-type=\"float\" office:value=\"{value}\"><text:p>{value}</text:p></table:table-cell>"
                    )),
                    CellValue::Float(value) => body.push_str(&format!(
                        "<table:table-cell office:value-type=\"float\" office:value=\"{value}\"><text:p>{value}</text:p></table:table-cell>"
                    )),
                    CellValue::Boolean(value) => body.push_str(&format!(
                        "<table:table-cell office:value-type=\"boolean\" office:boolean-value=\"{value}\"><text:p>{value}</text:p></table:table-cell>"
                    )),
                    CellValue::String(value) => body.push_str(&format!(
                        "<table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>",
                        xml_escape(value)
                    )),
                }
            }
            body.push_str("</table:table-row>");
        }
        body.push_str("</table:table>");
    }
    let content = odf_document("spreadsheet", &body, "");
    write_odf_package(output, ODS_MIME, &content)
}

pub(crate) fn convert_ods_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let content = read_odf_content(input, ODS_MIME)?;
    let sheets = read_ods_value_sheets(&content)?
        .into_iter()
        .map(|(name, rows)| {
            (
                name,
                rows.into_iter()
                    .map(|row| row.iter().map(CellValue::as_text).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let markdown = sheets
        .iter()
        .map(|(name, rows)| format!("## {name}\n\n{}", markdown_table_from_rows(rows)))
        .collect::<Vec<_>>()
        .join("\n\n");
    std::fs::write(
        output,
        append_open_document_payload(&markdown, input, Format::Ods)?,
    )?;
    Ok(())
}

pub(crate) fn convert_markdown_to_odp(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let (source, payload) = split_open_document_payload(&source)?;
    if restore_unchanged_package(source, payload.as_ref(), Format::Odp, output)? {
        return Ok(());
    }
    if patch_edited_package(source, payload.as_ref(), Format::Odp, output)? {
        return Ok(());
    }
    let slides = markdown_slides(source);
    let mut body = String::new();
    for (index, slide) in slides.iter().enumerate() {
        body.push_str(&format!(
            "<draw:page draw:name=\"page{}\" draw:style-name=\"dp1\" draw:master-page-name=\"Default\">",
            index + 1
        ));
        body.push_str("<draw:frame presentation:class=\"title\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"24cm\" svg:height=\"3cm\"><draw:text-box>");
        body.push_str(&format!("<text:p>{}</text:p>", xml_escape(&slide.title)));
        body.push_str("</draw:text-box></draw:frame>");
        body.push_str("<draw:frame presentation:class=\"outline\" svg:x=\"1cm\" svg:y=\"4cm\" svg:width=\"24cm\" svg:height=\"14cm\"><draw:text-box>");
        for (text, bulleted) in &slide.items {
            if *bulleted {
                body.push_str(&format!(
                    "<text:list text:style-name=\"Bullet\"><text:list-item><text:p>{}</text:p></text:list-item></text:list>",
                    xml_escape(text)
                ));
            } else {
                body.push_str(&format!("<text:p>{}</text:p>", xml_escape(text)));
            }
        }
        body.push_str("</draw:text-box></draw:frame></draw:page>");
    }
    let content = odf_document("presentation", &body, ODP_AUTOMATIC_STYLES);
    write_odf_package(output, ODP_MIME, &content)
}

pub(crate) fn convert_odp_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let content = read_odf_content(input, ODP_MIME)?;
    let markdown = odp_to_markdown(&content)?;
    std::fs::write(
        output,
        append_open_document_payload(&markdown, input, Format::Odp)?,
    )?;
    Ok(())
}

fn append_open_document_payload(
    markdown: &str,
    input: &Path,
    format: Format,
) -> Result<String, MarkoffError> {
    let payload = OpenDocumentPayload {
        format,
        markdown: markdown.to_string(),
        package: base64::engine::general_purpose::STANDARD.encode(std::fs::read(input)?),
    };
    let json = serde_json::to_vec(&payload).map_err(invalid_data)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(json);
    Ok(format!(
        "{}\n\n{ROUND_TRIP_BEGIN}{encoded}{ROUND_TRIP_END}\n",
        markdown.trim_end()
    ))
}

fn split_open_document_payload(
    source: &str,
) -> Result<(&str, Option<OpenDocumentPayload>), MarkoffError> {
    let Some(begin) = source.rfind(ROUND_TRIP_BEGIN) else {
        return Ok((source, None));
    };
    let encoded_start = begin + ROUND_TRIP_BEGIN.len();
    let Some(relative_end) = source[encoded_start..].find(ROUND_TRIP_END) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "OpenDocument service block is missing its closing marker",
        )
        .into());
    };
    let end = encoded_start + relative_end;
    if !source[end + ROUND_TRIP_END.len()..].trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "OpenDocument service block must be the final Markdown block",
        )
        .into());
    }
    let json = base64::engine::general_purpose::STANDARD
        .decode(&source[encoded_start..end])
        .map_err(invalid_data)?;
    let payload = serde_json::from_slice(&json).map_err(invalid_data)?;
    Ok((source[..begin].trim_end(), Some(payload)))
}

fn restore_unchanged_package(
    markdown: &str,
    payload: Option<&OpenDocumentPayload>,
    format: Format,
    output: &Path,
) -> Result<bool, MarkoffError> {
    let Some(payload) = payload else {
        return Ok(false);
    };
    if payload.format != format {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "OpenDocument service block contains {}, but the requested output is {format}",
                payload.format
            ),
        )
        .into());
    }
    if markdown.trim_end() != payload.markdown.trim_end() {
        return Ok(false);
    }
    let package = base64::engine::general_purpose::STANDARD
        .decode(&payload.package)
        .map_err(invalid_data)?;
    std::fs::write(output, package)?;
    Ok(true)
}

fn patch_edited_package(
    markdown: &str,
    payload: Option<&OpenDocumentPayload>,
    format: Format,
    output: &Path,
) -> Result<bool, MarkoffError> {
    let Some(payload) = payload else {
        return Ok(false);
    };
    if payload.format != format {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "OpenDocument service block contains {}, but the requested output is {format}",
                payload.format
            ),
        )
        .into());
    }

    let old_leaves = editable_markdown_leaves(&payload.markdown, format)?;
    let new_leaves = editable_markdown_leaves(markdown, format)?;
    if old_leaves.len() != new_leaves.len() {
        return Err(unsupported_structural_edit(format));
    }
    let changed = old_leaves
        .iter()
        .zip(&new_leaves)
        .filter(|(old, new)| old != new)
        .count();
    if changed == 0 {
        return Ok(false);
    }

    let package = base64::engine::general_purpose::STANDARD
        .decode(&payload.package)
        .map_err(invalid_data)?;
    let content = read_package_part(&package, "content.xml")?;
    let patched = if format == Format::Ods {
        patch_ods_xml(&content, &old_leaves, &new_leaves)?
    } else {
        patch_xml_text(&content, &old_leaves, &new_leaves, changed)?
    };
    let Some(patched) = patched else {
        return Err(unsupported_structural_edit(format));
    };
    rewrite_package(&package, output, &patched)?;
    Ok(true)
}

fn unsupported_structural_edit(format: Format) -> MarkoffError {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "the edited Markdown structure cannot be mapped safely onto the original {format} \
             package; change text in existing elements or remove the Markoff OpenDocument \
             service block to create a new document"
        ),
    )
    .into()
}

fn editable_markdown_leaves(markdown: &str, format: Format) -> Result<Vec<String>, MarkoffError> {
    if format == Format::Ods {
        return Ok(parse_markdown_tables(markdown)?
            .into_values()
            .flatten()
            .flatten()
            .collect());
    }
    let mut leaves = Vec::new();
    for event in Parser::new_ext(markdown, MarkdownOptions::all()) {
        match event {
            MarkdownEvent::Text(text)
            | MarkdownEvent::Code(text)
            | MarkdownEvent::InlineMath(text)
            | MarkdownEvent::DisplayMath(text) => leaves.push(text.into_string()),
            _ => {}
        }
    }
    Ok(leaves)
}

fn read_package_part(package: &[u8], name: &str) -> Result<String, MarkoffError> {
    let reader = std::io::Cursor::new(package);
    let mut archive = zip::ZipArchive::new(reader).map_err(invalid_data)?;
    let mut content = String::new();
    archive
        .by_name(name)
        .map_err(invalid_data)?
        .read_to_string(&mut content)?;
    Ok(content)
}

fn patch_xml_text(
    xml: &str,
    old_leaves: &[String],
    new_leaves: &[String],
    expected_changes: usize,
) -> Result<Option<String>, MarkoffError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    let mut cursor = 0usize;
    let mut applied_changes = 0usize;
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Text(text) => {
                let value = quick_xml::escape::unescape(text.as_ref())
                    .map_err(invalid_data)?
                    .into_owned();
                let match_index = old_leaves[cursor..]
                    .iter()
                    .position(|candidate| candidate == &value)
                    .map(|index| cursor + index);
                if let Some(index) = match_index {
                    if old_leaves[index] != new_leaves[index] {
                        writer
                            .write_event(Event::Text(BytesText::new(&new_leaves[index])))
                            .map_err(invalid_data)?;
                        applied_changes += 1;
                    } else {
                        writer
                            .write_event(Event::Text(text))
                            .map_err(invalid_data)?;
                    }
                    cursor = index + 1;
                } else {
                    writer
                        .write_event(Event::Text(text))
                        .map_err(invalid_data)?;
                }
            }
            Event::Eof => break,
            event => writer.write_event(event).map_err(invalid_data)?,
        }
    }
    if applied_changes != expected_changes {
        return Ok(None);
    }
    Ok(String::from_utf8(writer.into_inner())
        .map(Some)
        .map_err(invalid_data)?)
}

fn patch_ods_xml(
    xml: &str,
    old_leaves: &[String],
    new_leaves: &[String],
) -> Result<Option<String>, MarkoffError> {
    let mut patched = xml.to_string();
    let mut cursor = 0usize;
    for (old, new) in old_leaves.iter().zip(new_leaves) {
        let old_text = format!("<text:p>{}</text:p>", xml_escape(old));
        let Some(relative_text_start) = patched[cursor..].find(&old_text) else {
            if old != new {
                return Ok(None);
            }
            continue;
        };
        let text_start = cursor + relative_text_start;
        if old == new {
            cursor = text_start + old_text.len();
            continue;
        }

        let Some(cell_start) = patched[..text_start].rfind("<table:table-cell") else {
            return Ok(None);
        };
        let Some(relative_tag_end) = patched[cell_start..text_start].find('>') else {
            return Ok(None);
        };
        let tag_end = cell_start + relative_tag_end;
        let mut opening_tag = patched[cell_start..=tag_end].to_string();
        let value_attribute = format!("office:value=\"{}\"", xml_attribute_escape(old));
        let boolean_attribute = format!("office:boolean-value=\"{}\"", xml_attribute_escape(old));
        if opening_tag.contains(&value_attribute) {
            if new.parse::<f64>().is_err() {
                return Ok(None);
            }
            opening_tag = opening_tag.replacen(
                &value_attribute,
                &format!("office:value=\"{}\"", xml_attribute_escape(new)),
                1,
            );
        } else if opening_tag.contains(&boolean_attribute) {
            if !matches!(new.as_str(), "true" | "false") {
                return Ok(None);
            }
            opening_tag = opening_tag.replacen(
                &boolean_attribute,
                &format!("office:boolean-value=\"{}\"", xml_attribute_escape(new)),
                1,
            );
        }
        patched.replace_range(cell_start..=tag_end, &opening_tag);

        let search_start = cell_start + opening_tag.len();
        let Some(relative_adjusted_text_start) = patched[search_start..].find(&old_text) else {
            return Ok(None);
        };
        let adjusted_text_start = search_start + relative_adjusted_text_start;
        let new_text = format!("<text:p>{}</text:p>", xml_escape(new));
        patched.replace_range(
            adjusted_text_start..adjusted_text_start + old_text.len(),
            &new_text,
        );
        cursor = adjusted_text_start + new_text.len();
    }
    Ok(Some(patched))
}

struct PackageEntry {
    name: String,
    data: Vec<u8>,
    compression: zip::CompressionMethod,
    directory: bool,
}

fn rewrite_package(package: &[u8], output: &Path, content_xml: &str) -> Result<(), MarkoffError> {
    let reader = std::io::Cursor::new(package);
    let mut archive = zip::ZipArchive::new(reader).map_err(invalid_data)?;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(invalid_data)?;
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        entries.push(PackageEntry {
            name: entry.name().to_string(),
            data,
            compression: entry.compression(),
            directory: entry.is_dir(),
        });
    }

    let file = std::fs::File::create(output)?;
    let mut writer = zip::ZipWriter::new(file);
    for entry in entries {
        let options = SimpleFileOptions::default().compression_method(entry.compression);
        if entry.directory {
            writer
                .add_directory(&entry.name, options)
                .map_err(invalid_data)?;
            continue;
        }
        writer
            .start_file(&entry.name, options)
            .map_err(invalid_data)?;
        if entry.name == "content.xml" {
            writer.write_all(content_xml.as_bytes())?;
        } else {
            writer.write_all(&entry.data)?;
        }
    }
    writer.finish().map_err(invalid_data)?;
    Ok(())
}

fn write_odf_package(output: &Path, mime_type: &str, content: &str) -> Result<(), MarkoffError> {
    let file = std::fs::File::create(output)?;
    let mut archive = zip::ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive
        .start_file("mimetype", stored)
        .map_err(invalid_data)?;
    archive.write_all(mime_type.as_bytes())?;

    let compressed =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    write_odf_part(&mut archive, compressed, "content.xml", content)?;
    write_odf_part(&mut archive, compressed, "styles.xml", ODF_STYLES)?;
    write_odf_part(&mut archive, compressed, "meta.xml", ODF_META)?;
    let manifest = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{mime_type}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"meta.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
    );
    write_odf_part(&mut archive, compressed, "META-INF/manifest.xml", &manifest)?;
    archive.finish().map_err(invalid_data)?;
    Ok(())
}

fn write_odf_part(
    archive: &mut zip::ZipWriter<std::fs::File>,
    options: SimpleFileOptions,
    name: &str,
    content: &str,
) -> Result<(), MarkoffError> {
    archive.start_file(name, options).map_err(invalid_data)?;
    archive.write_all(content.as_bytes())?;
    Ok(())
}

fn read_odf_content(input: &Path, expected_mime: &str) -> Result<String, MarkoffError> {
    let file = std::fs::File::open(input)?;
    let mut archive = zip::ZipArchive::new(file).map_err(invalid_data)?;
    let mut mime = String::new();
    archive
        .by_name("mimetype")
        .map_err(invalid_data)?
        .read_to_string(&mut mime)?;
    if mime.trim() != expected_mime {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unexpected OpenDocument media type {mime:?}; expected {expected_mime}"),
        )
        .into());
    }
    let mut content = String::new();
    archive
        .by_name("content.xml")
        .map_err(invalid_data)?
        .read_to_string(&mut content)?;
    Ok(content)
}

fn odf_document(kind: &str, body: &str, automatic_styles: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:presentation=\"urn:oasis:names:tc:opendocument:xmlns:presentation:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" office:version=\"1.3\"><office:automatic-styles>{automatic_styles}</office:automatic-styles><office:body><office:{kind}>{body}</office:{kind}></office:body></office:document-content>"
    )
}

fn render_odt_blocks(blocks: &[Block], output: &mut String) -> Result<(), MarkoffError> {
    for block in blocks {
        match block {
            Block::Heading {
                level,
                text,
                content,
            } => {
                output.push_str(&format!(
                    "<text:h text:outline-level=\"{}\">",
                    (*level).clamp(1, 6)
                ));
                render_odt_content(content, text.as_deref(), output)?;
                output.push_str("</text:h>");
            }
            Block::Paragraph { text, content } => {
                output.push_str("<text:p>");
                render_odt_content(content, text.as_deref(), output)?;
                output.push_str("</text:p>");
            }
            Block::List { ordered, items, .. } => render_odt_list(*ordered, items, output)?,
            Block::ListItem {
                ordered,
                text,
                content,
                ..
            } => {
                output.push_str(if *ordered {
                    "<text:list text:style-name=\"Number\">"
                } else {
                    "<text:list text:style-name=\"Bullet\">"
                });
                output.push_str("<text:list-item><text:p>");
                render_odt_content(content, text.as_deref(), output)?;
                output.push_str("</text:p></text:list-item></text:list>");
            }
            Block::Table { rows, cells, .. } => {
                output.push_str("<table:table table:name=\"Table\">");
                if cells.is_empty() {
                    for row in rows.as_deref().unwrap_or_default() {
                        output.push_str("<table:table-row>");
                        for cell in row {
                            output.push_str(&format!(
                                "<table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>",
                                xml_escape(cell)
                            ));
                        }
                        output.push_str("</table:table-row>");
                    }
                } else {
                    for row in cells {
                        output.push_str("<table:table-row>");
                        for cell in row {
                            output.push_str(
                                "<table:table-cell office:value-type=\"string\"><text:p>",
                            );
                            render_odt_content(cell, None, output)?;
                            output.push_str("</text:p></table:table-cell>");
                        }
                        output.push_str("</table:table-row>");
                    }
                }
                output.push_str("</table:table>");
            }
            Block::Code { code, .. } => {
                output.push_str("<text:p text:style-name=\"CodeBlock\">");
                render_odf_text(code, output);
                output.push_str("</text:p>");
            }
            Block::Quote { text, blocks } => {
                output.push_str(
                    "<text:section text:style-name=\"QuoteSection\" text:name=\"Quote\">",
                );
                if blocks.is_empty() {
                    output.push_str(&format!(
                        "<text:p>{}</text:p>",
                        xml_escape(text.as_deref().unwrap_or_default())
                    ));
                } else {
                    render_odt_blocks(blocks, output)?;
                }
                output.push_str("</text:section>");
            }
            Block::HorizontalRule => {
                output.push_str("<text:p text:style-name=\"HorizontalRule\"/>")
            }
            Block::Math { text } => output.push_str(&format!(
                "<text:p text:style-name=\"Math\">{}</text:p>",
                xml_escape(text)
            )),
            Block::FootnoteDefinition { label, blocks } => {
                output.push_str(&format!("<text:p>[^{}]: ", xml_escape(label)));
                if let Some(Block::Paragraph { text, content }) = blocks.first() {
                    render_odt_content(content, text.as_deref(), output)?;
                    output.push_str("</text:p>");
                    render_odt_blocks(&blocks[1..], output)?;
                } else {
                    output.push_str("</text:p>");
                    render_odt_blocks(blocks, output)?;
                }
            }
            Block::Image { alt, .. } => {
                output.push_str(&format!("<text:p>[image: {}]</text:p>", xml_escape(alt)))
            }
            Block::Html { html } => {
                output.push_str(&format!("<text:p>{}</text:p>", xml_escape(html)))
            }
        }
    }
    Ok(())
}

fn render_odt_list(
    ordered: bool,
    items: &[ListItem],
    output: &mut String,
) -> Result<(), MarkoffError> {
    output.push_str(if ordered {
        "<text:list text:style-name=\"Number\">"
    } else {
        "<text:list text:style-name=\"Bullet\">"
    });
    for item in items {
        output.push_str("<text:list-item>");
        render_odt_blocks(&item.blocks, output)?;
        output.push_str("</text:list-item>");
    }
    output.push_str("</text:list>");
    Ok(())
}

fn render_odt_content(
    content: &[Inline],
    fallback: Option<&str>,
    output: &mut String,
) -> Result<(), MarkoffError> {
    if content.is_empty() {
        render_odf_text(fallback.unwrap_or_default(), output);
        return Ok(());
    }
    if let Some(text) = fallback {
        render_odf_text(text, output);
    }
    for inline in content {
        match inline {
            Inline::Text { text } => render_odf_text(text, output),
            Inline::Emphasis { content } => render_odt_span("Italic", content, output)?,
            Inline::Strong { content } => render_odt_span("Bold", content, output)?,
            Inline::Strikethrough { content } => render_odt_span("Strike", content, output)?,
            Inline::Underline { content } => render_odt_span("Underline", content, output)?,
            Inline::Superscript { content } => render_odt_span("Superscript", content, output)?,
            Inline::Subscript { content } => render_odt_span("Subscript", content, output)?,
            Inline::Code { text } => {
                output.push_str("<text:span text:style-name=\"Code\">");
                render_odf_text(text, output);
                output.push_str("</text:span>");
            }
            Inline::Math { text, display } => {
                output.push_str(if *display { "$$" } else { "$" });
                render_odf_text(text.trim(), output);
                output.push_str(if *display { "$$" } else { "$" });
            }
            Inline::Link {
                destination,
                content,
                ..
            } => {
                output.push_str(&format!(
                    "<text:a xlink:href=\"{}\">",
                    xml_attribute_escape(destination)
                ));
                render_odt_content(content, None, output)?;
                output.push_str("</text:a>");
            }
            Inline::Image {
                alt, destination, ..
            } => render_odf_text(if alt.is_empty() { destination } else { alt }, output),
            Inline::FootnoteReference { label } => render_odf_text(&format!("[^{label}]"), output),
            Inline::Footnote { content } => {
                output.push_str("^[");
                render_odt_content(content, None, output)?;
                output.push(']');
            }
            Inline::Bookmark { name } => output.push_str(&format!(
                "<text:bookmark text:name=\"{}\"/>",
                xml_attribute_escape(name)
            )),
            Inline::SoftBreak => output.push(' '),
            Inline::HardBreak => output.push_str("<text:line-break/>"),
            Inline::TaskListMarker { checked } => {
                output.push_str(if *checked { "☒ " } else { "☐ " })
            }
            Inline::Html { html } => render_odf_text(html, output),
        }
    }
    Ok(())
}

fn render_odt_span(
    style: &str,
    content: &[Inline],
    output: &mut String,
) -> Result<(), MarkoffError> {
    output.push_str(&format!("<text:span text:style-name=\"{style}\">"));
    render_odt_content(content, None, output)?;
    output.push_str("</text:span>");
    Ok(())
}

fn render_odf_text(text: &str, output: &mut String) {
    for (index, part) in text.split('\n').enumerate() {
        if index > 0 {
            output.push_str("<text:line-break/>");
        }
        output.push_str(&xml_escape(part));
    }
}

#[derive(Default)]
struct OdtReader {
    output: String,
    text: String,
    heading: Option<usize>,
    paragraph: bool,
    paragraph_style: String,
    quote_depth: usize,
    list_styles: Vec<bool>,
    list_item: bool,
    table_rows: Vec<Vec<String>>,
    table_row: Vec<String>,
    in_cell: bool,
    spans: Vec<String>,
    links: Vec<String>,
    style_markers: HashMap<String, (String, String)>,
    current_style: Option<String>,
}

fn odt_to_markdown(xml: &str) -> Result<String, MarkoffError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut state = OdtReader::default();
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "style" => {
                    state.current_style = attribute_value(&tag, "name")?;
                }
                "text-properties" => {
                    if let Some(name) = state.current_style.clone() {
                        let bold = attribute_value(&tag, "font-weight")?
                            .is_some_and(|value| value.eq_ignore_ascii_case("bold"));
                        let italic = attribute_value(&tag, "font-style")?
                            .is_some_and(|value| value.eq_ignore_ascii_case("italic"));
                        let strike = attribute_value(&tag, "text-line-through-style")?
                            .is_some_and(|value| !value.eq_ignore_ascii_case("none"));
                        let underline = attribute_value(&tag, "text-underline-style")?
                            .is_some_and(|value| !value.eq_ignore_ascii_case("none"));
                        let position = attribute_value(&tag, "text-position")?
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                        let code = attribute_value(&tag, "font-name")?
                            .is_some_and(|value| value.to_ascii_lowercase().contains("mono"));
                        state.style_markers.insert(
                            name,
                            odt_property_markers(
                                bold,
                                italic,
                                strike,
                                underline,
                                position.as_str(),
                                code,
                            ),
                        );
                    }
                }
                "h" => {
                    state.heading = attribute_value(&tag, "outline-level")?
                        .and_then(|value| value.parse().ok())
                        .or(Some(1));
                    state.text.clear();
                }
                "p" => {
                    state.paragraph = true;
                    state.paragraph_style =
                        attribute_value(&tag, "style-name")?.unwrap_or_default();
                    state.text.clear();
                }
                "section" => {
                    if attribute_value(&tag, "style-name")?
                        .is_some_and(|style| style.eq_ignore_ascii_case("QuoteSection"))
                    {
                        state.quote_depth += 1;
                    }
                }
                "list" => {
                    let ordered = attribute_value(&tag, "style-name")?
                        .is_some_and(|style| style.to_ascii_lowercase().contains("number"));
                    state.list_styles.push(ordered);
                }
                "list-item" => state.list_item = true,
                "table" => state.table_rows.clear(),
                "table-row" => state.table_row.clear(),
                "table-cell" => {
                    state.in_cell = true;
                    state.text.clear();
                }
                "span" => {
                    let style = attribute_value(&tag, "style-name")?.unwrap_or_default();
                    let (opening, closing) =
                        state.style_markers.get(&style).cloned().unwrap_or_else(|| {
                            let (opening, closing) = odt_style_markers(&style);
                            (opening.to_string(), closing.to_string())
                        });
                    state.text.push_str(&opening);
                    state.spans.push(closing);
                }
                "a" => {
                    let href = attribute_value(&tag, "href")?.unwrap_or_default();
                    state.links.push(href);
                    state.text.push('[');
                }
                "line-break" => state.text.push_str("  \n"),
                "tab" => state.text.push('\t'),
                "bookmark" | "bookmark-start" => {
                    if let Some(name) = attribute_value(&tag, "name")? {
                        state
                            .text
                            .push_str(&format!("<a id=\"{}\"></a>", xml_attribute_escape(&name)));
                    }
                }
                _ => {}
            },
            Event::Empty(tag) => match tag.local_name().as_ref() {
                "text-properties" => {
                    if let Some(name) = state.current_style.clone() {
                        let bold = attribute_value(&tag, "font-weight")?
                            .is_some_and(|value| value.eq_ignore_ascii_case("bold"));
                        let italic = attribute_value(&tag, "font-style")?
                            .is_some_and(|value| value.eq_ignore_ascii_case("italic"));
                        let strike = attribute_value(&tag, "text-line-through-style")?
                            .is_some_and(|value| !value.eq_ignore_ascii_case("none"));
                        let underline = attribute_value(&tag, "text-underline-style")?
                            .is_some_and(|value| !value.eq_ignore_ascii_case("none"));
                        let position = attribute_value(&tag, "text-position")?
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                        let code = attribute_value(&tag, "font-name")?
                            .is_some_and(|value| value.to_ascii_lowercase().contains("mono"));
                        state.style_markers.insert(
                            name,
                            odt_property_markers(
                                bold,
                                italic,
                                strike,
                                underline,
                                position.as_str(),
                                code,
                            ),
                        );
                    }
                }
                "p" if attribute_value(&tag, "style-name")?
                    .is_some_and(|style| style.eq_ignore_ascii_case("HorizontalRule")) =>
                {
                    push_markdown_block(&mut state.output, "---");
                }
                "line-break" => state.text.push_str("  \n"),
                "tab" => state.text.push('\t'),
                "bookmark" | "bookmark-start" => {
                    if let Some(name) = attribute_value(&tag, "name")? {
                        state
                            .text
                            .push_str(&format!("<a id=\"{}\"></a>", xml_attribute_escape(&name)));
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                state
                    .text
                    .push_str(&quick_xml::escape::unescape(text.as_ref()).map_err(invalid_data)?);
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                "style" => state.current_style = None,
                "span" => {
                    if let Some(marker) = state.spans.pop() {
                        state.text.push_str(&marker);
                    }
                }
                "a" => {
                    if let Some(href) = state.links.pop() {
                        state.text.push_str(&format!("]({href})"));
                    }
                }
                "h" => {
                    let level = state.heading.take().unwrap_or(1).clamp(1, 6);
                    push_markdown_block(
                        &mut state.output,
                        &format!("{} {}", "#".repeat(level), state.text.trim()),
                    );
                    state.text.clear();
                }
                "p" => {
                    state.paragraph = false;
                    if state.in_cell {
                    } else if state.list_item {
                        let depth = state.list_styles.len().saturating_sub(1);
                        let marker = if state.list_styles.last().copied().unwrap_or(false) {
                            "1. "
                        } else {
                            "- "
                        };
                        state.output.push_str(&"    ".repeat(depth));
                        state.output.push_str(marker);
                        let text = state.text.trim();
                        if let Some(rest) = text.strip_prefix("☒ ") {
                            state.output.push_str("[x] ");
                            state.output.push_str(rest);
                        } else if let Some(rest) = text.strip_prefix("☐ ") {
                            state.output.push_str("[ ] ");
                            state.output.push_str(rest);
                        } else {
                            state.output.push_str(text);
                        }
                        state.output.push('\n');
                        state.text.clear();
                    } else {
                        let mut text = state.text.trim().to_string();
                        if let Some(rest) = text.strip_prefix("☒ ") {
                            text = format!("[x] {rest}");
                        } else if let Some(rest) = text.strip_prefix("☐ ") {
                            text = format!("[ ] {rest}");
                        }
                        text = match state.paragraph_style.to_ascii_lowercase().as_str() {
                            "codeblock" => format!("```\n{text}\n```"),
                            "math" => format!("$$\n{text}\n$$"),
                            _ if state.quote_depth > 0 => text
                                .lines()
                                .map(|line| format!("{}{}", "> ".repeat(state.quote_depth), line))
                                .collect::<Vec<_>>()
                                .join("\n"),
                            _ => text,
                        };
                        push_markdown_block(&mut state.output, &text);
                        state.text.clear();
                    }
                    state.paragraph_style.clear();
                }
                "section" => state.quote_depth = state.quote_depth.saturating_sub(1),
                "list-item" => state.list_item = false,
                "list" => {
                    state.list_styles.pop();
                    if state.list_styles.is_empty() {
                        state.output.push('\n');
                    }
                }
                "table-cell" => {
                    state.table_row.push(state.text.trim().to_string());
                    state.text.clear();
                    state.in_cell = false;
                }
                "table-row" => state.table_rows.push(std::mem::take(&mut state.table_row)),
                "table" => {
                    let table = markdown_table_from_rows(&state.table_rows);
                    push_markdown_block(&mut state.output, &table);
                    state.table_rows.clear();
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(format!("{}\n", state.output.trim_end()))
}

fn odt_style_markers(style: &str) -> (&'static str, &'static str) {
    match style.to_ascii_lowercase().as_str() {
        "bold" | "strong" => ("**", "**"),
        "italic" | "emphasis" => ("*", "*"),
        "strike" | "strikethrough" => ("~~", "~~"),
        "underline" => ("<u>", "</u>"),
        "superscript" => ("$^{", "}$"),
        "subscript" => ("$_{", "}$"),
        "code" => ("`", "`"),
        _ => ("", ""),
    }
}

fn odt_property_markers(
    bold: bool,
    italic: bool,
    strike: bool,
    underline: bool,
    position: &str,
    code: bool,
) -> (String, String) {
    let mut opening = String::new();
    let mut closing = String::new();
    if underline {
        opening.push_str("<u>");
        closing.insert_str(0, "</u>");
    }
    if strike {
        opening.push_str("~~");
        closing.insert_str(0, "~~");
    }
    if bold && italic {
        opening.push_str("***");
        closing.insert_str(0, "***");
    } else if bold {
        opening.push_str("**");
        closing.insert_str(0, "**");
    } else if italic {
        opening.push('*');
        closing.insert(0, '*');
    }
    if position.starts_with("super") {
        opening.push_str("$^{");
        closing.insert_str(0, "}$");
    } else if position.starts_with("sub") {
        opening.push_str("$_{");
        closing.insert_str(0, "}$");
    }
    if code {
        opening.push('`');
        closing.insert(0, '`');
    }
    (opening, closing)
}

fn push_markdown_block(output: &mut String, block: &str) {
    if block.is_empty() {
        return;
    }
    if !output.is_empty() && !output.ends_with("\n\n") {
        output.push_str(if output.ends_with('\n') { "\n" } else { "\n\n" });
    }
    output.push_str(block);
    output.push_str("\n\n");
}

fn read_ods_value_sheets(xml: &str) -> Result<BTreeMap<String, Vec<Vec<CellValue>>>, MarkoffError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut sheets = BTreeMap::new();
    let mut sheet_name = None;
    let mut rows = Vec::new();
    let mut row: Vec<CellValue> = Vec::new();
    let mut cell = String::new();
    let mut in_cell = false;
    let mut repeated_columns = 1usize;
    let mut value_type = String::new();
    let mut stored_value = None;
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "table" => {
                    sheet_name = Some(
                        attribute_value(&tag, "name")?.unwrap_or_else(|| "Sheet1".to_string()),
                    );
                    rows.clear();
                }
                "table-row" => row.clear(),
                "table-cell" | "covered-table-cell" => {
                    in_cell = true;
                    cell.clear();
                    repeated_columns = attribute_value(&tag, "number-columns-repeated")?
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(1);
                    value_type = attribute_value(&tag, "value-type")?.unwrap_or_default();
                    stored_value =
                        attribute_value(&tag, "value")?.or(attribute_value(&tag, "boolean-value")?);
                }
                "line-break" if in_cell => cell.push('\n'),
                _ => {}
            },
            Event::Empty(tag)
                if matches!(
                    tag.local_name().as_ref(),
                    "table-cell" | "covered-table-cell"
                ) =>
            {
                let repeated = attribute_value(&tag, "number-columns-repeated")?
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1);
                row.extend(std::iter::repeat_n(CellValue::Empty, repeated));
            }
            Event::Text(text) if in_cell => {
                cell.push_str(&quick_xml::escape::unescape(text.as_ref()).map_err(invalid_data)?);
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                "table-cell" | "covered-table-cell" => {
                    let value = match value_type.as_str() {
                        "float" | "currency" | "percentage" => stored_value
                            .as_deref()
                            .and_then(|value| value.parse::<f64>().ok())
                            .map(|value| {
                                if value.fract() == 0.0
                                    && value >= i64::MIN as f64
                                    && value <= i64::MAX as f64
                                {
                                    CellValue::Integer(value as i64)
                                } else {
                                    CellValue::Float(value)
                                }
                            })
                            .unwrap_or_else(|| CellValue::String(cell.trim().to_string())),
                        "boolean" => stored_value
                            .as_deref()
                            .and_then(|value| value.parse().ok())
                            .map(CellValue::Boolean)
                            .unwrap_or_else(|| CellValue::String(cell.trim().to_string())),
                        _ if cell.is_empty() => CellValue::Empty,
                        _ => CellValue::String(cell.trim().to_string()),
                    };
                    row.extend(std::iter::repeat_n(value, repeated_columns));
                    in_cell = false;
                    stored_value = None;
                    value_type.clear();
                }
                "table-row" => rows.push(std::mem::take(&mut row)),
                "table" => {
                    if let Some(name) = sheet_name.take() {
                        sheets.insert(name, std::mem::take(&mut rows));
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(sheets)
}

struct Slide {
    title: String,
    items: Vec<(String, bool)>,
}

fn markdown_slides(source: &str) -> Vec<Slide> {
    let mut slides = Vec::new();
    let mut current = None;
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line == "---" {
            continue;
        }
        if let Some(title) = line.strip_prefix("# ").or_else(|| line.strip_prefix("## ")) {
            if let Some(slide) = current.take() {
                slides.push(slide);
            }
            current = Some(Slide {
                title: markdown_inline_to_plain_text(title),
                items: Vec::new(),
            });
            continue;
        }
        let (text, bullet) =
            if let Some(text) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
                (text, true)
            } else {
                (line, false)
            };
        current
            .get_or_insert_with(|| Slide {
                title: String::new(),
                items: Vec::new(),
            })
            .items
            .push((markdown_inline_to_plain_text(text), bullet));
    }
    if let Some(slide) = current {
        slides.push(slide);
    }
    if slides.is_empty() {
        slides.push(Slide {
            title: String::new(),
            items: Vec::new(),
        });
    }
    slides
}

fn odp_to_markdown(xml: &str) -> Result<String, MarkoffError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut slides = Vec::new();
    let mut title = String::new();
    let mut items = Vec::new();
    let mut frame_class = String::new();
    let mut text = String::new();
    let mut in_paragraph = false;
    let mut list_depth = 0usize;
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "page" => {
                    title.clear();
                    items.clear();
                }
                "frame" => frame_class = attribute_value(&tag, "class")?.unwrap_or_default(),
                "list" => list_depth += 1,
                "p" | "h" => {
                    in_paragraph = true;
                    text.clear();
                }
                "line-break" if in_paragraph => text.push(' '),
                _ => {}
            },
            Event::Text(value) if in_paragraph => {
                text.push_str(&quick_xml::escape::unescape(value.as_ref()).map_err(invalid_data)?);
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                "p" | "h" => {
                    in_paragraph = false;
                    let value = text.trim().to_string();
                    if !value.is_empty() {
                        if frame_class == "title" && title.is_empty() {
                            title = value;
                        } else {
                            items.push((value, list_depth > 0));
                        }
                    }
                }
                "list" => list_depth = list_depth.saturating_sub(1),
                "frame" => frame_class.clear(),
                "page" => slides.push(Slide {
                    title: std::mem::take(&mut title),
                    items: std::mem::take(&mut items),
                }),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    let rendered = slides
        .iter()
        .enumerate()
        .map(|(index, slide)| {
            let heading = if slide.title.is_empty() {
                format!("Slide {}", index + 1)
            } else {
                slide.title.clone()
            };
            let mut section = format!("## {heading}\n\n");
            for (text, bullet) in &slide.items {
                if *bullet {
                    section.push_str("- ");
                }
                section.push_str(text);
                section.push_str("\n\n");
            }
            section
        })
        .collect::<Vec<_>>()
        .join("---\n\n");
    Ok(rendered)
}

const ODT_AUTOMATIC_STYLES: &str = r#"
<style:style style:name="Bold" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
<style:style style:name="Italic" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>
<style:style style:name="Strike" style:family="text"><style:text-properties style:text-line-through-style="solid"/></style:style>
<style:style style:name="Underline" style:family="text"><style:text-properties style:text-underline-style="solid"/></style:style>
<style:style style:name="Superscript" style:family="text"><style:text-properties style:text-position="super 58%"/></style:style>
<style:style style:name="Subscript" style:family="text"><style:text-properties style:text-position="sub 58%"/></style:style>
<style:style style:name="Code" style:family="text"><style:text-properties style:font-name="Liberation Mono"/></style:style>
<style:style style:name="CodeBlock" style:family="paragraph"><style:text-properties style:font-name="Liberation Mono"/></style:style>
<style:style style:name="QuoteSection" style:family="section"/>
<style:style style:name="HorizontalRule" style:family="paragraph"><style:paragraph-properties fo:border-bottom="0.02cm solid #000000"/></style:style>
<style:style style:name="Math" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>
<text:list-style style:name="Bullet"><text:list-level-style-bullet text:level="1" text:bullet-char="•"/></text:list-style>
<text:list-style style:name="Number"><text:list-level-style-number text:level="1" style:num-format="1"/></text:list-style>
"#;

const ODP_AUTOMATIC_STYLES: &str = r#"
<style:style style:name="dp1" style:family="drawing-page"/>
<text:list-style style:name="Bullet"><text:list-level-style-bullet text:level="1" text:bullet-char="•"/></text:list-style>
"#;

const ODF_STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" office:version="1.3"><office:styles/><office:automatic-styles/><office:master-styles/></office:document-styles>"#;
const ODF_META: &str = r#"<?xml version="1.0" encoding="UTF-8"?><office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" office:version="1.3"><office:meta><meta:generator>markoff</meta:generator></office:meta></office:document-meta>"#;

#[cfg(test)]
mod tests {
    use super::odt_to_markdown;

    #[test]
    fn reads_odt_formatting_from_automatic_style_properties() {
        let xml = r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"><office:automatic-styles><style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-style="italic"/></style:style></office:automatic-styles><office:body><office:text><text:p>A <text:span text:style-name="T1">formatted</text:span> value.</text:p></office:text></office:body></office:document-content>"#;

        assert_eq!(odt_to_markdown(xml).unwrap(), "A ***formatted*** value.\n");
    }
}
