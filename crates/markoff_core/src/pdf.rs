use crate::MarkoffError;
use crate::error::invalid_data;
use crate::ocr::{OcrConfig, Recognizer};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub(crate) fn convert_pdf_with_ocr(
    input: &Path,
    output: &Path,
    config: &OcrConfig,
) -> Result<(), MarkoffError> {
    let pdfium = bundled_pdfium()?;
    let document = pdfium
        .load_pdf_from_file(input, None)
        .map_err(invalid_data)?;
    let mut pages = Vec::with_capacity(usize::try_from(document.pages().len()).unwrap_or_default());
    let mut recognizer = Recognizer::new(config);
    let mut page_images = Vec::new();
    for index in document.pages().as_range() {
        let page = document.pages().get(index).map_err(invalid_data)?;
        let text = page.text().map_err(invalid_data)?.all();
        if has_usable_text(&text) {
            pages.push(text);
        } else {
            let image = page
                .render_with_config(
                    &PdfRenderConfig::new()
                        .set_target_width(2000)
                        .set_maximum_height(3000),
                )
                .map_err(invalid_data)?
                .as_image()
                .map_err(invalid_data)?
                .to_rgb8();
            let recognized =
                recognizer.recognize(image, u32::try_from(index).map_err(invalid_data)? + 1)?;
            // A blank or illustration-only page remains visible even when OCR finds no words.
            if !has_usable_text(&recognized) {
                page_images.push(index);
            }
            pages.push(recognized);
        }
    }
    let image_dir = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(
            || Path::new("image").to_path_buf(),
            |parent| parent.join("image"),
        );
    let image_links = extract_pdf_images(input, &image_dir)?;
    for (page, links) in pages.iter_mut().zip(&image_links) {
        if !links.is_empty() {
            page.push_str("\n\n");
            page.push_str(&links.join("\n\n"));
        }
    }
    for index in page_images {
        if !image_links[usize::try_from(index).map_err(invalid_data)?].is_empty() {
            continue;
        }
        let page = document.pages().get(index).map_err(invalid_data)?;
        let name = format!("page{}.png", index + 1);
        std::fs::create_dir_all(&image_dir)?;
        page.render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(2000)
                .set_maximum_height(3000),
        )
        .map_err(invalid_data)?
        .as_image()
        .map_err(invalid_data)?
        .save(image_dir.join(&name))
        .map_err(invalid_data)?;
        pages[usize::try_from(index).map_err(invalid_data)?]
            .push_str(&format!("\n\n![Page {}](image/{name})", index + 1));
    }

    std::fs::write(output, format!("{}\n", pages.join("\n\n").trim_end()))?;
    Ok(())
}

fn has_usable_text(text: &str) -> bool {
    text.chars().any(|character| {
        !character.is_whitespace()
            && !character.is_control()
            && !matches!(
                character,
                '\u{fffd}' | '\u{feff}' | '\u{200b}' | '\u{200c}' | '\u{200d}'
            )
    })
}

static PDFIUM: OnceLock<Result<Pdfium, PdfiumInitializationError>> = OnceLock::new();

#[derive(Debug)]
struct PdfiumInitializationError(String);

impl std::fmt::Display for PdfiumInitializationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PdfiumInitializationError {}

pub(crate) fn bundled_pdfium() -> std::io::Result<&'static Pdfium> {
    match PDFIUM.get_or_init(|| {
        let path = ensure_pdfium_library()
            .map_err(|error| PdfiumInitializationError(error.to_string()))?;
        Pdfium::bind_to_library(path)
            .map(Pdfium::new)
            .map_err(|error| PdfiumInitializationError(error.to_string()))
    }) {
        Ok(pdfium) => Ok(pdfium),
        Err(error) => Err(invalid_data(PdfiumInitializationError(error.0.clone()))),
    }
}

const PDFIUM_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pdfium.bin"));
const PDFIUM_SHA256: &str = env!("MARKOFF_PDFIUM_SHA256");
const PDFIUM_VERSION: &str = env!("MARKOFF_PDFIUM_VERSION");
const PDFIUM_LIBRARY_NAME: &str = env!("MARKOFF_PDFIUM_LIBRARY_NAME");
const PDFIUM_TARGET: &str = env!("MARKOFF_PDFIUM_TARGET");

fn ensure_pdfium_library() -> io::Result<std::path::PathBuf> {
    let base = if let Some(path) = std::env::var_os("MARKOFF_PDFIUM_CACHE_DIR") {
        PathBuf::from(path)
    } else {
        #[cfg(target_os = "windows")]
        let base = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("no Windows application cache directory is set"))?;

        #[cfg(target_os = "macos")]
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("HOME is not set"))?
            .join("Library")
            .join("Caches");

        #[cfg(all(unix, not(target_os = "macos")))]
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .ok_or_else(|| io::Error::other("no user cache directory is available"))?;

        base
    };

    let path = base
        .join("markoff")
        .join("pdfium")
        .join(PDFIUM_VERSION)
        .join(PDFIUM_TARGET)
        .join(PDFIUM_LIBRARY_NAME);
    if cached_pdfium_is_valid(&path) {
        return Ok(path);
    }

    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("PDFium cache path has no parent directory"))?;
    fs::create_dir_all(parent)?;
    if path.exists() {
        fs::remove_file(&path)?;
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let temporary = parent.join(format!(
        ".{}.{}.{}.tmp",
        PDFIUM_LIBRARY_NAME,
        std::process::id(),
        timestamp
    ));
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(PDFIUM_BYTES)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        Ok::<_, io::Error>(())
    })();

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        if cached_pdfium_is_valid(&path) {
            return Ok(path);
        }
        return Err(error);
    }

    Ok(path)
}

fn cached_pdfium_is_valid(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| sha256_hex(&bytes) == PDFIUM_SHA256)
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

/// Extracts every image XObject referenced by the PDF's pages, saving each
/// as a file under `image_dir` and returning a Markdown image link per
/// extracted image, in page/appearance order.
fn extract_pdf_images(input: &Path, image_dir: &Path) -> Result<Vec<Vec<String>>, MarkoffError> {
    let document = lopdf::Document::load(input).map_err(invalid_data)?;
    let mut links = Vec::new();
    let mut counter = 0usize;

    for (_, page_id) in document.get_pages() {
        let mut page_links = Vec::new();
        let Ok(images) = document.get_page_images(page_id) else {
            links.push(page_links);
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
            page_links.push(format!("![](image/{file_name})"));
        }
        links.push(page_links);
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

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Object, Stream, dictionary};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn mixed_pdf(path: &Path) {
        let mut document = lopdf::Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let mut pages = Vec::new();
        for content in [
            "BT /F1 14 Tf 50 700 Td (Embedded first page) Tj ET",
            "",
            "BT /F1 14 Tf 50 700 Td (Embedded last page) Tj ET",
        ] {
            let content =
                document.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
            let page = document.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
                "Contents" => content,
            });
            pages.push(Object::Reference(page));
        }
        document.objects.insert(
            pages_id,
            dictionary! {
                "Type" => "Pages", "Kids" => pages, "Count" => 3,
            }
            .into(),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog);
        document.save(path).unwrap();
    }

    fn api_server(status: &str, body: &str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/ocr", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let thread = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "OCR server was not called"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            let header_end = loop {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    break request.len();
                }
            };
            let headers = String::from_utf8_lossy(&request).to_ascii_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("JSON request must have content length")
                .trim()
                .parse()
                .unwrap();
            request.resize(header_end + length, 0);
            socket.read_exact(&mut request[header_end..]).unwrap();
            let json: serde_json::Value = serde_json::from_slice(&request[header_end..]).unwrap();
            assert_eq!(json["page"], 2);
            assert_eq!(json["mime_type"], "image/png");
            use base64::Engine as _;
            let png = base64::engine::general_purpose::STANDARD
                .decode(json["image_base64"].as_str().unwrap())
                .unwrap();
            assert!(png.starts_with(b"\x89PNG"));
            socket.write_all(response.as_bytes()).unwrap();
        });
        (endpoint, thread)
    }

    #[test]
    fn mixed_pdf_recognizes_only_the_textless_page_and_preserves_order() {
        let temporary = crate::intermediate_path("pdf").unwrap();
        mixed_pdf(&temporary);
        let output = temporary.with_extension("md");
        let (endpoint, server) = api_server(
            "200 OK",
            r##"{"markdown":"# Recognized middle\n\n| A | B |\n| --- | --- |\n| 1 | 2 |"}"##,
        );
        convert_pdf_with_ocr(
            &temporary,
            &output,
            &OcrConfig::Api {
                endpoint,
                token_env: None,
                timeout_seconds: 10,
            },
        )
        .unwrap();
        server.join().unwrap();
        let markdown = std::fs::read_to_string(&output).unwrap();
        assert!(
            markdown.find("Embedded first").unwrap() < markdown.find("Recognized middle").unwrap()
        );
        assert!(
            markdown.find("Recognized middle").unwrap() < markdown.find("Embedded last").unwrap()
        );
        assert!(markdown.contains("| 1 | 2 |"));
        crate::remove_intermediate_markdown(&temporary);
    }

    #[test]
    fn missing_ocr_and_server_errors_do_not_publish_empty_success() {
        let temporary = crate::intermediate_path("pdf").unwrap();
        mixed_pdf(&temporary);
        let output = temporary.with_extension("md");
        let error = convert_pdf_with_ocr(&temporary, &output, &OcrConfig::Disabled).unwrap_err();
        assert!(error.to_string().contains("page 2"));
        assert!(!output.exists());
        let (endpoint, server) =
            api_server("500 Internal Server Error", r#"{"error":"unavailable"}"#);
        assert!(
            convert_pdf_with_ocr(
                &temporary,
                &output,
                &OcrConfig::Api {
                    endpoint,
                    token_env: None,
                    timeout_seconds: 10,
                }
            )
            .is_err()
        );
        server.join().unwrap();
        assert!(!output.exists());
        crate::remove_intermediate_markdown(&temporary);
    }

    #[test]
    fn invisible_characters_are_not_a_text_layer() {
        assert!(!has_usable_text(" \n\r\0\u{fffd}\u{200b}\u{feff}"));
        assert!(has_usable_text("42"));
        assert!(has_usable_text("Text"));
    }

    #[test]
    fn text_pages_do_not_initialize_a_configured_ocr_backend() {
        let temporary = crate::intermediate_path("pdf").unwrap();
        mixed_pdf(&temporary);
        let mut document = lopdf::Document::load(&temporary).unwrap();
        document.delete_pages(&[2]);
        document.save(&temporary).unwrap();
        let output = temporary.with_extension("md");
        for config in [
            OcrConfig::Api {
                endpoint: "invalid URL".into(),
                token_env: None,
                timeout_seconds: 0,
            },
            OcrConfig::Oar {
                auto_download: false,
                detection: "missing.onnx".into(),
                recognition: "missing.onnx".into(),
                dictionary: "missing.txt".into(),
            },
        ] {
            convert_pdf_with_ocr(&temporary, &output, &config).unwrap();
            let markdown = std::fs::read_to_string(&output).unwrap();
            assert!(markdown.contains("Embedded first page"));
            assert!(markdown.contains("Embedded last page"));
        }
        crate::remove_intermediate_markdown(&temporary);
    }

    #[test]
    fn empty_successful_ocr_retains_the_original_page() {
        let temporary = crate::intermediate_path("pdf").unwrap();
        mixed_pdf(&temporary);
        let output = temporary.with_extension("md");
        let (endpoint, server) = api_server("200 OK", r#"{"markdown":""}"#);
        convert_pdf_with_ocr(
            &temporary,
            &output,
            &OcrConfig::Api {
                endpoint,
                token_env: None,
                timeout_seconds: 10,
            },
        )
        .unwrap();
        server.join().unwrap();
        let markdown = std::fs::read_to_string(&output).unwrap();
        assert!(markdown.contains("![Page 2](image/page2.png)"));
        assert!(
            temporary
                .parent()
                .unwrap()
                .join("image")
                .join("page2.png")
                .is_file()
        );
        crate::remove_intermediate_markdown(&temporary);
    }

    #[test]
    fn reuses_the_bundled_pdfium_binding() {
        let first = bundled_pdfium().unwrap();
        let second = bundled_pdfium().unwrap();

        assert!(std::ptr::eq(first, second));
    }
}
