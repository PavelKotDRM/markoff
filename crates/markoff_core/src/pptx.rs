//! Bidirectional PowerPoint (PPTX) <-> Markdown conversion.
//!
//! Scope is intentionally limited to slide titles and body text (plain
//! paragraphs and bulleted items), mirroring the pragmatic subset already
//! supported for DOCX/PDF: shape positioning, images, charts, speaker notes,
//! and slide layouts/themes are not preserved.

use crate::MarkoffError;
use crate::error::invalid_data;
use crate::xml_utils::xml_escape;
use crate::zip_utils::write_zip_part;
use std::path::Path;

pub(crate) use crate::pptx_reader::convert_pptx_to_markdown;

struct Slide {
    title: String,
    items: Vec<(String, bool)>,
}

pub(crate) fn convert_markdown_to_pptx(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let source = std::fs::read_to_string(input)?;
    let slides = parse_markdown_into_slides(&source);
    write_pptx(&slides, output)
}

fn parse_markdown_into_slides(source: &str) -> Vec<Slide> {
    let mut slides = Vec::new();
    let mut current: Option<Slide> = None;
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line == "---" {
            continue;
        }
        if let Some(text) = line.strip_prefix("# ").or_else(|| line.strip_prefix("## ")) {
            if let Some(slide) = current.take() {
                slides.push(slide);
            }
            current = Some(Slide {
                title: text.trim().to_string(),
                items: Vec::new(),
            });
            continue;
        }
        let (text, bulleted) =
            if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
                (rest.to_string(), true)
            } else if let Some(rest) = strip_ordered_marker(line) {
                (rest, true)
            } else {
                (line.to_string(), false)
            };
        let slide = current.get_or_insert_with(|| Slide {
            title: String::new(),
            items: Vec::new(),
        });
        slide.items.push((text, bulleted));
    }
    if let Some(slide) = current.take() {
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

fn strip_ordered_marker(line: &str) -> Option<String> {
    let digits_end = line.find(|character: char| !character.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    line.get(digits_end..)?
        .strip_prefix(". ")
        .map(str::to_string)
}

fn write_pptx(slides: &[Slide], output: &Path) -> Result<(), MarkoffError> {
    use zip::write::SimpleFileOptions;

    let slide_count = slides.len();
    let file = std::fs::File::create(output)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    let app_properties = build_app_properties(slide_count);
    let presentation_xml = build_presentation_xml(slide_count);
    let presentation_rels = build_presentation_rels(slide_count);
    let content_types = build_content_types(slide_count);
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
    let slide_xml = slides.iter().map(build_slide_xml).collect::<Vec<_>>();
    for (index, xml) in slide_xml.iter().enumerate() {
        parts.push((format!("ppt/slides/slide{}.xml", index + 1), xml.as_str()));
        parts.push((
            format!("ppt/slides/_rels/slide{}.xml.rels", index + 1),
            SLIDE_RELS,
        ));
    }

    for (name, content) in parts {
        write_zip_part(&mut archive, options, &name, content)?;
    }

    archive.finish().map_err(invalid_data)?;
    Ok(())
}

fn build_content_types(slide_count: usize) -> String {
    let mut overrides = String::new();
    for index in 1..=slide_count {
        overrides.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{index}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/><Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/><Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/><Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>{overrides}<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>"
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

fn build_slide_xml(slide: &Slide) -> String {
    let title = xml_escape(&slide.title);
    let mut body = String::new();
    if slide.items.is_empty() {
        body.push_str("<a:p><a:endParaRPr lang=\"en-US\"/></a:p>");
    }
    for (text, bulleted) in &slide.items {
        let paragraph_properties = if *bulleted {
            "<a:pPr><a:buChar char=\"\u{2022}\"/></a:pPr>"
        } else {
            "<a:pPr><a:buNone/></a:pPr>"
        };
        body.push_str(&format!(
            "<a:p>{paragraph_properties}<a:r><a:t>{}</a:t></a:r></a:p>",
            xml_escape(text)
        ));
    }
    let title_paragraph = if title.is_empty() {
        "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>".to_string()
    } else {
        format!("<a:p><a:r><a:t>{title}</a:t></a:r></a:p>")
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{title_paragraph}</p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Content\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{body}</p:txBody></p:sp></p:spTree></p:cSld></p:sld>"
    )
}

const PACKAGE_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>";

const CORE_PROPERTIES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>Presentation</dc:title><dc:creator>markoff</dc:creator></cp:coreProperties>";

const THEME_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Markoff Theme\"><a:themeElements><a:clrScheme name=\"Markoff\"><a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1><a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1><a:dk2><a:srgbClr val=\"1F497D\"/></a:dk2><a:lt2><a:srgbClr val=\"EEECE1\"/></a:lt2><a:accent1><a:srgbClr val=\"4F81BD\"/></a:accent1><a:accent2><a:srgbClr val=\"C0504D\"/></a:accent2><a:accent3><a:srgbClr val=\"9BBB59\"/></a:accent3><a:accent4><a:srgbClr val=\"8064A2\"/></a:accent4><a:accent5><a:srgbClr val=\"4BACC6\"/></a:accent5><a:accent6><a:srgbClr val=\"F79646\"/></a:accent6><a:hlink><a:srgbClr val=\"0000FF\"/></a:hlink><a:folHlink><a:srgbClr val=\"800080\"/></a:folHlink></a:clrScheme><a:fontScheme name=\"Markoff\"><a:majorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"Markoff\"><a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w=\"9525\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln w=\"25400\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln w=\"38100\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>";

const SLIDE_MASTER_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:sldMaster xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title Placeholder\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Body Placeholder\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst></p:sldMaster>";

const SLIDE_MASTER_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"../theme/theme1.xml\"/></Relationships>";

const SLIDE_LAYOUT_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><p:sldLayout xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" type=\"title\" preserve=\"1\"><p:cSld name=\"Title and Content\"><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Content\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>";

const SLIDE_LAYOUT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"../slideMasters/slideMaster1.xml\"/></Relationships>";

const SLIDE_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/></Relationships>";

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
