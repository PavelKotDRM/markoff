use eframe::egui;
use egui_commonmark::CommonMarkCache;
use egui_extras::{Column, TableBuilder};
use markoff_core::{ConversionRequest, Format, convert_document, detect_format};
use std::path::PathBuf;

#[path = "preview.rs"]
mod preview;

use preview::{SourcePreview, load_source_preview, render_preview};

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
    csv_delimiter: String,
    markdown_cache: CommonMarkCache,
    preview_pane: PreviewPane,
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
            csv_delimiter: ",".to_string(),
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

    fn convert_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let delimiter = self.csv_delimiter.bytes().next().unwrap_or(b',');
        let job = &mut self.jobs[index];
        let result = detect_format(&job.input).and_then(|from| {
            convert_document(&ConversionRequest {
                input: job.input.clone(),
                output: job.output.clone(),
                from,
                to: self.target,
                overwrite: self.overwrite,
                csv_delimiter: delimiter,
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
                egui::ComboBox::from_id_salt("target_format")
                    .selected_text(self.target.to_string())
                    .show_ui(ui, |ui| {
                        for format in [
                            Format::Markdown,
                            Format::Json,
                            Format::Csv,
                            Format::Yaml,
                            Format::Toml,
                            Format::Xlsx,
                            Format::Html,
                            Format::Pptx,
                        ] {
                            ui.selectable_value(&mut self.target, format, format.to_string());
                        }
                    });
                if ui.button("Apply format").clicked() {
                    self.update_outputs();
                }
                ui.checkbox(&mut self.overwrite, "Overwrite existing files");
                ui.label("CSV delimiter:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.csv_delimiter)
                        .desired_width(20.0)
                        .char_limit(1),
                );
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
                                if ui.small_button(job.status.label()).clicked() {
                                    remove = Some(index);
                                }
                            });
                        });
                    }
                });
            if let Some(index) = remove {
                self.jobs.remove(index);
                self.selected = self.selected.filter(|selected| *selected != index);
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let selected_preview = self
                .selected
                .and_then(|index| self.jobs.get(index))
                .map(|job| &job.source_preview);
            let result_preview = self
                .selected
                .and_then(|index| self.jobs.get(index))
                .map(|job| &job.result_preview);
            let markdown_cache = &mut self.markdown_cache;

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.preview_pane, PreviewPane::Source, "Source");
                ui.selectable_value(&mut self.preview_pane, PreviewPane::Result, "Result");
            });
            ui.separator();

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
    }
}

#[cfg(test)]
mod tests {
    use super::{SourcePreview, load_source_preview};
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
}
