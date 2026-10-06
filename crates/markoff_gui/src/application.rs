use eframe::egui;
use egui_commonmark::CommonMarkCache;
use egui_extras::{Column, TableBuilder};
use markoff_core::{
    ConversionRequest, Format, convert_document, detect_format, supports_tables_only,
};
use std::path::PathBuf;

#[path = "preview.rs"]
mod preview;
#[path = "style_preview.rs"]
mod style_preview;

use preview::{SourcePreview, load_source_preview, render_preview};
use style_preview::{StylePreviewState, show_style_preview};

const BUILD_INFO: &str = concat!(
    "Version: ",
    env!("CARGO_PKG_VERSION"),
    "\nCommit: ",
    env!("VERGEN_GIT_SHA"),
    "\nBranch: ",
    env!("VERGEN_GIT_BRANCH"),
    "\nBuilt: ",
    env!("VERGEN_BUILD_TIMESTAMP"),
    "\nTarget: ",
    env!("VERGEN_CARGO_TARGET_TRIPLE"),
    "\nRustc: ",
    env!("VERGEN_RUSTC_SEMVER"),
);
const RENDERER: &str = if cfg!(target_os = "linux") {
    "wgpu"
} else {
    "glow"
};

/// Runs the native markoff graphical application.
pub fn run() -> eframe::Result<()> {
    #[cfg(target_os = "linux")]
    let renderer = eframe::Renderer::Wgpu;
    #[cfg(not(target_os = "linux"))]
    let renderer = eframe::Renderer::Glow;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        renderer,
        ..Default::default()
    };

    eframe::run_native(
        "markoff",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(MarkoffApp::default()))
        }),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobStatus {
    Pending,
    Success,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewPane {
    Source,
    Result,
}

impl JobStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Success => "Success",
            Self::Failed => "Failed",
        }
    }
}

struct ConversionJob {
    input: PathBuf,
    output: PathBuf,
    source_preview: SourcePreview,
    result_preview: SourcePreview,
    status: JobStatus,
    message: String,
}

struct MarkoffApp {
    jobs: Vec<ConversionJob>,
    selected: Option<usize>,
    target: Format,
    dark_mode: bool,
    show_about: bool,
    overwrite: bool,
    tables_only: bool,
    csv_delimiter: String,
    style: Option<PathBuf>,
    style_preview: Option<StylePreviewState>,
    show_style_preview: bool,
    style_notice: Option<StyleNotice>,
    markdown_cache: CommonMarkCache,
    preview_pane: PreviewPane,
}

enum StyleNotice {
    Exported(PathBuf),
    Imported(PathBuf),
    Failed(String),
}

impl Default for MarkoffApp {
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            selected: None,
            target: Format::Markdown,
            dark_mode: true,
            show_about: false,
            overwrite: false,
            tables_only: false,
            csv_delimiter: ",".to_string(),
            style: None,
            style_preview: None,
            show_style_preview: false,
            style_notice: None,
            markdown_cache: CommonMarkCache::default(),
            preview_pane: PreviewPane::Source,
        }
    }
}

impl MarkoffApp {
    fn add_file(&mut self, input: PathBuf) {
        if self.jobs.iter().any(|job| job.input == input) {
            return;
        }
        let mut output = input.clone();
        output.set_extension(self.target.to_string());
        self.jobs.push(ConversionJob {
            source_preview: load_source_preview(&input),
            result_preview: SourcePreview::message("Converted output will appear here."),
            input,
            output,
            status: JobStatus::Pending,
            message: String::new(),
        });
        self.selected = Some(self.jobs.len() - 1);
    }

    fn update_outputs(&mut self) {
        for job in &mut self.jobs {
            job.output.set_extension(self.target.to_string());
            job.status = JobStatus::Pending;
            job.message.clear();
            job.result_preview = SourcePreview::message("Converted output will appear here.");
        }
    }

    fn remove_job(&mut self, index: usize) {
        self.jobs.remove(index);
        self.selected = match self.selected {
            Some(selected) if selected == index => None,
            Some(selected) if selected > index => Some(selected - 1),
            other => other,
        };
    }

    fn convert_selected(&mut self) {
        let Some(job) = self.selected.and_then(|index| self.jobs.get_mut(index)) else {
            return;
        };
        self.preview_pane = PreviewPane::Result;
        let delimiter = match self.csv_delimiter.as_bytes() {
            [delimiter] if delimiter.is_ascii() => *delimiter,
            _ => {
                job.status = JobStatus::Failed;
                job.message = "CSV delimiter must be a single ASCII character.".to_string();
                job.result_preview = SourcePreview::message(job.message.clone());
                return;
            }
        };
        let result = detect_format(&job.input).and_then(|from| {
            convert_document(&ConversionRequest {
                input: job.input.clone(),
                output: job.output.clone(),
                from,
                to: self.target,
                overwrite: self.overwrite,
                csv_delimiter: delimiter,
                tables_only: self.tables_only,
                style: self.style.clone(),
            })
        });
        match result {
            Ok(()) => {
                job.status = JobStatus::Success;
                job.message = format!("Saved to {}", job.output.display());
                job.result_preview = load_source_preview(&job.output);
            }
            Err(error) => {
                job.status = JobStatus::Failed;
                job.message = error.to_string();
                job.result_preview = SourcePreview::message(job.message.clone());
            }
        }
    }

    fn preview_style(&mut self) {
        let Some(path) = self.style.as_deref() else {
            return;
        };
        self.style_preview = Some(StylePreviewState::load(path));
        self.show_style_preview = true;
    }

    fn export_style_template(&mut self, path: PathBuf) {
        self.style_notice = Some(match markoff_core::write_default_style_theme(&path, true) {
            Ok(()) => StyleNotice::Exported(path),
            Err(error) => StyleNotice::Failed(format!(
                "Unable to export style template to {}: {error}",
                path.display()
            )),
        });
    }

    fn import_style_theme(&mut self, source: PathBuf, path: PathBuf) {
        match markoff_core::write_style_theme_from_document(&source, &path, true) {
            Ok(()) => {
                self.style_preview = Some(StylePreviewState::load(&path));
                self.style = Some(path.clone());
                self.show_style_preview = true;
                self.style_notice = Some(StyleNotice::Imported(path));
                self.update_outputs();
            }
            Err(error) => {
                self.style_notice = Some(StyleNotice::Failed(format!(
                    "Unable to import style from {}: {error}",
                    source.display()
                )));
            }
        }
    }

    fn create_default_style(&mut self, path: PathBuf) {
        self.show_style_preview = true;
        if let Err(error) = markoff_core::write_default_style_theme(&path, true) {
            self.style_preview = Some(StylePreviewState::Error {
                path,
                message: format!("unable to create default style theme: {error}"),
            });
            return;
        }
        self.style_preview = Some(StylePreviewState::load(&path));
        self.style = Some(path);
        self.update_outputs();
    }
}

impl eframe::App for MarkoffApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dropped = ui.ctx().input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            self.add_file(file.path().to_path_buf());
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Add files").clicked()
                    && let Some(files) = rfd::FileDialog::new().pick_files()
                {
                    for file in files {
                        self.add_file(file);
                    }
                }
                ui.separator();
                ui.label("Convert to:");
                let previous_target = self.target;
                egui::ComboBox::from_id_salt("target_format")
                    .selected_text(self.target.to_string())
                    .show_ui(ui, |ui| {
                        for format in [
                            Format::Markdown,
                            Format::Pdf,
                            Format::Doc,
                            Format::Docx,
                            Format::Odt,
                            Format::Json,
                            Format::Csv,
                            Format::Yaml,
                            Format::Toml,
                            Format::Xls,
                            Format::Xlsx,
                            Format::Ods,
                            Format::Html,
                            Format::Ppt,
                            Format::Pptx,
                            Format::Odp,
                        ] {
                            ui.selectable_value(&mut self.target, format, format.to_string());
                        }
                    });
                if self.target != previous_target {
                    self.update_outputs();
                }
                ui.checkbox(&mut self.overwrite, "Overwrite existing files");
                let tables_only_supported = self
                    .selected
                    .and_then(|index| self.jobs.get(index))
                    .and_then(|job| detect_format(&job.input).ok())
                    .is_some_and(|from| supports_tables_only(from, self.target));
                if !tables_only_supported {
                    self.tables_only = false;
                }
                ui.add_enabled_ui(tables_only_supported, |ui| {
                    ui.checkbox(&mut self.tables_only, "Tables only")
                        .on_hover_text(
                            "Keep only table blocks when converting between documents and JSON/YAML/TOML.",
                        );
                });
                ui.label("CSV delimiter:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.csv_delimiter)
                        .desired_width(20.0)
                        .char_limit(1),
                );
                ui.separator();
                ui.label("Style:");
                let style_supported = matches!(
                    self.target,
                    Format::Pdf | Format::Html | Format::Doc | Format::Docx | Format::Odt
                );
                ui.add_enabled_ui(style_supported, |ui| {
                    let label = self
                        .style
                        .as_ref()
                        .and_then(|path| path.file_name())
                        .and_then(|name| name.to_str())
                        .unwrap_or("Default");
                    ui.label(label);
                    if ui
                        .button("Import...")
                        .on_hover_text(
                            "Create a theme from formatting in an existing DOC/DOCX, XLS/XLSX, PPT/PPTX, or ODS file.",
                        )
                        .clicked()
                        && let Some(source) = rfd::FileDialog::new()
                            .add_filter(
                                "Office documents",
                                &["doc", "docx", "xls", "xlsx", "xlsm", "ppt", "pptx", "ods"],
                            )
                            .pick_file()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("TOML theme", &["toml"])
                            .set_file_name("style-theme.toml")
                            .save_file()
                    {
                        self.import_style_theme(source, path);
                    }
                    if ui.button("Choose...").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("TOML theme", &["toml"])
                            .pick_file()
                    {
                        self.style_preview = Some(StylePreviewState::load(&path));
                        self.style = Some(path);
                        self.show_style_preview = true;
                        self.update_outputs();
                    }
                    if ui
                        .button("New...")
                        .on_hover_text(
                            "Save an editable TOML theme with every setting at its default value.",
                        )
                        .clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("TOML theme", &["toml"])
                            .set_file_name("style-theme.toml")
                            .save_file()
                    {
                        self.create_default_style(path);
                    }
                    if ui
                        .add_enabled(self.style.is_some(), egui::Button::new("Preview"))
                        .clicked()
                    {
                        self.preview_style();
                    }
                    if ui
                        .add_enabled(self.style.is_some(), egui::Button::new("Clear"))
                        .clicked()
                    {
                        self.style = None;
                        self.style_preview = None;
                        self.show_style_preview = false;
                        self.update_outputs();
                    }
                });
                if ui
                    .button("Export template...")
                    .on_hover_text(
                        "Save a TOML style template with every setting at its default value \
                         and a comment, for editing in a text editor.",
                    )
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("TOML theme", &["toml"])
                        .set_file_name("style-theme.toml")
                        .save_file()
                {
                    self.export_style_template(path);
                }
                if ui.button("Convert selected").clicked() {
                    self.convert_selected();
                }
                if ui
                    .button(if self.dark_mode { "Light" } else { "Dark" })
                    .clicked()
                {
                    self.dark_mode = !self.dark_mode;
                    ui.ctx().set_visuals(if self.dark_mode {
                        egui::Visuals::dark()
                    } else {
                        egui::Visuals::light()
                    });
                }
                if ui.button("About").clicked() {
                    self.show_about = true;
                }
            });
            let mut dismiss_notice = false;
            if let Some(notice) = &self.style_notice {
                ui.horizontal_wrapped(|ui| {
                    match notice {
                        StyleNotice::Exported(path) => {
                            ui.label(format!("Style template saved to {}", path.display()));
                        }
                        StyleNotice::Imported(path) => {
                            ui.label(format!("Style imported to {}", path.display()));
                        }
                        StyleNotice::Failed(message) => {
                            ui.colored_label(ui.visuals().error_fg_color, message);
                        }
                    }
                    dismiss_notice = ui.small_button("Dismiss").clicked();
                });
            }
            if dismiss_notice {
                self.style_notice = None;
            }
        });

        egui::Panel::left("queue").resizable(true).show(ui, |ui| {
            ui.heading("Queue");
            ui.label("Drop files anywhere in this window.");
            let mut remove = None;
            TableBuilder::new(ui)
                .striped(true)
                .column(Column::remainder())
                .column(Column::auto())
                .header(22.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("File");
                    });
                    header.col(|ui| {
                        ui.strong("Status");
                    });
                })
                .body(|mut body| {
                    for (index, job) in self.jobs.iter().enumerate() {
                        body.row(24.0, |mut row| {
                            row.col(|ui| {
                                let name = job
                                    .input
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("unnamed");
                                if ui
                                    .selectable_label(self.selected == Some(index), name)
                                    .clicked()
                                {
                                    self.selected = Some(index);
                                }
                            });
                            row.col(|ui| {
                                if ui
                                    .small_button(job.status.label())
                                    .on_hover_text(if job.message.is_empty() {
                                        job.output.display().to_string()
                                    } else {
                                        job.message.clone()
                                    })
                                    .clicked()
                                {
                                    remove = Some(index);
                                }
                            });
                        });
                    }
                });
            if let Some(index) = remove {
                self.remove_job(index);
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let selected_job = self.selected.and_then(|index| self.jobs.get(index));
            let selected_preview = selected_job.map(|job| &job.source_preview);
            let result_preview = selected_job.map(|job| &job.result_preview);
            let markdown_cache = &mut self.markdown_cache;

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.preview_pane, PreviewPane::Source, "Source");
                ui.selectable_value(&mut self.preview_pane, PreviewPane::Result, "Result");
            });
            ui.separator();
            if let Some(job) = selected_job
                && job.status == JobStatus::Failed
            {
                egui::Frame::group(ui.style())
                    .fill(ui.visuals().error_fg_color.linear_multiply(0.08))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.colored_label(ui.visuals().error_fg_color, "Conversion failed:");
                            ui.label(&job.message);
                        });
                    });
                ui.add_space(6.0);
            }

            match self.preview_pane {
                PreviewPane::Source => {
                    ui.heading("Source");
                    egui::ScrollArea::vertical()
                        .id_salt("source_scroll")
                        .show(ui, |ui| {
                            render_preview(
                                ui,
                                selected_preview,
                                markdown_cache,
                                "Select a file to preview its source.",
                            );
                        });
                }
                PreviewPane::Result => {
                    ui.heading("Result");
                    egui::ScrollArea::vertical()
                        .id_salt("result_scroll")
                        .show(ui, |ui| {
                            render_preview(
                                ui,
                                result_preview,
                                markdown_cache,
                                "Converted output will appear here.",
                            );
                        });
                }
            }
        });

        if self.show_about {
            egui::Window::new("About markoff")
                .collapsible(false)
                .resizable(false)
                .open(&mut self.show_about)
                .show(ui.ctx(), |ui| {
                    ui.heading("markoff");
                    ui.label("Document conversion workspace");
                    ui.separator();
                    ui.monospace(format!("{BUILD_INFO}\nRenderer: {RENDERER}"));
                });
        }
        if self.show_style_preview
            && let Some(state) = self.style_preview.as_ref()
        {
            show_style_preview(ui.ctx(), &mut self.show_style_preview, state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        JobStatus, MarkoffApp, PreviewPane, SourcePreview, StyleNotice, StylePreviewState,
        load_source_preview,
    };
    use markoff_core::{Format, convert_file};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_path(name: &str, extension: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("markoff_gui_{name}_{nanos}.{extension}"))
    }

    #[test]
    fn previews_docx_as_markdown() {
        let markdown = temporary_path("preview_input", "md");
        let document = temporary_path("preview_document", "docx");
        fs::write(&markdown, "# Project\n\nA **bold** note.\n").unwrap();
        convert_file(&markdown, &document, Format::Markdown, Format::Docx).unwrap();

        let preview = load_source_preview(&document);
        let SourcePreview::Markdown {
            content: markdown, ..
        } = preview
        else {
            panic!("expected a Markdown preview for a converted DOCX file");
        };
        assert!(markdown.contains("# Project"));
        assert!(markdown.contains("**bold**"));

        fs::remove_file(markdown).ok();
        fs::remove_file(document).ok();
    }

    #[test]
    fn converts_markdown_to_pdf_in_gui() {
        let markdown = temporary_path("pdf_preview_input", "md");
        fs::write(
            &markdown,
            "# Содержание\n\n- [1. Первый раздел](#1-первый-раздел)\n- [2. Второй раздел](#2-второй-раздел)\n\n## 1. Первый раздел\n\nCyrillic: Привет, мир!\n\n## 2. Второй раздел\n",
        )
        .unwrap();

        let mut app = MarkoffApp {
            target: Format::Pdf,
            ..MarkoffApp::default()
        };
        app.add_file(markdown.clone());
        let output = app.jobs[0].output.clone();

        app.convert_selected();

        assert!(matches!(app.jobs[0].status, JobStatus::Success));
        let SourcePreview::Markdown { content, .. } = &app.jobs[0].result_preview else {
            panic!("expected a Markdown preview for the generated PDF");
        };
        assert!(content.contains("Cyrillic: Привет, мир!"));

        fs::remove_file(markdown).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn applies_selected_style_theme_in_gui() {
        let markdown = temporary_path("style_input", "md");
        let theme = temporary_path("style_theme", "toml");
        fs::write(&markdown, "# Styled\n\nText.\n").unwrap();
        fs::write(
            &theme,
            "[document]\nfont_family = \"Georgia\"\ntext_color = \"#123456\"\n",
        )
        .unwrap();

        let mut app = MarkoffApp {
            target: Format::Html,
            style: Some(theme.clone()),
            ..MarkoffApp::default()
        };
        app.add_file(markdown.clone());
        let output = app.jobs[0].output.clone();
        app.convert_selected();

        assert!(matches!(app.jobs[0].status, JobStatus::Success));
        let html = fs::read_to_string(&output).unwrap();
        assert!(html.contains("font-family:\"Georgia\""));
        assert!(html.contains("color:#123456"));

        fs::remove_file(markdown).ok();
        fs::remove_file(theme).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn exports_style_template_without_selecting_it() {
        let theme = temporary_path("export_style", "toml");
        let mut app = MarkoffApp::default();

        app.export_style_template(theme.clone());

        assert!(matches!(
            &app.style_notice,
            Some(StyleNotice::Exported(path)) if path == &theme
        ));
        assert!(app.style.is_none());
        assert_eq!(
            fs::read_to_string(&theme).unwrap(),
            markoff_core::default_style_theme_toml()
        );

        let blocked = theme.with_extension("dir");
        fs::create_dir_all(&blocked).unwrap();
        app.export_style_template(blocked.clone());
        assert!(matches!(app.style_notice, Some(StyleNotice::Failed(_))));

        fs::remove_file(theme).ok();
        fs::remove_dir(blocked).ok();
    }

    #[test]
    fn imports_office_style_theme_in_gui() {
        let markdown = temporary_path("style_import_source", "md");
        let source = temporary_path("style_import_document", "docx");
        let theme = temporary_path("style_import_theme", "toml");
        fs::write(&markdown, "# Imported\n\nText.\n").unwrap();
        convert_file(&markdown, &source, Format::Markdown, Format::Docx).unwrap();

        let mut app = MarkoffApp::default();
        app.import_style_theme(source.clone(), theme.clone());

        assert_eq!(app.style.as_ref(), Some(&theme));
        assert!(app.show_style_preview);
        assert!(matches!(
            app.style_preview,
            Some(StylePreviewState::Loaded { .. })
        ));
        assert!(matches!(
            &app.style_notice,
            Some(StyleNotice::Imported(path)) if path == &theme
        ));
        assert_eq!(
            markoff_core::load_style_theme_preview(&theme)
                .unwrap()
                .font_size_pt,
            11.0
        );

        fs::remove_file(markdown).ok();
        fs::remove_file(source).ok();
        fs::remove_file(theme).ok();
    }

    #[test]
    fn creates_default_style_theme_in_gui() {
        let theme = temporary_path("default_style", "toml");
        let mut app = MarkoffApp::default();

        app.create_default_style(theme.clone());

        assert_eq!(app.style.as_ref(), Some(&theme));
        assert!(app.show_style_preview);
        assert!(matches!(
            app.style_preview,
            Some(StylePreviewState::Loaded { .. })
        ));
        assert_eq!(
            fs::read_to_string(&theme).unwrap(),
            markoff_core::default_style_theme_toml()
        );

        fs::remove_file(theme).ok();
    }

    #[test]
    fn loads_selected_theme_for_style_preview() {
        let theme = temporary_path("style_preview", "toml");
        fs::write(
            &theme,
            "[document]\nfont_family = \"Georgia\"\nfont_size_pt = 14\ntext_color = \"#123456\"\n",
        )
        .unwrap();
        let mut app = MarkoffApp {
            style: Some(theme.clone()),
            ..MarkoffApp::default()
        };

        app.preview_style();

        assert!(app.show_style_preview);
        let Some(StylePreviewState::Loaded {
            path,
            theme: preview,
        }) = app.style_preview.as_ref()
        else {
            panic!("expected a loaded style preview");
        };
        assert_eq!(path, &theme);
        assert_eq!(preview.font_family, "Georgia");
        assert_eq!(preview.font_size_pt, 14.0);
        assert_eq!(
            (
                preview.text_color.red,
                preview.text_color.green,
                preview.text_color.blue
            ),
            (0x12, 0x34, 0x56)
        );

        fs::remove_file(theme).unwrap();
    }

    #[test]
    fn converts_only_table_blocks_when_enabled_in_gui() {
        let markdown = temporary_path("tables_only_input", "md");
        fs::write(
            &markdown,
            "# Report\n\nSummary.\n\n| Name | Score |\n| --- | --- |\n| Ada | 42 |\n",
        )
        .unwrap();

        let mut app = MarkoffApp {
            target: Format::Json,
            tables_only: true,
            ..MarkoffApp::default()
        };
        app.add_file(markdown.clone());
        let output = app.jobs[0].output.clone();

        app.convert_selected();

        assert!(matches!(app.jobs[0].status, JobStatus::Success));
        let document: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        let blocks = document["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "table");

        fs::remove_file(markdown).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn keeps_all_document_blocks_by_default_in_gui() {
        let markdown = temporary_path("full_document_input", "md");
        fs::write(
            &markdown,
            "| A | B |\r\n| --- | --- |\r\n| 1 | 2 |\r\n\r\n# Title\r\n\r\nA paragraph.\r\n\r\n- One\r\n  - Nested\r\n",
        )
        .unwrap();

        let mut app = MarkoffApp {
            target: Format::Json,
            ..MarkoffApp::default()
        };
        app.add_file(markdown.clone());
        assert!(!app.tables_only);
        let output = app.jobs[0].output.clone();

        app.convert_selected();

        assert!(matches!(app.jobs[0].status, JobStatus::Success));
        let document: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        let blocks = document["blocks"].as_array().unwrap();
        let block_types = blocks
            .iter()
            .map(|block| block["type"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(block_types, ["table", "heading", "paragraph", "list"]);
        assert_eq!(
            blocks[3]["items"][0]["blocks"][0]["content"][0]["text"],
            "One"
        );
        assert_eq!(blocks[3]["items"][0]["blocks"][1]["type"], "list");
        assert_eq!(
            blocks[3]["items"][0]["blocks"][1]["items"][0]["blocks"][0]["content"][0]["text"],
            "Nested"
        );

        fs::remove_file(markdown).ok();
        fs::remove_file(output).ok();
    }

    #[test]
    fn removing_an_earlier_job_preserves_the_selected_file() {
        let mut app = MarkoffApp::default();
        let first = temporary_path("first", "md");
        let selected = temporary_path("selected", "md");
        let last = temporary_path("last", "md");
        for path in [&first, &selected, &last] {
            fs::write(path, "# Preview\n").unwrap();
            app.add_file(path.clone());
        }
        app.selected = Some(1);
        app.remove_job(0);
        assert_eq!(app.selected, Some(0));
        assert_eq!(app.jobs[0].input, selected);
        app.remove_job(0);
        assert_eq!(app.selected, None);
        for path in [first, selected, last] {
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn invalid_csv_delimiter_is_reported_without_writing_output() {
        let input = temporary_path("delimiter_input", "md");
        fs::write(&input, "| A |\n| --- |\n| B |\n").unwrap();
        let mut app = MarkoffApp {
            target: Format::Csv,
            csv_delimiter: String::new(),
            ..MarkoffApp::default()
        };
        app.add_file(input.clone());
        app.convert_selected();
        assert!(matches!(app.jobs[0].status, JobStatus::Failed));
        assert!(app.jobs[0].message.contains("delimiter"));
        assert!(matches!(app.preview_pane, PreviewPane::Result));
        let SourcePreview::Text(message) = &app.jobs[0].result_preview else {
            panic!("expected the conversion error in the result preview");
        };
        assert_eq!(message, &app.jobs[0].message);
        assert!(!app.jobs[0].output.exists());
        fs::remove_file(input).unwrap();
    }

    #[test]
    fn changing_target_refreshes_pending_outputs_and_preview() {
        let input = temporary_path("target_change", "md");
        fs::write(&input, "# Preview\n").unwrap();
        let mut app = MarkoffApp::default();
        app.add_file(input.clone());
        app.jobs[0].status = JobStatus::Failed;
        app.jobs[0].message = "previous conversion failed".to_string();

        app.target = Format::Pdf;
        app.update_outputs();

        assert_eq!(app.jobs[0].output.extension().unwrap(), "pdf");
        assert!(matches!(app.jobs[0].status, JobStatus::Pending));
        assert!(app.jobs[0].message.is_empty());
        assert!(matches!(app.jobs[0].result_preview, SourcePreview::Text(_)));
        fs::remove_file(input).unwrap();
    }
}
