use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use super::CellValue;
use crate::MarkoffError;
use crate::error::invalid_data;
use crate::xml_utils::xml_escape;
use crate::zip_utils::write_zip_part;

const SPREADSHEET_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELATIONSHIP_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_RELATIONSHIP_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
const XML_HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";
const STYLES: &str = "<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><fonts count=\"1\"><font><sz val=\"11\"/><color theme=\"1\"/><name val=\"Calibri\"/><family val=\"2\"/><scheme val=\"minor\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>";

/// Converts a width in characters to Excel's stored width (7px digit plus
/// 5px padding, rounded to 1/256), as Excel and `rust_xlsxwriter` do.
fn excel_column_width(characters: usize) -> f64 {
    let pixels = (characters as f64 * 7.0 + 0.5).floor() + 5.0;
    (pixels / 7.0 * 256.0).floor() / 256.0
}

fn validate_name(name: &str) -> Result<(), MarkoffError> {
    if name.is_empty()
        || name.chars().count() > 31
        || name.contains(['[', ']', ':', '*', '?', '/', '\\'])
        || name.starts_with('\'')
        || name.ends_with('\'')
        || name.eq_ignore_ascii_case("History")
        || name.chars().any(|c| c.is_control())
    {
        return Err(invalid_data(format!("Invalid XLSX worksheet name: {name:?}")).into());
    }
    Ok(())
}

fn cell_reference(row: usize, column: usize) -> String {
    let mut number = column + 1;
    let mut letters = Vec::new();
    while number > 0 {
        letters.push((b'A' + ((number - 1) % 26) as u8) as char);
        number = (number - 1) / 26;
    }
    format!("{}{row}", letters.iter().rev().collect::<String>())
}

fn spreadsheet_text(value: &str) -> String {
    let mut text = String::new();
    let bytes = value.as_bytes();
    for (offset, character) in value.char_indices() {
        // OOXML uses _xHHHH_ for XML-forbidden characters and escapes
        // literal occurrences of that syntax by encoding the underscore.
        let literal_escape = character == '_'
            && bytes.get(offset + 1) == Some(&b'x')
            && bytes.get(offset + 6) == Some(&b'_')
            && bytes
                .get(offset + 2..offset + 6)
                .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit));
        if literal_escape {
            text.push_str("_x005F_");
        } else if character <= '\u{1f}' && !matches!(character, '\t' | '\n')
            || matches!(character, '\u{fffe}' | '\u{ffff}')
        {
            write!(text, "_x{:04X}_", character as u32).unwrap();
        } else {
            text.push(character);
        }
    }
    xml_escape(&text)
}

fn worksheet(name: &str, rows: &[Vec<CellValue>]) -> Result<String, MarkoffError> {
    if rows.len() > 1_048_576 {
        return Err(MarkoffError::InvalidInput {
            path: format!("worksheet {name:?} exceeds the XLSX row limit"),
        });
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns > 16_384 {
        return Err(MarkoffError::InvalidInput {
            path: format!("worksheet {name:?} exceeds the XLSX column limit"),
        });
    }
    let mut xml = format!("{XML_HEADER}<worksheet xmlns=\"{SPREADSHEET_NS}\">");
    if rows.len() > 1 && rows.first().is_some_and(|row| !row.is_empty()) {
        xml.push_str("<sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/><selection pane=\"bottomLeft\" activeCell=\"A2\" sqref=\"A2\"/></sheetView></sheetViews>");
    }
    if columns > 0 {
        xml.push_str("<cols>");
        for column in 0..columns {
            let width = rows
                .iter()
                .filter_map(|row| row.get(column))
                .map(|value| value.as_text().chars().count())
                .max()
                .unwrap_or(0)
                .clamp(8, 40);
            write!(
                xml,
                "<col min=\"{}\" max=\"{}\" width=\"{}\" customWidth=\"1\"/>",
                column + 1,
                column + 1,
                excel_column_width(width)
            )
            .unwrap();
        }
        xml.push_str("</cols>");
    }
    xml.push_str("<sheetData>");
    for (row_index, row) in rows.iter().enumerate() {
        write!(xml, "<row r=\"{}\">", row_index + 1).unwrap();
        for (column, value) in row.iter().enumerate() {
            let reference = cell_reference(row_index + 1, column);
            match value {
                // Empty strings stay blank cells, matching the previous writer.
                CellValue::Empty => {}
                CellValue::String(value) if value.is_empty() => {}
                CellValue::Integer(value) => {
                    write!(xml, "<c r=\"{reference}\"><v>{value}</v></c>").unwrap();
                }
                CellValue::Float(value) if value.is_finite() => {
                    write!(xml, "<c r=\"{reference}\"><v>{value}</v></c>").unwrap();
                }
                CellValue::Float(_) => return Err(invalid_data("Non-finite XLSX number").into()),
                CellValue::Boolean(value) => {
                    write!(
                        xml,
                        "<c r=\"{reference}\" t=\"b\"><v>{}</v></c>",
                        u8::from(*value)
                    )
                    .unwrap();
                }
                CellValue::String(value) => {
                    if value.chars().count() > 32_767 {
                        return Err(
                            invalid_data("XLSX string exceeds the 32767 character limit").into(),
                        );
                    }
                    write!(xml, "<c r=\"{reference}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", spreadsheet_text(value)).unwrap();
                }
            }
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData></worksheet>");
    Ok(xml)
}

pub(super) fn write_sheets(
    output: &Path,
    sheets: &BTreeMap<String, Vec<Vec<CellValue>>>,
) -> Result<(), MarkoffError> {
    let mut names = BTreeSet::new();
    let mut parts = Vec::new();
    let mut workbook = format!(
        "{XML_HEADER}<workbook xmlns=\"{SPREADSHEET_NS}\" xmlns:r=\"{RELATIONSHIP_NS}\"><bookViews><workbookView/></bookViews><sheets>"
    );
    let mut rels = format!("{XML_HEADER}<Relationships xmlns=\"{PACKAGE_RELATIONSHIP_NS}\">");
    let mut types = format!(
        "{XML_HEADER}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>"
    );
    let default_sheet;
    let sheets = if sheets.is_empty() {
        default_sheet = BTreeMap::from([("Sheet1".to_string(), Vec::new())]);
        &default_sheet
    } else {
        sheets
    };
    for (index, (name, rows)) in sheets.iter().enumerate() {
        validate_name(name)?;
        if !names.insert(name.to_lowercase()) {
            return Err(invalid_data("Duplicate XLSX worksheet name (case-insensitive)").into());
        }
        let id = index + 1;
        write!(
            workbook,
            "<sheet name=\"{}\" sheetId=\"{id}\" r:id=\"rId{id}\"/>",
            spreadsheet_text(name).replace('"', "&quot;")
        )
        .unwrap();
        write!(rels, "<Relationship Id=\"rId{id}\" Type=\"{RELATIONSHIP_NS}/worksheet\" Target=\"worksheets/sheet{id}.xml\"/>").unwrap();
        write!(types, "<Override PartName=\"/xl/worksheets/sheet{id}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>").unwrap();
        parts.push((
            format!("xl/worksheets/sheet{id}.xml"),
            worksheet(name, rows)?,
        ));
    }
    workbook.push_str("</sheets></workbook>");
    let styles_id = sheets.len() + 1;
    write!(rels, "<Relationship Id=\"rId{styles_id}\" Type=\"{RELATIONSHIP_NS}/styles\" Target=\"styles.xml\"/>").unwrap();
    types.push_str("<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>");
    rels.push_str("</Relationships>");
    types.push_str("</Types>");
    parts.push(("xl/styles.xml".into(), format!("{XML_HEADER}{STYLES}")));
    parts.push(("xl/workbook.xml".into(), workbook));
    parts.push(("xl/_rels/workbook.xml.rels".into(), rels));
    parts.push(("[Content_Types].xml".into(), types));
    parts.push(("_rels/.rels".into(), format!("{XML_HEADER}<Relationships xmlns=\"{PACKAGE_RELATIONSHIP_NS}\"><Relationship Id=\"rId1\" Type=\"{RELATIONSHIP_NS}/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>")));
    let mut archive = zip::ZipWriter::new(std::fs::File::create(output)?);
    for (name, xml) in parts {
        write_zip_part(
            &mut archive,
            zip::write::SimpleFileOptions::default(),
            &name,
            &xml,
        )?;
    }
    archive.finish().map_err(invalid_data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlsx_writer_preserves_header_freeze_and_column_widths() {
        let rows = vec![
            vec![CellValue::String("short".into()), CellValue::Empty],
            vec![CellValue::String("x".repeat(100)), CellValue::Boolean(true)],
        ];
        let xml = worksheet("Data", &rows).unwrap();
        assert!(xml.contains("ySplit=\"1\""));
        assert!(xml.contains("topLeftCell=\"A2\""));
        assert!(xml.contains("min=\"1\" max=\"1\" width=\"40.7109375\""));
        assert!(xml.contains("min=\"2\" max=\"2\" width=\"8.7109375\""));
        assert!(!worksheet("Empty", &[]).unwrap().contains("<pane"));
        assert!(!worksheet("Single", &rows[..1]).unwrap().contains("<pane"));
    }

    #[test]
    fn xlsx_writer_rejects_invalid_names_and_values() {
        for name in [
            "", "a/b", "a\\b", "a:b", "a*b", "a?b", "[a]", "'a", "a'", "History", "a\n",
        ] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        assert!(validate_name(&"a".repeat(32)).is_err());
        assert!(validate_name("Data & \"More\"").is_ok());
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(worksheet("Data", &[vec![CellValue::Float(number)]]).is_err());
        }
        assert!(worksheet("Data", &[vec![CellValue::String("x".repeat(32_768))]]).is_err());
        assert!(worksheet("Data", &[vec![CellValue::Empty; 16_385]]).is_err());
        assert!(worksheet("Data", &vec![Vec::new(); 1_048_577]).is_err());
    }

    #[test]
    fn xlsx_writer_leaves_empty_strings_as_blank_cells() {
        let xml = worksheet("Data", &[vec![CellValue::String(String::new())]]).unwrap();
        assert!(!xml.contains("<c "));
    }

    #[test]
    fn xlsx_writer_uses_valid_cell_references() {
        for (column, expected) in [(0, "A1"), (25, "Z1"), (26, "AA1"), (16_383, "XFD1")] {
            assert_eq!(cell_reference(1, column), expected);
        }
    }

    #[test]
    fn xlsx_writer_round_trips_xml_and_ooxml_escaped_strings() {
        let values = [
            " & < > \" '\t\n",
            "_x0041_",
            "_x005F_",
            "before\r\n\0\u{b}\u{fffe}after",
            "\u{410}\u{1f600}",
        ];
        let rows: Vec<Vec<CellValue>> = values
            .iter()
            .map(|value| vec![CellValue::String((*value).to_string())])
            .collect();
        let xml = worksheet("Data", &rows).unwrap();
        assert_eq!(
            super::super::reader::worksheet(&xml, &[], &[]).unwrap(),
            rows
        );
    }

    #[test]
    fn xlsx_writer_produces_package_readable_by_independent_reader() {
        let path = std::env::temp_dir().join(format!(
            "markoff_xlsx_writer_{}_{}.xlsx",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sheets = BTreeMap::from([
            (
                "Data".to_string(),
                vec![
                    vec![
                        CellValue::String("name".into()),
                        CellValue::String("score".into()),
                    ],
                    vec![CellValue::String("Ada".into()), CellValue::Integer(42)],
                ],
            ),
            ("Empty".to_string(), Vec::new()),
        ]);
        write_sheets(&path, &sheets).unwrap();
        let result = office_oxide::xlsx::XlsxDocument::open(&path);
        std::fs::remove_file(&path).unwrap();
        let workbook = result.unwrap();
        assert_eq!(workbook.worksheets.len(), 2);
        assert_eq!(workbook.worksheets[0].rows.len(), 2);
        assert_eq!(workbook.worksheets[0].rows[1].cells[0].reference.col, 0);
        assert_eq!(workbook.sheet_to_csv(0).unwrap(), "name,score\r\nAda,42");
    }
}
