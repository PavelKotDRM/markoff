//! Bidirectional PowerPoint (PPTX) <-> Markdown conversion.
//!
//! Slides carry titles, paragraphs, bulleted items, images, and tables (charts
//! are imported as tables of their cached data). Blocks are reflowed
//! top-to-bottom; exact shape positioning, speaker notes, animations, and
//! custom layouts/themes are not preserved.

use crate::MarkoffError;
use crate::error::invalid_data;
use crate::slides::{Slide, SlideBlock, SlideImage, image_placeholder_text, parse_markdown_slides};
use crate::xml_utils::{xml_attribute_escape, xml_escape};
use crate::zip_utils::write_zip_part;
use pulldown_cmark::{Event, Options, Parser};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

pub(crate) use crate::pptx_reader::convert_pptx_to_markdown;

const SLIDE_WIDTH: i64 = 9_144_000;
const SLIDE_HEIGHT: i64 = 6_858_000;
const MARGIN: i64 = 457_200;
const CONTENT_WIDTH: i64 = SLIDE_WIDTH - 2 * MARGIN;
const CONTENT_TOP: i64 = 1_600_200;
const LINE_HEIGHT: i64 = 411_480;
const ROW_HEIGHT: i64 = 370_840;
const GAP: i64 = 91_440;
const EMU_PER_PIXEL: f64 = 9_525.0;

pub(crate) fn convert_markdown_to_pptx(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let base_dir = input.parent().unwrap_or_else(|| Path::new("."));
    let slides = parse_markdown_slides(&source, base_dir);
    write_pptx(&slides, output)
}

pub(crate) fn markdown_inline_to_plain_text(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_MATH);
    options.insert(Options::ENABLE_SUPERSCRIPT);
    options.insert(Options::ENABLE_SUBSCRIPT);

    let mut text = String::new();
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Text(value)
            | Event::Code(value)
            | Event::InlineMath(value)
            | Event::DisplayMath(value) => text.push_str(&value),
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            Event::TaskListMarker(checked) => {
                text.push_str(if checked { "[x] " } else { "[ ] " });
            }
            Event::FootnoteReference(label) => {
                text.push('[');
                text.push_str(&label);
                text.push(']');
            }
            Event::Start(_)
            | Event::End(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::Rule => {}
        }
    }
    text.trim().to_string()
}

struct SlidePart {
    xml: String,
    relationships: String,
    media: Vec<(String, Vec<u8>)>,
}

fn write_pptx(slides: &[Slide], output: &Path) -> Result<(), MarkoffError> {
    use zip::write::SimpleFileOptions;

    let mut media_counter = 0usize;
    let mut media_types = BTreeMap::new();
    let slide_parts = slides
        .iter()
        .map(|slide| build_slide(slide, &mut media_counter, &mut media_types))
        .collect::<Vec<_>>();

    let slide_count = slides.len();
    let file = std::fs::File::create(output)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    let app_properties = build_app_properties(slide_count);
    let presentation_xml = build_presentation_xml(slide_count);
    let presentation_rels = build_presentation_rels(slide_count);
    let content_types = build_content_types(slide_count, &media_types);
    let mut parts = vec![
        ("[Content_Types].xml".to_string(), content_types.as_str()),
        ("_rels/.rels".to_string(), PACKAGE_RELS),
        ("docProps/core.xml".to_string(), CORE_PROPERTIES),
        ("docProps/app.xml".to_string(), app_properties.as_str()),
        (
            "ppt/presentation.xml".to_string(),
            presentation_xml.as_str(),
        ),
        (
            "ppt/_rels/presentation.xml.rels".to_string(),
            presentation_rels.as_str(),
        ),
        ("ppt/theme/theme1.xml".to_string(), THEME_XML),
        (
            "ppt/slideMasters/slideMaster1.xml".to_string(),
            SLIDE_MASTER_XML,
        ),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels".to_string(),
            SLIDE_MASTER_RELS,
        ),
        (
            "ppt/slideLayouts/slideLayout1.xml".to_string(),
            SLIDE_LAYOUT_XML,
        ),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels".to_string(),
            SLIDE_LAYOUT_RELS,
        ),
    ];
    for (index, part) in slide_parts.iter().enumerate() {
        parts.push((
            format!("ppt/slides/slide{}.xml", index + 1),
            part.xml.as_str(),
        ));
        parts.push((
            format!("ppt/slides/_rels/slide{}.xml.rels", index + 1),
            part.relationships.as_str(),
        ));
    }

    for (name, content) in parts {
        write_zip_part(&mut archive, options, &name, content)?;
    }
    let stored = options.compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in slide_parts.iter().flat_map(|part| &part.media) {
        archive.start_file(name, stored).map_err(invalid_data)?;
        archive.write_all(bytes)?;
    }

    archive.finish().map_err(invalid_data)?;
    Ok(())
}

fn build_content_types(slide_count: usize, media_types: &BTreeMap<&str, &str>) -> String {
    let mut defaults = String::new();
    for (extension, media_type) in media_types {
        defaults.push_str(&format!(
            "<Default Extension=\"{extension}\" ContentType=\"{media_type}\"/>"
        ));
    }
    let mut overrides = String::new();
    for index in 1..=slide_count {
        overrides.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{index}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>{defaults}<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/><Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/><Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/><Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>{overrides}<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>"
    )
}

fn build_presentation_rels(slide_count: usize) -> String {
    let mut relationships = String::from(
        "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/>",
    );
    for index in 1..=slide_count {
        relationships.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{index}.xml\"/>",
            index + 1
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{relationships}</Relationships>"
    )
}

fn build_presentation_xml(slide_count: usize) -> String {
    let mut slide_ids = String::new();
    for index in 0..slide_count {
        slide_ids.push_str(&format!(
            "<p:sldId id=\"{}\" r:id=\"rId{}\"/>",
            256 + index,
            index + 2
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst><p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx=\"9144000\" cy=\"6858000\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>"
    )
}

fn build_app_properties(slide_count: usize) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>markoff</Application><Slides>{slide_count}</Slides></Properties>"
    )
}

fn build_slide(
    slide: &Slide,
    media_counter: &mut usize,
    media_types: &mut BTreeMap<&'static str, &'static str>,
) -> SlidePart {
    let mut shapes = String::new();
    let mut relationships = String::from(
        "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>",
    );
    let mut media = Vec::new();
    let mut next_id = 3u32;
    let mut y = CONTENT_TOP;
    let mut pending_text: Vec<(String, bool)> = Vec::new();
    let mut used_body_placeholder = false;

    let mut flush_text =
        |pending: &mut Vec<(String, bool)>, shapes: &mut String, next_id: &mut u32, y: &mut i64| {
            if pending.is_empty() {
                return;
            }
            let height = text_height(pending);
            shapes.push_str(&text_shape(
                *next_id,
                !used_body_placeholder,
                pending,
                *y,
                height,
            ));
            used_body_placeholder = true;
            *next_id += 1;
            *y += height + GAP;
            pending.clear();
        };

    for block in &slide.blocks {
        match block {
            SlideBlock::Text { text, bulleted } => pending_text.push((text.clone(), *bulleted)),
            SlideBlock::Image {
                alt,
                source,
                data: Some(image),
            } if image.extension != "svg" => {
                flush_text(&mut pending_text, &mut shapes, &mut next_id, &mut y);
                *media_counter += 1;
                let relationship_id = format!("rId{}", media.len() + 2);
                let name = format!("image{}.{}", *media_counter, image.extension);
                relationships.push_str(&format!(
                    "<Relationship Id=\"{relationship_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/{name}\"/>"
                ));
                media_types.insert(image.extension, image.media_type);
                let (width, height) = image_extent(image, y);
                let x = MARGIN + (CONTENT_WIDTH - width) / 2;
                let description = if alt.is_empty() { source } else { alt };
                shapes.push_str(&format!(
                    "<p:pic><p:nvPicPr><p:cNvPr id=\"{next_id}\" name=\"Picture {next_id}\" descr=\"{}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{relationship_id}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{width}\" cy=\"{height}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
                    xml_attribute_escape(description)
                ));
                media.push((format!("ppt/media/{name}"), image.bytes.clone()));
                next_id += 1;
                y += height + GAP;
            }
            SlideBlock::Image { alt, source, .. } => {
                pending_text.push((image_placeholder_text(alt, source), false));
            }
            SlideBlock::Table(rows) => {
                flush_text(&mut pending_text, &mut shapes, &mut next_id, &mut y);
                let height = table_height(rows);
                shapes.push_str(&table_frame(next_id, rows, y, height));
                next_id += 1;
                y += height + GAP;
            }
        }
    }
    flush_text(&mut pending_text, &mut shapes, &mut next_id, &mut y);

    let title = xml_escape(&slide.title);
    let title_paragraph = if title.is_empty() {
        "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>".to_string()
    } else {
        format!("<a:p><a:r><a:rPr lang=\"en-US\"/><a:t>{title}</a:t></a:r></a:p>")
    };
    let title_height = CONTENT_TOP - MARGIN - GAP;
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{MARGIN}\" y=\"{MARGIN}\"/><a:ext cx=\"{CONTENT_WIDTH}\" cy=\"{title_height}\"/></a:xfrm></p:spPr><p:txBody><a:bodyPr anchor=\"b\"><a:normAutofit/></a:bodyPr><a:lstStyle/>{title_paragraph}</p:txBody></p:sp>{shapes}</p:spTree></p:cSld></p:sld>"
    );
    SlidePart {
        xml,
        relationships: format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{relationships}</Relationships>"
        ),
        media,
    }
}

fn text_height(items: &[(String, bool)]) -> i64 {
    let lines = items
        .iter()
        .map(|(text, _)| text.chars().count() / 80 + 1)
        .sum::<usize>()
        .max(1);
    i64::try_from(lines).unwrap_or(i64::MAX / LINE_HEIGHT) * LINE_HEIGHT + 2 * GAP
}

fn text_shape(id: u32, placeholder: bool, items: &[(String, bool)], y: i64, height: i64) -> String {
    let mut paragraphs = String::new();
    for (text, bulleted) in items {
        let properties = if *bulleted {
            "<a:pPr marL=\"285750\" indent=\"-285750\"><a:buFont typeface=\"Arial\"/><a:buChar char=\"\u{2022}\"/></a:pPr>"
        } else {
            "<a:pPr marL=\"0\" indent=\"0\"><a:buNone/></a:pPr>"
        };
        paragraphs.push_str(&format!(
            "<a:p>{properties}<a:r><a:rPr lang=\"en-US\"/><a:t>{}</a:t></a:r></a:p>",
            xml_escape(text)
        ));
    }
    let (name, shape_properties, placeholder_properties) = if placeholder {
        (
            "Content",
            "<p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>",
            "<p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr>",
        )
    } else {
        ("Text", "<p:cNvSpPr txBox=\"1\"/>", "<p:nvPr/>")
    };
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name} {id}\"/>{shape_properties}{placeholder_properties}</p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{MARGIN}\" y=\"{y}\"/><a:ext cx=\"{CONTENT_WIDTH}\" cy=\"{height}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr wrap=\"square\"><a:normAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"
    )
}

fn image_extent(image: &SlideImage, y: i64) -> (i64, i64) {
    let (pixel_width, pixel_height) = image.size.unwrap_or((640, 480));
    let natural_width = f64::from(pixel_width.max(1)) * EMU_PER_PIXEL;
    let natural_height = f64::from(pixel_height.max(1)) * EMU_PER_PIXEL;
    let max_height = (SLIDE_HEIGHT - y - MARGIN).max(SLIDE_HEIGHT / 4) as f64;
    let scale = (CONTENT_WIDTH as f64 / natural_width)
        .min(max_height / natural_height)
        .min(1.0);
    (
        ((natural_width * scale).round() as i64).max(1),
        ((natural_height * scale).round() as i64).max(1),
    )
}

fn table_height(rows: &[Vec<String>]) -> i64 {
    rows.iter()
        .map(|row| {
            let lines = row
                .iter()
                .map(|cell| cell.lines().count().max(1))
                .max()
                .unwrap_or(1);
            i64::try_from(lines).unwrap_or(1) * ROW_HEIGHT
        })
        .sum::<i64>()
        .max(ROW_HEIGHT)
}

fn table_frame(id: u32, rows: &[Vec<String>], y: i64, height: i64) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let column_width = CONTENT_WIDTH / i64::try_from(columns).unwrap_or(1);
    let border = |side: &str| {
        format!(
            "<a:{side} w=\"12700\"><a:solidFill><a:srgbClr val=\"808080\"/></a:solidFill></a:{side}>"
        )
    };
    let cell_properties = format!(
        "<a:tcPr>{}{}{}{}</a:tcPr>",
        border("lnL"),
        border("lnR"),
        border("lnT"),
        border("lnB")
    );
    let mut grid = String::new();
    for _ in 0..columns {
        grid.push_str(&format!("<a:gridCol w=\"{column_width}\"/>"));
    }
    let mut table_rows = String::new();
    for (row_index, row) in rows.iter().enumerate() {
        table_rows.push_str(&format!("<a:tr h=\"{ROW_HEIGHT}\">"));
        for column in 0..columns {
            let cell = row.get(column).map_or("", String::as_str);
            let bold = if row_index == 0 { " b=\"1\"" } else { "" };
            let mut paragraphs = String::new();
            for line in cell.split('\n') {
                if line.is_empty() {
                    paragraphs.push_str("<a:p><a:endParaRPr lang=\"en-US\"/></a:p>");
                } else {
                    paragraphs.push_str(&format!(
                        "<a:p><a:r><a:rPr lang=\"en-US\" sz=\"1400\"{bold}/><a:t>{}</a:t></a:r></a:p>",
                        xml_escape(line)
                    ));
                }
            }
            table_rows.push_str(&format!(
                "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</a:txBody>{cell_properties}</a:tc>"
            ));
        }
        table_rows.push_str("</a:tr>");
    }
    format!(
        "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {id}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"{MARGIN}\" y=\"{y}\"/><a:ext cx=\"{CONTENT_WIDTH}\" cy=\"{height}\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr firstRow=\"1\" bandRow=\"1\"/><a:tblGrid>{grid}</a:tblGrid>{table_rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>"
    )
}

const PACKAGE_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>";

const CORE_PROPERTIES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>Presentation</dc:title><dc:creator>markoff</dc:creator></cp:coreProperties>";

const THEME_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Markoff Theme\"><a:themeElements><a:clrScheme name=\"Markoff\"><a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1><a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1><a:dk2><a:srgbClr val=\"1F497D\"/></a:dk2><a:lt2><a:srgbClr val=\"EEECE1\"/></a:lt2><a:accent1><a:srgbClr val=\"4F81BD\"/></a:accent1><a:accent2><a:srgbClr val=\"C0504D\"/></a:accent2><a:accent3><a:srgbClr val=\"9BBB59\"/></a:accent3><a:accent4><a:srgbClr val=\"8064A2\"/></a:accent4><a:accent5><a:srgbClr val=\"4BACC6\"/></a:accent5><a:accent6><a:srgbClr val=\"F79646\"/></a:accent6><a:hlink><a:srgbClr val=\"0000FF\"/></a:hlink><a:folHlink><a:srgbClr val=\"800080\"/></a:folHlink></a:clrScheme><a:fontScheme name=\"Markoff\"><a:majorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"Markoff\"><a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w=\"9525\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln w=\"25400\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln w=\"38100\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>";

const SLIDE_MASTER_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:sldMaster xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title Placeholder\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Body Placeholder\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst></p:sldMaster>";

const SLIDE_MASTER_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"../theme/theme1.xml\"/></Relationships>";

const SLIDE_LAYOUT_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:sldLayout xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" type=\"title\" preserve=\"1\"><p:cSld name=\"Title and Content\"><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Content\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>";

const SLIDE_LAYOUT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"../slideMasters/slideMaster1.xml\"/></Relationships>";

#[cfg(test)]
mod tests {
    use super::{convert_markdown_to_pptx, convert_pptx_to_markdown};
    use crate::test_support::unique_temp_path;
    use std::fs;

    #[test]
    fn round_trips_titles_and_bullets_through_pptx() {
        let markdown_in = unique_temp_path("pptx_input", "md");
        let pptx = unique_temp_path("pptx_presentation", "pptx");
        let markdown_out = unique_temp_path("pptx_output", "md");
        fs::write(
            &markdown_in,
            "# Welcome\n\nIntro paragraph.\n\n## Agenda\n\n- First topic\n- Second topic\n",
        )
        .unwrap();

        convert_markdown_to_pptx(&markdown_in, &pptx).unwrap();
        convert_pptx_to_markdown(&pptx, &markdown_out).unwrap();

        let rendered = fs::read_to_string(&markdown_out).unwrap();
        assert!(rendered.contains("## Welcome"));
        assert!(rendered.contains("Intro paragraph."));
        assert!(rendered.contains("## Agenda"));
        assert!(rendered.contains("- First topic"));
        assert!(rendered.contains("- Second topic"));

        fs::remove_file(markdown_in).ok();
        fs::remove_file(pptx).ok();
        fs::remove_file(markdown_out).ok();
    }
}
