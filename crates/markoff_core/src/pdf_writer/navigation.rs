use super::content::normalize_anchor;
use super::*;
use crate::error::invalid_data;
use base64::Engine as _;

pub(super) fn add_pdf_navigation(
    path: &Path,
    headings: &[HeadingOutline],
    anchors: &HashMap<String, usize>,
) -> Result<(), MarkoffError> {
    let mut document = lopdf::Document::load(path).map_err(invalid_data)?;
    let pages = document.get_pages();
    let mut heading_stack = Vec::<(u8, u32)>::new();

    for heading in headings {
        let page_number = u32::try_from(heading.page + 1).map_err(invalid_data)?;
        let page = pages
            .get(&page_number)
            .copied()
            .ok_or_else(|| invalid_data(std::io::Error::other("PDF heading page is missing")))?;
        while heading_stack
            .last()
            .is_some_and(|(level, _)| *level >= heading.level)
        {
            heading_stack.pop();
        }
        let parent = heading_stack.last().map(|(_, bookmark)| *bookmark);
        let format = if heading.level == 1 { 2 } else { 0 };
        let bookmark = document.add_bookmark(
            lopdf::Bookmark::new(heading.title.clone(), [0.1, 0.2, 0.35], format, page),
            parent,
        );
        heading_stack.push((heading.level, bookmark));
    }

    if let Some(outline) = document.build_outline() {
        let catalog = document.catalog_mut().map_err(invalid_data)?;
        catalog.set("Outlines", outline);
        catalog.set("PageMode", "UseOutlines");
    }

    let page_ids = pages.values().copied().collect::<Vec<_>>();
    for page_id in page_ids {
        let annotations = page_annotations(&document, page_id)?;
        for (annotation_index, annotation) in annotations.iter().enumerate() {
            let Some(uri) = annotation_uri(&document, annotation)? else {
                continue;
            };
            let Some(encoded_target) = uri.strip_prefix(INTERNAL_LINK_PREFIX) else {
                continue;
            };
            let target = String::from_utf8(
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(encoded_target)
                    .map_err(invalid_data)?,
            )
            .map_err(invalid_data)?;
            let target = normalize_anchor(&target);
            let action = if let Some(page_number) = anchors.get(&target) {
                let page_number = u32::try_from(*page_number + 1).map_err(invalid_data)?;
                let page = pages.get(&page_number).copied().ok_or_else(|| {
                    invalid_data(std::io::Error::other(
                        "PDF link destination page is missing",
                    ))
                })?;
                let mut action = lopdf::Dictionary::new();
                action.set("S", "GoTo");
                action.set(
                    "D",
                    vec![
                        lopdf::Object::Reference(page),
                        lopdf::Object::Name(b"Fit".to_vec()),
                    ],
                );
                lopdf::Object::Dictionary(action)
            } else {
                let mut action = lopdf::Dictionary::new();
                action.set("S", "URI");
                action.set("URI", format!("#{target}"));
                lopdf::Object::Dictionary(action)
            };
            set_annotation_action(&mut document, page_id, annotation_index, annotation, action)?;
        }
    }

    document.save(path).map_err(invalid_data)?;
    Ok(())
}

fn page_annotations(
    document: &lopdf::Document,
    page_id: lopdf::ObjectId,
) -> Result<Vec<lopdf::Object>, MarkoffError> {
    let page = document
        .get_object(page_id)
        .map_err(invalid_data)?
        .as_dict()
        .map_err(invalid_data)?;
    let Ok(annotations) = page.get(b"Annots") else {
        return Ok(Vec::new());
    };
    let Ok(annotations) = annotations.as_array() else {
        return Ok(Vec::new());
    };
    Ok(annotations.clone())
}

fn annotation_uri(
    document: &lopdf::Document,
    annotation: &lopdf::Object,
) -> Result<Option<String>, MarkoffError> {
    let annotation = if let Ok(annotation_id) = annotation.as_reference() {
        document.get_object(annotation_id).map_err(invalid_data)?
    } else {
        annotation
    };
    let Ok(annotation) = annotation.as_dict() else {
        return Ok(None);
    };
    let Some(action) = annotation.get(b"A").ok() else {
        return Ok(None);
    };
    let action = if let Ok(action_id) = action.as_reference() {
        document.get_object(action_id).map_err(invalid_data)?
    } else {
        action
    };
    let Ok(action) = action.as_dict() else {
        return Ok(None);
    };
    let Some(uri) = action.get(b"URI").ok() else {
        return Ok(None);
    };
    let Ok(uri) = uri.as_str() else {
        return Ok(None);
    };
    let uri = String::from_utf8(uri.to_vec()).map_err(invalid_data)?;
    Ok(Some(uri))
}

fn set_annotation_action(
    document: &mut lopdf::Document,
    page_id: lopdf::ObjectId,
    annotation_index: usize,
    annotation: &lopdf::Object,
    action: lopdf::Object,
) -> Result<(), MarkoffError> {
    if let Ok(annotation_id) = annotation.as_reference() {
        document
            .get_object_mut(annotation_id)
            .map_err(invalid_data)?
            .as_dict_mut()
            .map_err(invalid_data)?
            .set("A", action);
    } else {
        let page = document
            .get_object_mut(page_id)
            .map_err(invalid_data)?
            .as_dict_mut()
            .map_err(invalid_data)?;
        let annotations = page
            .get_mut(b"Annots")
            .map_err(invalid_data)?
            .as_array_mut()
            .map_err(invalid_data)?;
        annotations
            .get_mut(annotation_index)
            .ok_or_else(|| {
                invalid_data(std::io::Error::other(
                    "PDF annotation disappeared during navigation update",
                ))
            })?
            .as_dict_mut()
            .map_err(invalid_data)?
            .set("A", action);
    }
    Ok(())
}
