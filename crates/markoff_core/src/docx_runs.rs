use crate::MarkoffError;
use crate::docx_inline::{DocxRunStyle, markdown_from_docx_run};
use crate::error::invalid_data;
use quick_xml::XmlVersion;

pub(super) struct PendingRun {
    pub(super) text: String,
    pub(super) style: DocxRunStyle,
    pub(super) target: Option<String>,
}

pub(super) fn resolve_general_ref(
    event: &quick_xml::events::BytesRef<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> Result<String, MarkoffError> {
    let name = decoder.decode(event).map_err(invalid_data)?;
    let escaped = format!("&{name};");
    Ok(quick_xml::escape::unescape(&escaped)
        .map_err(invalid_data)?
        .into_owned())
}

pub(super) fn word_property_enabled(
    event: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> bool {
    event
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.local_name().as_ref() == b"val")
        .and_then(|attribute| {
            attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
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
