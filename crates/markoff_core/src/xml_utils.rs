//! Small XML/Markdown escaping and quick-xml attribute helpers shared by the
//! DOCX, PPTX, and HTML converters.

use crate::MarkoffError;
use crate::error::invalid_data;

/// Escapes the three characters that are unsafe to place inside XML text
/// content (`&`, `<`, `>`); attribute values are never built with this
/// helper, so `"`/`'` are intentionally left alone.
pub(crate) fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn xml_attribute_escape(value: &str) -> String {
    xml_escape(value)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Which Markdown-special characters need escaping when emitting plain text
/// that came from a given source format, so it round-trips as literal text
/// rather than being misread as Markdown syntax.
#[derive(Clone, Copy)]
pub(crate) enum MarkdownEscapeContext {
    /// DOCX runs also render `~~strikethrough~~` and raw `<tags>`.
    Docx,
    /// HTML text uses `_` for emphasis (`*_*`), so it needs escaping too.
    Html,
    /// PPTX text never contains literal Markdown syntax beyond the base set.
    Plain,
}

pub(crate) fn markdown_escape(value: &str, context: MarkdownEscapeContext) -> String {
    let value = value
        .replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('`', "\\`")
        .replace('[', "\\[")
        .replace(']', "\\]");
    match context {
        MarkdownEscapeContext::Docx => value.replace('~', "\\~").replace('<', "\\<"),
        MarkdownEscapeContext::Html => value.replace('_', "\\_"),
        MarkdownEscapeContext::Plain => value,
    }
}

/// Reads a single attribute's decoded/unescaped value from a start tag,
/// returning `Ok(None)` when the attribute is absent.
pub(crate) fn attribute_value(
    tag: &quick_xml::events::BytesStart<'_>,
    name: &str,
) -> Result<Option<String>, MarkoffError> {
    tag.attributes()
        .flatten()
        .find(|attribute| attribute.key.local_name().as_ref() == name)
        .map(|attribute| {
            attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|value| value.into_owned())
                .map_err(invalid_data)
        })
        .transpose()
        .map_err(MarkoffError::from)
}

/// Parses an OOXML `*.rels` relationships document into an `Id -> Target`
/// map, as used by both `word/_rels/document.xml.rels` (DOCX) and
/// `ppt/_rels/presentation.xml.rels` (PPTX).
pub(crate) fn parse_relationships(
    xml: &str,
) -> Result<std::collections::BTreeMap<String, String>, MarkoffError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    let mut relationships = std::collections::BTreeMap::new();
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(tag) | Event::Empty(tag)
                if tag.local_name().as_ref() == "Relationship" =>
            {
                let id = attribute_value(&tag, "Id")?;
                let target = attribute_value(&tag, "Target")?;
                if let (Some(id), Some(target)) = (id, target) {
                    relationships.insert(id, target);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(relationships)
}

#[cfg(test)]
mod tests {
    use super::{
        MarkdownEscapeContext, markdown_escape, parse_relationships, xml_attribute_escape,
        xml_escape,
    };

    #[test]
    fn xml_escape_covers_the_three_reserved_characters() {
        assert_eq!(xml_escape("a & b < c > d"), "a &amp; b &lt; c &gt; d");
    }

    #[test]
    fn xml_attribute_escape_covers_quotes() {
        assert_eq!(
            xml_attribute_escape("a & b < c > \"d\""),
            "a &amp; b &lt; c &gt; &quot;d&quot;"
        );
    }

    #[test]
    fn docx_context_also_escapes_tilde_and_angle_bracket() {
        assert_eq!(
            markdown_escape("~a<b", MarkdownEscapeContext::Docx),
            "\\~a\\<b"
        );
        assert_eq!(
            markdown_escape("~a<b", MarkdownEscapeContext::Plain),
            "~a<b"
        );
    }

    #[test]
    fn parses_relationship_targets() {
        let xml = "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"x\" Target=\"slides/slide1.xml\"/></Relationships>";
        let relationships = parse_relationships(xml).unwrap();
        assert_eq!(
            relationships.get("rId1").map(String::as_str),
            Some("slides/slide1.xml")
        );
    }
}
