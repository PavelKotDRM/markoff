use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::{CellValue, cell_value_from_float};
use crate::MarkoffError;
use crate::error::invalid_data;
use crate::xml_utils::attribute_value;

fn required_attribute(tag: &BytesStart<'_>, name: &str) -> Result<String, MarkoffError> {
    attribute_value(tag, name)?
        .ok_or_else(|| invalid_data(format!("XLSX element is missing attribute {name}")).into())
}

fn read_part(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<String, MarkoffError> {
    let mut xml = String::new();
    archive
        .by_name(name)
        .map_err(|error| invalid_data(format!("Cannot read XLSX package part {name}: {error}")))?
        .read_to_string(&mut xml)?;
    validate_xml(&xml)?;
    Ok(xml)
}

fn validate_xml(xml: &str) -> Result<(), MarkoffError> {
    let mut reader = Reader::from_str(xml);
    let mut depth = 0usize;
    let mut root_seen = false;
    loop {
        let event = reader.read_event().map_err(invalid_data)?;
        match event {
            Event::Start(ref tag) | Event::Empty(ref tag) => {
                for attribute in tag.attributes() {
                    attribute.map_err(invalid_data)?;
                }
                if depth == 0 {
                    if root_seen {
                        return Err(invalid_data("Multiple XLSX XML roots").into());
                    }
                    root_seen = true;
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid_data("Unexpected XLSX XML closing tag"))?
            }
            Event::Text(text) if depth == 0 && !text.as_ref().trim().is_empty() => {
                return Err(invalid_data("Unexpected text outside XLSX XML root").into());
            }
            Event::Eof if depth != 0 || !root_seen => {
                return Err(invalid_data("Truncated or empty XLSX XML part").into());
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

fn resolve_part(base: &str, target: &str) -> Result<String, MarkoffError> {
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        base.split('/').filter(|part| !part.is_empty()).collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts
                    .pop()
                    .ok_or_else(|| invalid_data("XLSX relationship escapes package root"))?;
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

struct Relationship {
    target: String,
    kind: String,
}

fn relationships(xml: &str, base: &str) -> Result<BTreeMap<String, Relationship>, MarkoffError> {
    let mut reader = Reader::from_str(xml);
    let mut result = BTreeMap::new();
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) | Event::Empty(tag)
                if tag.local_name().as_ref() == "Relationship" =>
            {
                if attribute_value(&tag, "TargetMode")?.as_deref() == Some("External") {
                    continue;
                }
                result.insert(
                    required_attribute(&tag, "Id")?,
                    Relationship {
                        target: resolve_part(base, &required_attribute(&tag, "Target")?)?,
                        kind: required_attribute(&tag, "Type")?,
                    },
                );
            }
            Event::Eof => return Ok(result),
            _ => {}
        }
    }
}

pub(super) fn read_sheets(
    input: &Path,
) -> Result<BTreeMap<String, Vec<Vec<CellValue>>>, MarkoffError> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(input)?).map_err(invalid_data)?;
    let root = relationships(&read_part(&mut archive, "_rels/.rels")?, "")?;
    let workbook_path = &root
        .values()
        .find(|relationship| relationship.kind.ends_with("/officeDocument"))
        .ok_or_else(|| invalid_data("XLSX workbook relationship is missing"))?
        .target;
    let (base, file) = workbook_path
        .rsplit_once('/')
        .unwrap_or(("", workbook_path));
    let rels_path = resolve_part(base, &format!("_rels/{file}.rels"))?;
    let rels = relationships(&read_part(&mut archive, &rels_path)?, base)?;
    let mut strings = Vec::new();
    let mut date_styles = Vec::new();
    for relationship in rels.values() {
        if relationship.kind.ends_with("/sharedStrings") {
            strings = shared_strings(&read_part(&mut archive, &relationship.target)?)?;
        } else if relationship.kind.ends_with("/styles") {
            date_styles = parse_date_styles(&read_part(&mut archive, &relationship.target)?)?;
        }
    }
    let xml = read_part(&mut archive, workbook_path)?;
    let mut reader = Reader::from_str(&xml);
    let mut sheets = BTreeMap::new();
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == "sheet" => {
                let name = spreadsheet_text(&required_attribute(&tag, "name")?)?;
                let id = required_attribute(&tag, "id")?;
                let relationship = rels.get(&id).ok_or_else(|| {
                    invalid_data(format!("XLSX sheet relationship {id} is missing"))
                })?;
                if !relationship.kind.ends_with("/worksheet") {
                    return Err(
                        invalid_data(format!("Unsupported XLSX sheet type for {name}")).into(),
                    );
                }
                let rows = worksheet(
                    &read_part(&mut archive, &relationship.target)?,
                    &strings,
                    &date_styles,
                )?;
                if sheets.insert(name, rows).is_some() {
                    return Err(invalid_data("Duplicate XLSX sheet name").into());
                }
            }
            Event::Eof => return Ok(sheets),
            _ => {}
        }
    }
}

fn element_text(reader: &mut Reader<&[u8]>) -> Result<String, MarkoffError> {
    let mut text = String::new();
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Text(event) => {
                text.push_str(&event.xml_content(quick_xml::XmlVersion::Implicit1_0))
            }
            Event::CData(event) => {
                text.push_str(&event.xml_content(quick_xml::XmlVersion::Implicit1_0))
            }
            Event::GeneralRef(event) => {
                let entity = format!("&{};", event.as_ref());
                text.push_str(&quick_xml::escape::unescape(&entity).map_err(invalid_data)?);
            }
            Event::End(_) => return Ok(text),
            Event::Eof => return Err(invalid_data("Unterminated XLSX text element").into()),
            Event::Start(_) | Event::Empty(_) => {
                return Err(invalid_data("Unexpected nested XLSX text element").into());
            }
            _ => {}
        }
    }
}

fn spreadsheet_text(value: &str) -> Result<String, MarkoffError> {
    let mut text = String::new();
    let mut remaining = value;
    while !remaining.is_empty() {
        let mut units = Vec::new();
        while remaining.starts_with("_x") && remaining.as_bytes().get(6) == Some(&b'_') {
            let digits = &remaining.as_bytes()[2..6];
            if !digits.iter().all(u8::is_ascii_hexdigit) {
                break;
            }
            units.push(u16::from_str_radix(&remaining[2..6], 16).map_err(invalid_data)?);
            remaining = &remaining[7..];
        }
        if !units.is_empty() {
            text.push_str(&String::from_utf16(&units).map_err(invalid_data)?);
        } else if let Some(character) = remaining.chars().next() {
            text.push(character);
            remaining = &remaining[character.len_utf8()..];
        }
    }
    Ok(text)
}

fn shared_strings(xml: &str) -> Result<Vec<String>, MarkoffError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_item = false;
    let mut in_phonetic = false;
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "si" => {
                    in_item = true;
                    current.clear();
                }
                "rPh" => in_phonetic = true,
                "t" if in_item && !in_phonetic => current.push_str(&element_text(&mut reader)?),
                _ => {}
            },
            Event::End(tag) => match tag.local_name().as_ref() {
                "si" => {
                    strings.push(spreadsheet_text(&current)?);
                    current.clear();
                    in_item = false;
                }
                "rPh" => in_phonetic = false,
                _ => {}
            },
            Event::Eof => return Ok(strings),
            _ => {}
        }
    }
}

fn is_date_format(code: &str) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    let mut bracketed = None;
    for character in code.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '"' => quoted = !quoted,
            '\\' | '_' | '*' if !quoted => escaped = true,
            '[' if !quoted => bracketed = Some(String::new()),
            ']' if !quoted => {
                if let Some(token) = bracketed.take()
                    && !token.is_empty()
                    && token
                        .chars()
                        .all(|c| matches!(c, 'h' | 'm' | 's' | 'H' | 'M' | 'S'))
                {
                    return true;
                }
            }
            _ if bracketed.is_some() && !quoted => {
                if let Some(token) = bracketed.as_mut() {
                    token.push(character);
                }
            }
            'y' | 'm' | 'd' | 'h' | 's' | 'Y' | 'M' | 'D' | 'H' | 'S' if !quoted => {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn parse_date_styles(xml: &str) -> Result<Vec<bool>, MarkoffError> {
    let mut reader = Reader::from_str(xml);
    let mut formats = BTreeMap::new();
    let mut ids = Vec::new();
    let mut in_cell_xfs = false;
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) | Event::Empty(tag) => match tag.local_name().as_ref() {
                "numFmt" => {
                    let id = required_attribute(&tag, "numFmtId")?
                        .parse::<u32>()
                        .map_err(invalid_data)?;
                    formats.insert(id, is_date_format(&required_attribute(&tag, "formatCode")?));
                }
                "cellXfs" => in_cell_xfs = true,
                "xf" if in_cell_xfs => {
                    ids.push(
                        attribute_value(&tag, "numFmtId")?
                            .map(|id| id.parse::<u32>().map_err(invalid_data))
                            .transpose()?
                            .unwrap_or(0),
                    );
                }
                _ => {}
            },
            Event::End(tag) if tag.local_name().as_ref() == "cellXfs" => in_cell_xfs = false,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(ids
        .into_iter()
        .map(|id| {
            formats
                .get(&id)
                .copied()
                .unwrap_or(matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58))
        })
        .collect())
}

fn coordinates(reference: &str) -> Result<(usize, usize), MarkoffError> {
    let split = reference
        .find(|c: char| c.is_ascii_digit())
        .ok_or_else(|| invalid_data("Invalid XLSX cell reference"))?;
    let (letters, digits) = reference.split_at(split);
    let mut column = 0usize;
    for letter in letters.bytes() {
        if !letter.is_ascii_uppercase() {
            return Err(invalid_data("Invalid XLSX column reference").into());
        }
        column = column
            .checked_mul(26)
            .and_then(|value| value.checked_add(usize::from(letter - b'A') + 1))
            .ok_or_else(|| invalid_data("XLSX column reference overflow"))?;
    }
    let row = digits.parse::<usize>().map_err(invalid_data)?;
    if !(1..=1_048_576).contains(&row) || !(1..=16_384).contains(&column) {
        return Err(invalid_data("XLSX cell reference exceeds worksheet limits").into());
    }
    Ok((row - 1, column - 1))
}

fn cell_value(
    kind: &str,
    value: Option<&str>,
    inline: String,
    strings: &[String],
    date_style: bool,
) -> Result<CellValue, MarkoffError> {
    if kind == "inlineStr" {
        return Ok(CellValue::String(spreadsheet_text(&inline)?));
    }
    if kind == "str" {
        return Ok(CellValue::String(spreadsheet_text(
            value.unwrap_or_default(),
        )?));
    }
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(CellValue::Empty);
    };
    match kind {
        "s" => {
            let index = value.parse::<usize>().map_err(invalid_data)?;
            strings.get(index).cloned().map(CellValue::String)
                .ok_or_else(|| invalid_data("XLSX shared string index is out of bounds").into())
        }
        "b" => match value {
            "0" => Ok(CellValue::Boolean(false)),
            "1" => Ok(CellValue::Boolean(true)),
            _ => Err(invalid_data("Invalid XLSX boolean value").into()),
        },
        "" | "n" if !date_style => {
            let number = value.parse::<f64>().map_err(invalid_data)?;
            if !number.is_finite() {
                return Err(invalid_data("Non-finite XLSX number").into());
            }
            Ok(cell_value_from_float(number))
        }
        _ => Err(invalid_data(format!(
            "XLSX cell type cannot be represented without data loss: {kind:?} (value {value:?}, date style {date_style})"
        )).into()),
    }
}

pub(super) fn worksheet(
    xml: &str,
    strings: &[String],
    date_styles: &[bool],
) -> Result<Vec<Vec<CellValue>>, MarkoffError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut cells = BTreeMap::new();
    let mut row = 0usize;
    let mut next_row = 0usize;
    let mut column = 0usize;
    let mut position = None;
    let mut kind = String::new();
    let mut value = None;
    let mut inline = String::new();
    let mut date_style = false;
    let mut in_data = false;
    let mut in_phonetic = false;
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) => match tag.local_name().as_ref() {
                "sheetData" => in_data = true,
                "row" if in_data => {
                    row = match attribute_value(&tag, "r")? {
                        Some(index) => {
                            let index = index.parse::<usize>().map_err(invalid_data)?;
                            if !(1..=1_048_576).contains(&index) {
                                return Err(invalid_data("Invalid XLSX row index").into());
                            }
                            index - 1
                        }
                        None => next_row,
                    };
                    next_row = row + 1;
                    column = 0;
                }
                "c" if in_data => {
                    let coordinate = match attribute_value(&tag, "r")? {
                        Some(reference) => coordinates(&reference)?,
                        None if row < 1_048_576 && column < 16_384 => (row, column),
                        None => {
                            return Err(invalid_data("XLSX cell exceeds worksheet limits").into());
                        }
                    };
                    column = coordinate.1 + 1;
                    position = Some(coordinate);
                    kind = attribute_value(&tag, "t")?.unwrap_or_default();
                    date_style = match attribute_value(&tag, "s")? {
                        Some(style) => {
                            let style = style.parse::<usize>().map_err(invalid_data)?;
                            *date_styles
                                .get(style)
                                .ok_or_else(|| invalid_data("XLSX style index is out of bounds"))?
                        }
                        None => false,
                    };
                    value = None;
                    inline.clear();
                }
                "v" if position.is_some() => value = Some(element_text(&mut reader)?),
                "rPh" => in_phonetic = true,
                "t" if position.is_some() && !in_phonetic => {
                    inline.push_str(&element_text(&mut reader)?);
                }
                _ => {}
            },
            Event::End(tag) => match tag.local_name().as_ref() {
                "c" => {
                    if let Some(coordinate) = position.take() {
                        let cell = cell_value(
                            &kind,
                            value.as_deref(),
                            std::mem::take(&mut inline),
                            strings,
                            date_style,
                        )?;
                        if !matches!(cell, CellValue::Empty)
                            && cells.insert(coordinate, cell).is_some()
                        {
                            return Err(invalid_data("Duplicate XLSX cell reference").into());
                        }
                    }
                }
                "sheetData" => in_data = false,
                "rPh" => in_phonetic = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    if cells.is_empty() {
        return Ok(Vec::new());
    }
    let min_row = cells.keys().map(|&(row, _)| row).min().unwrap();
    let max_row = cells.keys().map(|&(row, _)| row).max().unwrap();
    let min_column = cells.keys().map(|&(_, column)| column).min().unwrap();
    let max_column = cells.keys().map(|&(_, column)| column).max().unwrap();
    let mut rows = vec![vec![CellValue::Empty; max_column - min_column + 1]; max_row - min_row + 1];
    for ((row, column), cell) in cells {
        rows[row - min_row][column - min_column] = cell;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlsx_rejects_truncated_or_malformed_xml_parts() {
        for xml in [
            "",
            "<worksheet>",
            "<a/><b/>",
            "<a x=\"1\" x=\"2\"/>",
            "<a></b>",
            "text<a/>",
        ] {
            assert!(validate_xml(xml).is_err(), "{xml}");
        }
        for xml in ["<worksheet/>", "<worksheet><sheetData/></worksheet>"] {
            validate_xml(xml).unwrap();
        }
    }

    #[test]
    fn xlsx_reads_sparse_typed_cells_and_cached_formulas() {
        let xml = r#"<worksheet><sheetData>
            <row r="3"><c r="C3" t="s"><v>0</v></c><c r="E3" t="b"><v>1</v></c></row>
            <row r="5"><c r="C5"><v>-7</v></c><c><f>1/2</f><v>0.5</v></c><c t="b"><v>0</v></c></row>
            <row><c r="C6" t="inlineStr"><is><r><t> A &amp; </t></r><rPh><t>ignored</t></rPh><r><t><![CDATA[B]]></t></r></is></c><c t="str"><f>""</f><v/></c></row>
        </sheetData></worksheet>"#;
        let rows = worksheet(xml, &["042".into()], &[]).unwrap();
        assert_eq!(
            rows,
            vec![
                vec![
                    CellValue::String("042".into()),
                    CellValue::Empty,
                    CellValue::Boolean(true)
                ],
                vec![CellValue::Empty; 3],
                vec![
                    CellValue::Integer(-7),
                    CellValue::Float(0.5),
                    CellValue::Boolean(false)
                ],
                vec![
                    CellValue::String(" A & B".into()),
                    CellValue::String(String::new()),
                    CellValue::Empty
                ],
            ]
        );
    }

    #[test]
    fn xlsx_reads_rich_shared_strings_and_xml_entities() {
        let xml = r#"<sst><si><t> &lt;&#65;&#x42;&amp; </t></si><si><r><t>one</t></r><rPh><t>ignored</t></rPh><r><t>two</t></r></si><si><t/></si></sst>"#;
        assert_eq!(shared_strings(xml).unwrap(), [" <AB& ", "onetwo", ""]);
    }

    #[test]
    fn xlsx_detects_builtin_custom_and_elapsed_date_formats() {
        let xml = r#"<styleSheet><numFmts>
            <numFmt numFmtId="164" formatCode="yyyy-mm-dd"/>
            <numFmt numFmtId="165" formatCode="[h]:mm:ss"/>
            <numFmt numFmtId="166" formatCode="[Magenta][$USD-409]0.00"/>
            <numFmt numFmtId="167" formatCode="0.00 &quot;days&quot;"/>
            </numFmts><cellStyleXfs><xf numFmtId="14"/></cellStyleXfs>
            <cellXfs><xf/><xf numFmtId="14"/><xf numFmtId="164"/><xf numFmtId="165"/><xf numFmtId="166"/><xf numFmtId="167"/></cellXfs></styleSheet>"#;
        assert_eq!(
            parse_date_styles(xml).unwrap(),
            [false, true, true, true, false, false]
        );
        for code in [r"0\m", "0_m", "0*m", "[Red]0.0"] {
            assert!(!is_date_format(code), "{code}");
        }
    }

    #[test]
    fn xlsx_rejects_lossy_or_invalid_cell_values() {
        for cell in [
            r#"<c t="d"><v>2026-09-30T12:00:00</v></c>"#,
            r#"<c t="e"><v>#DIV/0!</v></c>"#,
            r#"<c s="0"><v>45000</v></c>"#,
        ] {
            let xml = format!("<worksheet><sheetData><row>{cell}</row></sheetData></worksheet>");
            let error = worksheet(&xml, &[], &[true]).unwrap_err();
            assert!(
                error.to_string().contains("cannot be represented"),
                "{error}"
            );
        }
        for cell in [
            r#"<c t="s"><v>10</v></c>"#,
            r#"<c t="b"><v>yes</v></c>"#,
            r#"<c><v>NaN</v></c>"#,
            r#"<c><v>not a number</v></c>"#,
            r#"<c s="2"><v>1</v></c>"#,
            r#"<c r="XFE1"><v>1</v></c>"#,
            r#"<c r="A0"><v>1</v></c>"#,
            r#"<c><v>1</c>"#,
        ] {
            let xml = format!("<worksheet><sheetData><row>{cell}</row></sheetData></worksheet>");
            assert!(worksheet(&xml, &[], &[]).is_err(), "{cell}");
        }
    }

    #[test]
    fn xlsx_ignores_empty_styled_cells_and_preserves_empty_sheets() {
        let xml = r#"<worksheet><sheetData><row><c s="0"/><c><f>1+1</f></c></row></sheetData></worksheet>"#;
        assert!(worksheet(xml, &[], &[true]).unwrap().is_empty());
    }

    #[test]
    fn xlsx_follows_package_relationships_for_xlsm_and_multiple_sheets() {
        use crate::zip_utils::write_zip_part;

        let path = std::env::temp_dir().join(format!(
            "markoff_xlsx_reader_{}_{}.xlsm",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let parts = [
            (
                "_rels/.rels",
                r#"<Relationships><Relationship Id="root" Type="ns/officeDocument" Target="/custom/book.xml"/></Relationships>"#,
            ),
            (
                "custom/book.xml",
                r#"<workbook xmlns:r="ns"><sheets><sheet name="Data &amp; More" r:id="data"/><sheet name="Empty" r:id="empty"/></sheets></workbook>"#,
            ),
            (
                "custom/_rels/book.xml.rels",
                r#"<Relationships><Relationship Id="data" Type="ns/worksheet" Target="../sheets/data.xml"/><Relationship Id="empty" Type="ns/worksheet" Target="/sheets/empty.xml"/><Relationship Id="strings" Type="ns/sharedStrings" Target="strings.xml"/><Relationship Id="styles" Type="ns/styles" Target="styles.xml"/></Relationships>"#,
            ),
            ("custom/strings.xml", r#"<sst><si><t>042</t></si></sst>"#),
            (
                "custom/styles.xml",
                r#"<styleSheet><cellXfs><xf numFmtId="0"/></cellXfs></styleSheet>"#,
            ),
            (
                "sheets/data.xml",
                r#"<worksheet><sheetData><row><c t="s" s="0"><v>0</v></c></row></sheetData></worksheet>"#,
            ),
            ("sheets/empty.xml", r#"<worksheet><sheetData/></worksheet>"#),
        ];
        for (name, xml) in parts {
            write_zip_part(
                &mut archive,
                zip::write::SimpleFileOptions::default(),
                name,
                xml,
            )
            .unwrap();
        }
        archive.finish().unwrap();
        let result = read_sheets(&path);
        std::fs::remove_file(&path).unwrap();
        let sheets = result.unwrap();
        assert_eq!(
            sheets["Data & More"],
            vec![vec![CellValue::String("042".into())]]
        );
        assert!(sheets["Empty"].is_empty());
        assert_eq!(sheets.len(), 2);
    }

    #[test]
    fn xlsx_rejects_invalid_relationships_and_references() {
        assert_eq!(
            resolve_part("xl", "../sheets/data.xml").unwrap(),
            "sheets/data.xml"
        );
        assert!(resolve_part("xl", "../../outside.xml").is_err());
        assert!(
            relationships(
                "<Relationships><Relationship Id=\"x\"/></Relationships>",
                ""
            )
            .is_err()
        );
        for reference in [
            "",
            "A",
            "0",
            "1",
            "a1",
            "A1048577",
            "XFE1",
            "A-1",
            "AAAAAAAAAAAAAAAAAAAA1",
        ] {
            assert!(coordinates(reference).is_err(), "{reference}");
        }
        assert_eq!(coordinates("XFD1048576").unwrap(), (1_048_575, 16_383));
    }
}
