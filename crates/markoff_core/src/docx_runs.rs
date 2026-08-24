use crate::MarkoffError;
use crate::docx_inline::{DocxRunStyle, markdown_from_docx_run};
use crate::error::invalid_data;

pub(super) struct PendingRun {
    pub(super) text: String,
    pub(super) style: DocxRunStyle,
    pub(super) target: Option<String>,
}

pub(super) fn resolve_general_ref(
    event: &quick_xml::events::BytesRef<'_>,
) -> Result<String, MarkoffError> {
    let name = event.as_ref();
    let escaped = format!("&{name};");
    Ok(quick_xml::escape::unescape(&escaped)
        .map_err(invalid_data)?
        .into_owned())
}

pub(super) fn word_property_enabled(event: &quick_xml::events::BytesStart<'_>) -> bool {
    event
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.local_name().as_ref() == "val")
        .and_then(|attribute| {
            attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
        })
        .is_none_or(|value| !matches!(value.as_ref(), "0" | "false" | "off" | "none" | "nil"))
}

pub(super) fn flush_pending_run(paragraph: &mut String, pending_run: &mut Option<PendingRun>) {
    if let Some(pending) = pending_run.take() {
        paragraph.push_str(&markdown_from_docx_run(
            &pending.text,
            pending.style,
            pending.target.as_deref(),
        ));
    }
}

pub(super) fn queue_docx_run(
    paragraph: &mut String,
    pending_run: &mut Option<PendingRun>,
    text: &str,
    style: DocxRunStyle,
    target: Option<String>,
) {
    if let Some(previous) = pending_run.as_mut()
        && previous.style == style
        && previous.target.as_deref() == target.as_deref()
    {
        previous.text.push_str(text);
    } else {
        flush_pending_run(paragraph, pending_run);
        *pending_run = Some(PendingRun {
            text: text.to_string(),
            style,
            target,
        });
    }
}
