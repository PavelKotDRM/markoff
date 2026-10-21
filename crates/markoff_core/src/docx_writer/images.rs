use std::collections::BTreeMap;
use std::path::Path;

use crate::MarkoffError;
use crate::error::invalid_data;
use crate::xml_utils::xml_attribute_escape;

struct ImagePart {
    relationship_id: String,
    part_name: String,
    drawing_id: usize,
    extension: String,
    content_type: &'static str,
    bytes: Vec<u8>,
}

pub(super) struct ImageAllocator {
    images: BTreeMap<String, ImagePart>,
}

impl ImageAllocator {
    pub(super) fn new(source: &str, base_dir: &Path) -> Result<Self, MarkoffError> {
        let mut images = BTreeMap::new();
        let mut remaining = source;
        while let Some(start) = remaining.find("![") {
            let Some((_, destination, consumed)) = parse_image_destination(&remaining[start..])
            else {
                remaining = &remaining[start + 2..];
                continue;
            };
            if !destination.is_empty()
                && !destination.contains("://")
                && !destination.starts_with("data:")
                && !images.contains_key(destination)
            {
                let path = base_dir.join(destination);
                let extension = path
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let Some(content_type) = image_content_type(&extension) else {
                    remaining = &remaining[start + consumed..];
                    continue;
                };
                let bytes = std::fs::read(&path)?;
                validate_image_bytes(&path, &bytes)?;
                let index = images.len() + 1;
                images.insert(
                    destination.to_string(),
                    ImagePart {
                        relationship_id: format!("rId{}", 4 + index - 1),
                        part_name: format!("image{index}.{extension}"),
                        drawing_id: 1000 + index,
                        extension,
                        content_type,
                        bytes,
                    },
                );
            }
            remaining = &remaining[start + consumed..];
        }
        Ok(Self { images })
    }

    pub(super) fn next_relationship_id(&self) -> u32 {
        4 + self.images.len() as u32
    }

    pub(super) fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    pub(super) fn drawing_xml(&self, destination: &str, alt: &str) -> Option<String> {
        let image = self.images.get(destination)?;
        let rel = &image.relationship_id;
        let name = xml_attribute_escape(alt);
        let svg_extension = if image.extension == "svg" {
            format!(
                "<a:extLst><a:ext uri=\"{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}\"><asvg:svgBlip r:embed=\"{rel}\"/></a:ext></a:extLst>"
            )
        } else {
            String::new()
        };
        Some(format!(
            "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"4572000\" cy=\"2286000\"/><wp:docPr id=\"{}\" name=\"{}\" descr=\"{}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{}\" name=\"{}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rel}\">{svg_extension}</a:blip><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"4572000\" cy=\"2286000\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
            image.drawing_id, name, name, image.drawing_id, name
        ))
    }

    pub(super) fn relationship_entries(&self) -> String {
        self.images
            .values()
            .map(|image| {
                format!(
                    "<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{}\"/>",
                    image.relationship_id,
                    xml_attribute_escape(&image.part_name)
                )
            })
            .collect()
    }

    pub(super) fn content_type_entries(&self) -> String {
        let extensions = self
            .images
            .values()
            .map(|image| (image.extension.as_str(), image.content_type))
            .collect::<BTreeMap<_, _>>();
        extensions
            .into_iter()
            .map(|(extension, content_type)| {
                format!(
                    "<Default Extension=\"{}\" ContentType=\"{}\"/>",
                    xml_attribute_escape(extension),
                    content_type
                )
            })
            .collect()
    }

    pub(super) fn parts(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.images
            .values()
            .map(|image| (image.part_name.as_str(), image.bytes.as_slice()))
    }
}

fn image_content_type(extension: &str) -> Option<&'static str> {
    match extension {
        "svg" => Some("image/svg+xml"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        _ => None,
    }
}

pub(super) fn parse_image_destination(value: &str) -> Option<(&str, &str, usize)> {
    let after_start = value.strip_prefix("![")?;
    let mut depth = 0usize;
    let mut escaped = false;
    let label_end = after_start.char_indices().find_map(|(index, character)| {
        if escaped {
            escaped = false;
            return None;
        }
        if character == '\\' {
            escaped = true;
            return None;
        }
        match character {
            '[' => {
                depth += 1;
                None
            }
            ']' if depth == 0 => Some(index),
            ']' => {
                depth -= 1;
                None
            }
            _ => None,
        }
    })?;
    let after_label = &after_start[label_end + 1..];
    let after_open = after_label.strip_prefix('(')?;
    depth = 0;
    escaped = false;
    let destination_end = after_open.char_indices().find_map(|(index, character)| {
        if escaped {
            escaped = false;
            return None;
        }
        if character == '\\' {
            escaped = true;
            return None;
        }
        match character {
            '(' => {
                depth += 1;
                None
            }
            ')' if depth == 0 => Some(index),
            ')' => {
                depth -= 1;
                None
            }
            _ => None,
        }
    })?;
    let label = &after_start[..label_end];
    let raw_destination = after_open[..destination_end].trim();
    let destination = if let Some(destination) = raw_destination.strip_prefix('<') {
        destination.split_once('>')?.0
    } else {
        raw_destination.split_whitespace().next()?
    };
    Some((label, destination, 2 + label_end + 2 + destination_end + 1))
}

pub(super) fn validate_image_bytes(path: &Path, bytes: &[u8]) -> Result<(), MarkoffError> {
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "svg"
        && (!bytes
            .windows(4)
            .any(|window| window.eq_ignore_ascii_case(b"<svg"))
            || std::str::from_utf8(bytes).is_err())
    {
        return Err(invalid_data(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Markdown SVG image does not contain valid UTF-8 SVG content",
        ))
        .into());
    }
    Ok(())
}
