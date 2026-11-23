use crate::MarkoffError;
use crate::error::invalid_data;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

/// Recognition backend used for PDF pages without usable embedded text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case", deny_unknown_fields)]
pub enum OcrConfig {
    /// Return an actionable error instead of silently dropping a scanned page.
    #[default]
    Disabled,
    /// Local ONNX text detection and recognition using oar-ocr.
    Oar {
        /// Download registered model names on demand when true.
        #[serde(default)]
        auto_download: bool,
        /// Detection ONNX file, or registered name when downloads are enabled.
        detection: std::path::PathBuf,
        /// Recognition ONNX file, or registered name when downloads are enabled.
        recognition: std::path::PathBuf,
        /// Matching character dictionary file or registered name.
        dictionary: std::path::PathBuf,
    },
    /// Local layout-aware recognition using oar-ocr-vl.
    OarVl {
        /// Download checkpoints on demand when true.
        #[serde(default)]
        auto_download: bool,
        /// Supported Hugging Face model ID, e.g. PaddlePaddle/PaddleOCR-VL-1.5.
        model: String,
        /// Local checkpoint directory; required without automatic downloads.
        model_dir: Option<std::path::PathBuf>,
        /// Local PP-DocLayout checkpoint for layout-composed models.
        layout_dir: Option<std::path::PathBuf>,
    },
    /// POST a rendered page to an explicitly selected OCR server.
    Api {
        /// Full HTTP(S) endpoint accepting the documented page JSON contract.
        endpoint: String,
        /// Optional environment variable holding a bearer token.
        token_env: Option<String>,
        /// Overall request timeout in seconds (must be positive).
        #[serde(default = "default_timeout")]
        timeout_seconds: u64,
    },
}

fn default_timeout() -> u64 {
    120
}

pub(crate) struct Recognizer<'a> {
    config: &'a OcrConfig,
    local: Option<local::LocalRecognizer>,
}

impl<'a> Recognizer<'a> {
    pub(crate) fn new(config: &'a OcrConfig) -> Self {
        Self {
            config,
            local: None,
        }
    }

    pub(crate) fn recognize(
        &mut self,
        image: image::RgbImage,
        page: u32,
    ) -> Result<String, MarkoffError> {
        match self.config {
            OcrConfig::Disabled => Err(ocr_error(format!(
                "PDF page {page} has no usable text layer; configure OCR with backend oar, oar_vl, or api"
            ))),
            OcrConfig::Api {
                endpoint,
                token_env,
                timeout_seconds,
            } => recognize_api(
                image,
                page,
                endpoint,
                token_env.as_deref(),
                *timeout_seconds,
            ),
            _ => {
                if self.local.is_none() {
                    self.local = Some(local::LocalRecognizer::load(self.config)?);
                }
                self.local
                    .as_ref()
                    .expect("initialized above")
                    .recognize(image)
            }
        }
    }
}

fn recognize_api(
    image: image::RgbImage,
    page: u32,
    endpoint: &str,
    token_env: Option<&str>,
    timeout_seconds: u64,
) -> Result<String, MarkoffError> {
    let uri: ureq::http::Uri = endpoint.parse().map_err(invalid_data)?;
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri.host().is_none()
        || timeout_seconds == 0
    {
        return Err(ocr_error(
            "OCR API requires an HTTP(S) URL and a positive timeout",
        ));
    }
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(invalid_data)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(timeout_seconds)))
        .max_redirects(0)
        .build()
        .into();
    let mut request = agent.post(endpoint);
    if let Some(name) = token_env {
        let token = std::env::var(name)
            .map_err(|_| ocr_error(format!("OCR token environment variable {name} is not set")))?;
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    #[derive(Deserialize)]
    struct Response {
        markdown: String,
    }
    let response: Response = request
        .send_json(serde_json::json!({
            "page": page,
            "mime_type": "image/png",
            "image_base64": base64::engine::general_purpose::STANDARD.encode(png.into_inner()),
        }))
        .map_err(invalid_data)?
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_json()
        .map_err(invalid_data)?;
    Ok(response.markdown)
}

pub(crate) fn ocr_error(message: impl Into<String>) -> MarkoffError {
    MarkoffError::InvalidOption {
        message: message.into(),
    }
}

#[path = "ocr_local.rs"]
mod local;
