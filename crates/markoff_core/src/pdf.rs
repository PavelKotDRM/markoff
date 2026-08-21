use crate::MarkoffError;
use crate::error::invalid_data;
use std::path::Path;

pub(crate) fn convert_pdf_to_markdown(input: &Path, output: &Path) -> Result<(), MarkoffError> {
    let text = pdf_extract::extract_text(input).map_err(invalid_data)?;
    let mut markdown = text.trim_end().to_string();

    let image_dir = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(
            || Path::new("image").to_path_buf(),
            |parent| parent.join("image"),
        );
    let image_links = extract_pdf_images(input, &image_dir).unwrap_or_default();
    if !image_links.is_empty() {
        markdown.push_str("\n\n");
        markdown.push_str(&image_links.join("\n\n"));
    }

    std::fs::write(output, format!("{markdown}\n"))?;
    Ok(())
}

/// Extracts every image XObject referenced by the PDF's pages, saving each
/// as a file under `image_dir` and returning a Markdown image link per
/// extracted image, in page/appearance order.
fn extract_pdf_images(input: &Path, image_dir: &Path) -> Result<Vec<String>, MarkoffError> {
    let document = lopdf::Document::load(input).map_err(invalid_data)?;
    let mut links = Vec::new();
    let mut counter = 0usize;

    for (_, page_id) in document.get_pages() {
        let Ok(images) = document.get_page_images(page_id) else {
            continue;
        };
        for image in images {
            let Some((bytes, extension)) = decode_pdf_image(&document, &image) else {
                continue;
            };
            counter += 1;
            let file_name = format!("image{counter}.{extension}");
            std::fs::create_dir_all(image_dir)?;
            std::fs::write(image_dir.join(&file_name), bytes)?;
            links.push(format!("![](image/{file_name})"));
        }
    }
    Ok(links)
}

/// Decodes a PDF image XObject into standalone file bytes, returning the
/// bytes and a matching file extension. JPEG/JPEG2000 streams are already in
/// their target format and are copied as-is; other filters are decompressed
/// to raw samples and re-encoded as PNG. Color spaces and bit depths this
/// does not recognize are skipped rather than mis-rendered.
fn decode_pdf_image(
    document: &lopdf::Document,
    image: &lopdf::xobject::PdfImage<'_>,
) -> Option<(Vec<u8>, &'static str)> {
    let filters = image.filters.clone().unwrap_or_default();
    if filters.iter().any(|filter| filter == "DCTDecode") {
        return Some((image.content.to_vec(), "jpg"));
    }
    if filters.iter().any(|filter| filter == "JPXDecode") {
        return Some((image.content.to_vec(), "jp2"));
    }

    let stream = document.get_object(image.id).ok()?.as_stream().ok()?;
    let samples = stream.decompressed_content().ok()?;
    let width = u32::try_from(image.width).ok()?;
    let height = u32::try_from(image.height).ok()?;
    let bits_per_component = image.bits_per_component.unwrap_or(8);
    let color_type = match (image.color_space.as_deref(), bits_per_component) {
        (Some("DeviceRGB" | "CalRGB"), 8) => png::ColorType::Rgb,
        (Some("DeviceGray" | "CalGray"), 8) => png::ColorType::Grayscale,
        _ => return None,
    };

    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, width, height);
        encoder.set_color(color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&samples).ok()?;
    }
    Some((png_bytes, "png"))
}
