use super::{OcrConfig, ocr_error};
use crate::MarkoffError;
use crate::error::invalid_data;
use oar_ocr::oarocr::{OAROCR, OAROCRBuilder};
use oar_ocr_vl::{AnyPageParser, AnyPageParserOptions, DocParserOptions, PageParser};
use std::path::Path;

pub(super) enum LocalRecognizer {
    Text(Box<OAROCR>),
    Vision(Box<AnyPageParser>),
}

impl LocalRecognizer {
    pub(super) fn load(config: &OcrConfig) -> Result<Self, MarkoffError> {
        match config {
            OcrConfig::Oar {
                auto_download,
                detection,
                recognition,
                dictionary,
            } => {
                if !auto_download {
                    for path in [detection, recognition, dictionary] {
                        require_file(path)?;
                    }
                }
                let runtime = std::env::var_os("ORT_DYLIB_PATH")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        if cfg!(target_os = "windows") {
                            "onnxruntime.dll".into()
                        } else if cfg!(target_os = "macos") {
                            "libonnxruntime.dylib".into()
                        } else {
                            "libonnxruntime.so".into()
                        }
                    });
                ort::init_from(&runtime)
                    .map_err(|error| ocr_error(format!(
                        "cannot load ONNX Runtime: {error}; set ORT_DYLIB_PATH to the runtime library"
                    )))?
                    .commit();
                Ok(Self::Text(Box::new(
                    OAROCRBuilder::new(detection.clone(), recognition.clone(), dictionary)
                        .build()
                        .map_err(invalid_data)?,
                )))
            }
            OcrConfig::OarVl {
                auto_download,
                model,
                model_dir,
                layout_dir,
            } => {
                let model = model.parse().map_err(invalid_data)?;
                if let Some(layout) = layout_dir
                    && !layout.is_dir()
                {
                    return Err(ocr_error(format!(
                        "OCR layout directory does not exist: {}",
                        layout.display()
                    )));
                }
                let device = oar_ocr_vl::utils::parse_device("cpu").map_err(invalid_data)?;
                let parser = if let Some(directory) = model_dir {
                    if !directory.is_dir() {
                        return Err(ocr_error(format!(
                            "OCR model directory does not exist: {}",
                            directory.display()
                        )));
                    }
                    let mut options = oar_ocr_vl::AnyPageParserLoadOptions::default();
                    if let Some(layout) = layout_dir {
                        options = options.with_layout_dir(layout);
                    }
                    AnyPageParser::from_dir_with_options(model, directory, device, &options)
                } else if *auto_download {
                    let mut options = oar_ocr_vl::AnyPageParserPretrainedOptions::default();
                    if let Some(layout) = layout_dir {
                        options = options.with_layout_dir(layout);
                    }
                    AnyPageParser::from_pretrained(model, device, &options)
                } else {
                    return Err(ocr_error(
                        "oar_vl requires model_dir or auto_download = true",
                    ));
                }
                .map_err(invalid_data)?;
                Ok(Self::Vision(Box::new(parser)))
            }
            _ => Err(ocr_error("expected a local OCR backend")),
        }
    }

    pub(super) fn recognize(&self, image: image::RgbImage) -> Result<String, MarkoffError> {
        match self {
            Self::Text(engine) => {
                let results = engine
                    .predict_with_rows(vec![image])
                    .map_err(invalid_data)?;
                if results.len() != 1 {
                    return Err(ocr_error("oar-ocr returned an unexpected page count"));
                }
                let (page, rows) = &results[0];
                let text = page.text_by_rows(rows).map_err(invalid_data)?;
                let document = crate::document_model::Document {
                    blocks: vec![crate::document_model::Block::Paragraph {
                        text: None,
                        content: vec![crate::document_model::Inline::Text { text }],
                    }],
                };
                crate::document::document_to_markdown(&document, Path::new("."))
            }
            Self::Vision(engine) => {
                let options = AnyPageParserOptions::default().with_doc_parser_options(
                    DocParserOptions::default()
                        .with_skip_auxiliary_regions(false)
                        .with_auxiliary_labels(Vec::new())
                        .with_markdown_ignore_labels(Vec::new()),
                );
                let page = engine.parse_page(&image, &options).map_err(invalid_data)?;
                if !page.diagnostics.is_empty() {
                    return Err(ocr_error(format!(
                        "oar-ocr-vl reported incomplete recognition: {}",
                        page.diagnostics
                            .iter()
                            .map(|diagnostic| diagnostic.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; ")
                    )));
                }

                page.markdown
                    .ok_or_else(|| ocr_error("oar-ocr-vl returned no Markdown"))
            }
        }
    }
}

fn require_file(path: &Path) -> Result<(), MarkoffError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(ocr_error(format!(
            "OCR model file does not exist: {}",
            path.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "downloads OCR weights and runs CPU inference"]
    fn automatic_models_recognize_rendered_text() {
        let temporary = crate::intermediate_path("md").unwrap();
        std::fs::write(&temporary, "# HELLO WORLD\n\nInvoice 12345\n").unwrap();
        let pdf = temporary.with_extension("pdf");
        crate::convert_file(
            &temporary,
            &pdf,
            crate::Format::Markdown,
            crate::Format::Pdf,
        )
        .unwrap();
        let pdfium = crate::pdf::bundled_pdfium().unwrap();
        let document = pdfium.load_pdf_from_file(&pdf, None).unwrap();
        let page = document.pages().get(0).unwrap();
        let image = page
            .render_with_config(
                &pdfium_bundled::pdfium_render::prelude::PdfRenderConfig::new()
                    .set_target_width(1600),
            )
            .unwrap()
            .as_image()
            .unwrap()
            .to_rgb8();
        let engine = LocalRecognizer::load(&OcrConfig::Oar {
            auto_download: true,
            detection: "pp-ocrv6_tiny_det.onnx".into(),
            recognition: "pp-ocrv6_tiny_rec.onnx".into(),
            dictionary: "ppocrv6_tiny_dict.txt".into(),
        })
        .unwrap();
        let text = engine.recognize(image).unwrap();
        assert!(text.contains("HELLO"), "{text}");
        assert!(text.contains("12345"), "{text}");
        drop(page);
        drop(document);
        crate::remove_intermediate_markdown(&temporary);
    }
}
