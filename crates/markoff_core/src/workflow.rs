use crate::{ConversionRequest, Format, MarkoffError, OcrConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Optional OCR and persistent intermediate settings, shared by CLI and GUI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConversionOptions {
    /// Keep a self-contained JSON document beside the final output and use it
    /// as the actual input to the final conversion step.
    pub keep_intermediate: bool,
    /// Recognition of PDF pages without embedded text. Disabled by default.
    pub ocr: OcrConfig,
}

impl ConversionOptions {
    /// Read a TOML configuration. Relative local model paths resolve beside it.
    ///
    /// # Errors
    ///
    /// Returns an error for unreadable files or invalid configuration.
    pub fn from_file(path: &Path) -> Result<Self, MarkoffError> {
        let mut options: Self =
            toml::from_str(&std::fs::read_to_string(path)?).map_err(crate::error::invalid_data)?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let resolve = |path: &mut PathBuf| {
            if path.is_relative() {
                *path = base.join(&*path);
            }
        };
        match &mut options.ocr {
            OcrConfig::Oar {
                auto_download,
                detection,
                recognition,
                dictionary,
                ..
            } => {
                for path in [detection, recognition, dictionary] {
                    if !*auto_download || path.components().count() > 1 {
                        resolve(path);
                    }
                }
            }
            OcrConfig::OarVl {
                model_dir,
                layout_dir,
                ..
            } => {
                for path in [model_dir, layout_dir].into_iter().flatten() {
                    resolve(path);
                }
            }
            _ => {}
        }
        Ok(options)
    }
}

/// Path of the persistent JSON intermediate for a destination.
#[must_use]
pub fn intermediate_document_path(output: &Path) -> PathBuf {
    let mut name = output.as_os_str().to_os_string();
    name.push(".intermediate.json");
    PathBuf::from(name)
}

/// Convert through an optional visible, editable, self-contained JSON stage.
///
/// The intermediate is retained even if the final exporter fails. It can be
/// edited and converted independently to any supported destination. Existing
/// data-oriented routes are used for workbooks; document routes embed images
/// in the JSON schema instead of leaving references to temporary files.
///
/// # Errors
///
/// Returns validation, recognition, import, or export errors. Both output paths
/// honor the request's overwrite policy; neither may refer to the source file.
pub fn convert_document_with_options(
    request: &ConversionRequest,
    options: &ConversionOptions,
) -> Result<(), MarkoffError> {
    crate::validate_request(request)?;
    if !options.keep_intermediate {
        return crate::route_with_options(request, options);
    }

    let intermediate = intermediate_document_path(&request.output);
    let import = ConversionRequest {
        output: intermediate.clone(),
        to: Format::Json,
        style: None,
        tables_only: request.tables_only
            && !matches!(request.from, Format::Json | Format::Yaml | Format::Toml),
        ..request.clone()
    };
    crate::validate_request(&import)?;
    if intermediate.exists()
        && request.output.exists()
        && same_file::is_same_file(&intermediate, &request.output)?
    {
        return Err(MarkoffError::InvalidOption {
            message: "output and intermediate paths must differ".to_string(),
        });
    }
    if request.from == Format::Json {
        // Validate JSON before publishing it as the intermediate.
        let source = std::fs::read_to_string(&request.input)?;
        let value: serde_json::Value =
            serde_json::from_str(&source).map_err(crate::error::invalid_data)?;
        std::fs::write(
            &intermediate,
            serde_json::to_string_pretty(&value).map_err(crate::error::invalid_data)?,
        )?;
    } else {
        crate::route_with_options(&import, options)?;
    }
    if request.to == Format::Json {
        std::fs::copy(intermediate, &request.output)?;
        return Ok(());
    }
    crate::route_with_options(
        &ConversionRequest {
            input: intermediate,
            from: Format::Json,
            ..request.clone()
        },
        options,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{convert_document, convert_file};
    use std::fs;

    struct Workspace(PathBuf);

    impl Workspace {
        fn new() -> Self {
            Self(
                crate::intermediate_path("json")
                    .unwrap()
                    .parent()
                    .unwrap()
                    .to_path_buf(),
            )
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Workspace {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn request(input: &Path, output: &Path, from: Format, to: Format) -> ConversionRequest {
        ConversionRequest {
            input: input.into(),
            output: output.into(),
            from,
            to,
            overwrite: false,
            csv_delimiter: b',',
            tables_only: false,
            style: None,
        }
    }

    fn normalize_images(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                if object.contains_key("data") && object.contains_key("destination") {
                    object.insert("destination".into(), "embedded-image".into());
                }
                for value in object.values_mut() {
                    normalize_images(value);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    normalize_images(value);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn rich_document_survives_docx_json_toml_yaml_markdown_docx_loop() {
        let workspace = Workspace::new();
        let markdown = workspace.path("source.md");
        let image = image::RgbImage::from_pixel(2, 2, image::Rgb([255, 0, 0]));
        image.save(workspace.path("figure.png")).unwrap();
        fs::write(&markdown, "# Heading\n\nA **bold** and *italic* [link](https://example.com).\n\n- first\n- second\n\n| Name | Value |\n| --- | --- |\n| Ada | **42** |\n\n![Figure](figure.png)\n\n```rust\nlet answer = 42;\n```\n").unwrap();
        let mut input = markdown;
        let mut from = Format::Markdown;
        let mut baseline = None;
        for (index, to) in [
            Format::Docx,
            Format::Json,
            Format::Toml,
            Format::Yaml,
            Format::Markdown,
            Format::Docx,
            Format::Json,
        ]
        .into_iter()
        .enumerate()
        {
            let output = workspace.path(&format!("step{index}.{to}"));
            convert_file(&input, &output, from, to).unwrap();
            if to == Format::Json {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
                normalize_images(&mut value);
                if let Some(expected) = &baseline {
                    assert_eq!(
                        &value, expected,
                        "supported document nodes changed across the loop"
                    );
                } else {
                    assert!(value.to_string().contains("\"data\""));
                    baseline = Some(value);
                }
            }
            input = output;
            from = to;
        }
    }

    #[test]
    fn intermediate_is_self_contained_editable_and_honors_overwrite() {
        let workspace = Workspace::new();
        let input = workspace.path("source.md");
        let output = workspace.path("result.docx");
        // A data URI exercises image import from OCR API Markdown as well.
        fs::write(&input, "# Original\n\n![dot](data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=)\n").unwrap();
        let options = ConversionOptions {
            keep_intermediate: true,
            ..Default::default()
        };
        let request = request(&input, &output, Format::Markdown, Format::Docx);
        convert_document_with_options(&request, &options).unwrap();
        let intermediate = intermediate_document_path(&output);
        let source = fs::read_to_string(&intermediate).unwrap();
        assert!(source.contains("\"format\": \"png\""));
        assert!(source.contains("\"data\":"));
        fs::remove_file(&output).unwrap();
        assert!(matches!(
            convert_document_with_options(&request, &options),
            Err(MarkoffError::OutputExists { .. })
        ));
        assert!(!output.exists());
        fs::write(&intermediate, source.replace("Original", "Edited")).unwrap();
        convert_document(&ConversionRequest {
            input: intermediate,
            from: Format::Json,
            ..request
        })
        .unwrap();
        let restored = workspace.path("restored.md");
        convert_file(&output, &restored, Format::Docx, Format::Markdown).unwrap();
        assert!(fs::read_to_string(restored).unwrap().contains("Edited"));
    }

    #[test]
    fn formerly_missing_document_pairs_use_shared_conversion_routes() {
        let workspace = Workspace::new();
        let source = workspace.path("source.md");
        fs::write(&source, "# Shared pipeline\n\nText **retained**.\n").unwrap();
        for (from, to) in [
            (Format::Html, Format::Odt),
            (Format::Odt, Format::Docx),
            (Format::Pptx, Format::Html),
            (Format::Pdf, Format::Docx),
        ] {
            let input = workspace.path(&format!("source.{from}"));
            let output = workspace.path(&format!("result-{from}.{to}"));
            convert_file(&source, &input, Format::Markdown, from).unwrap();
            convert_file(&input, &output, from, to).unwrap();
            let restored = workspace.path(&format!("restored-{from}.md"));
            convert_file(&output, &restored, to, Format::Markdown).unwrap();
            assert!(
                fs::read_to_string(restored)
                    .unwrap()
                    .contains("Shared pipeline")
            );
        }
    }

    #[test]
    fn typed_workbook_sheets_survive_json_toml_yaml_and_ods() {
        let workspace = Workspace::new();
        let input = workspace.path("source.json");
        let value = serde_json::json!({
            "First": [{"id": 42, "active": true, "name": "Ada"}],
            "Second": [{"score": 3.5, "name": "Grace"}]
        });
        fs::write(&input, value.to_string()).unwrap();
        let mut input = input;
        let mut from = Format::Json;
        for (index, to) in [
            Format::Xlsx,
            Format::Ods,
            Format::Xlsx,
            Format::Json,
            Format::Toml,
            Format::Yaml,
            Format::Ods,
            Format::Json,
        ]
        .into_iter()
        .enumerate()
        {
            let output = workspace.path(&format!("typed{index}.{to}"));
            convert_file(&input, &output, from, to).unwrap();
            input = output;
            from = to;
        }
        let restored: serde_json::Value =
            serde_json::from_slice(&fs::read(input).unwrap()).unwrap();
        assert_eq!(restored, value);
    }

    #[test]
    fn structured_chains_preserve_root_arrays_scalars_and_nulls_through_toml() {
        let workspace = Workspace::new();
        for (index, expected) in [
            serde_json::json!([{"id": 42, "missing": null}, {"id": 7, "missing": "null"}]),
            serde_json::json!({"nested": [true, null, 3.5]}),
            serde_json::json!({"large": u64::MAX}),
            serde_json::json!({"__markoff_json_value_v1": "not a protocol payload"}),
            serde_json::json!("scalar"),
            serde_json::Value::Null,
        ]
        .into_iter()
        .enumerate()
        {
            let input = workspace.path(&format!("value{index}.json"));
            let toml = workspace.path(&format!("value{index}.toml"));
            let yaml = workspace.path(&format!("value{index}.yaml"));
            let restored = workspace.path(&format!("restored{index}.json"));
            fs::write(&input, expected.to_string()).unwrap();
            convert_file(&input, &toml, Format::Json, Format::Toml).unwrap();
            convert_file(&toml, &yaml, Format::Toml, Format::Yaml).unwrap();
            convert_file(&yaml, &restored, Format::Yaml, Format::Json).unwrap();
            let value: serde_json::Value =
                serde_json::from_slice(&fs::read(restored).unwrap()).unwrap();
            assert_eq!(value, expected);
        }
    }

    #[test]
    fn local_config_paths_are_relative_to_the_config_not_the_process() {
        let workspace = Workspace::new();
        let config = workspace.path("ocr.toml");
        fs::write(&config, "[ocr]\nbackend = 'oar'\ndetection = 'det.onnx'\nrecognition = 'rec.onnx'\ndictionary = 'dict.txt'\n").unwrap();
        let options = ConversionOptions::from_file(&config).unwrap();
        let OcrConfig::Oar { detection, .. } = options.ocr else {
            panic!("expected oar");
        };
        assert_eq!(detection, workspace.path("det.onnx"));
        fs::write(&config, "keep_intermedate = true").unwrap();
        assert!(ConversionOptions::from_file(&config).is_err());
    }

    #[test]
    fn intermediate_survives_export_failure_and_supports_table_filtering() {
        let workspace = Workspace::new();
        let input = workspace.path("source.md");
        let output = workspace.path("result.docx");
        fs::write(&input, "# Heading\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n").unwrap();
        let options = ConversionOptions {
            keep_intermediate: true,
            ..Default::default()
        };
        let mut request = request(&input, &output, Format::Markdown, Format::Docx);
        request.style = Some(workspace.path("missing-theme.toml"));
        assert!(convert_document_with_options(&request, &options).is_err());
        let intermediate = intermediate_document_path(&output);
        assert!(intermediate.is_file());
        assert!(!output.exists());
        let filtered = workspace.path("filtered.docx");
        convert_document_with_options(
            &ConversionRequest {
                input: intermediate,
                output: filtered.clone(),
                from: Format::Json,
                to: Format::Docx,
                tables_only: true,
                style: None,
                ..request
            },
            &options,
        )
        .unwrap();
        let markdown = workspace.path("filtered.md");
        convert_file(&filtered, &markdown, Format::Docx, Format::Markdown).unwrap();
        let source = fs::read_to_string(markdown).unwrap();
        assert!(source.contains("| A | B |"));
        assert!(!source.contains("Heading"));
    }
}
