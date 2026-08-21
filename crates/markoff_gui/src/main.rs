use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use egui_extras::{Column, TableBuilder};
use markoff_core::{ConversionRequest, Format, convert_document, convert_file, detect_format};
use std::path::{Path, PathBuf};

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
        Box::new(|_cc| Ok(Box::new(MarkoffApp::default()))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobStatus {
    Pending,
    Success,
    Failed,
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

/// How a source file's preview should be displayed, chosen by its format.
enum SourcePreview {
    /// Rendered as formatted Markdown (headings, bold, lists, ...).
    Markdown(String),
    /// Rendered as a collapsible tree (JSON/YAML/TOML).
    Tree(serde_json::Value),
    /// Shown as plain text (unsupported/unreadable formats).
    Text(String),
}

struct ConversionJob {
    input: PathBuf,
    output: PathBuf,
    source_preview: SourcePreview,
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
            }
            Err(error) => {
                job.status = JobStatus::Failed;
                job.message = error.to_string();
            }
        }
    }

    fn result_preview(&self) -> String {
        self.selected
            .and_then(|index| self.jobs.get(index))
            .map(|job| match std::fs::read_to_string(&job.output) {
                Ok(content) => content,
                Err(_) => job.message.clone(),
            })
            .unwrap_or_else(|| "Converted output will appear here.".to_string())
    }
}

fn preview_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time is after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "markoff_preview_{}_{}.md",
        std::process::id(),
        nanos
    ))
}

fn load_source_preview(input: &Path) -> SourcePreview {
    match detect_format(input) {
        Ok(Format::Markdown) => SourcePreview::Markdown(read_text_or_error(input)),
        Ok(Format::Json) => {
            structured_tree_preview(input, |source| serde_json::from_str(source).map_err(|error| error.to_string()))
        }
        Ok(Format::Yaml) => structured_tree_preview(input, |source| {
            serde_yaml::from_str::<serde_json::Value>(source).map_err(|error| error.to_string())
        }),
        Ok(Format::Toml) => structured_tree_preview(input, |source| {
            source
                .parse::<toml::Value>()
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }),
        Ok(format @ (Format::Docx | Format::Pdf | Format::Xlsx | Format::Pptx)) => {
            let preview = preview_path();
            let rendered = convert_file(input, &preview, format, Format::Markdown)
                .map_err(|error| error.to_string())
                .and_then(|()| std::fs::read_to_string(&preview).map_err(|error| error.to_string()));
            std::fs::remove_file(&preview).ok();
            match rendered {
                Ok(markdown) => SourcePreview::Markdown(markdown),
                Err(error) => {
                    SourcePreview::Text(format!("Unable to create Markdown preview: {error}"))
                }
            }
        }
        Ok(Format::Html) => {
            let preview = preview_path();
            let rendered = convert_file(input, &preview, Format::Html, Format::Markdown)
                .map_err(|error| error.to_string())
                .and_then(|()| std::fs::read_to_string(&preview).map_err(|error| error.to_string()));
            std::fs::remove_file(&preview).ok();
            match rendered {
                Ok(markdown) => SourcePreview::Markdown(markdown),
                Err(error) => {
                    SourcePreview::Text(format!("Unable to create Markdown preview: {error}"))
                }
            }
        }
        Ok(_) => SourcePreview::Text(read_text_or_error(input)),
        Err(_) => SourcePreview::Text(format!(
            "Unable to preview {}: unrecognized format",
            input.display()
        )),
    }
}

fn read_text_or_error(input: &Path) -> String {
    std::fs::read_to_string(input)
        .unwrap_or_else(|error| format!("Unable to read {}: {error}", input.display()))
}

fn structured_tree_preview(
    input: &Path,
    parse: impl FnOnce(&str) -> Result<serde_json::Value, String>,
) -> SourcePreview {
    match std::fs::read_to_string(input) {
        Ok(source) => match parse(&source) {
            Ok(value) => SourcePreview::Tree(value),
            Err(error) => {
                SourcePreview::Text(format!("Unable to parse {}: {error}", input.display()))
            }
        },
        Err(error) => SourcePreview::Text(format!("Unable to read {}: {error}", input.display())),
    }
}

/// Renders a JSON value as a collapsible tree; scalars at the root are shown
/// as a single label since there is nothing to expand.
fn render_json_tree(ui: &mut egui::Ui, value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                render_json_node(ui, key, child);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                render_json_node(ui, &index.to_string(), child);
            }
        }
        scalar => {
            ui.label(json_scalar_to_string(scalar));
        }
    }
}

fn render_json_node(ui: &mut egui::Ui, key: &str, value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            egui::CollapsingHeader::new(key)
                .id_salt(key)
                .default_open(false)
                .show(ui, |ui| {
                    for (child_key, child) in map {
                        render_json_node(ui, child_key, child);
                    }
                });
        }
        serde_json::Value::Array(items) => {
            egui::CollapsingHeader::new(format!("{key} [{}]", items.len()))
                .id_salt(key)
                .default_open(false)
                .show(ui, |ui| {
                    for (index, child) in items.iter().enumerate() {
                        render_json_node(ui, &index.to_string(), child);
                    }
                });
        }
        scalar => {
            ui.label(format!("{key}: {}", json_scalar_to_string(scalar)));
        }
    }
}

fn json_scalar_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        other => other.to_string(),
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
            let result_preview = self.result_preview();
            let markdown_cache = &mut self.markdown_cache;
            ui.columns(2, |columns| {
                columns[0].heading("Source");
                egui::ScrollArea::vertical()
                    .id_salt("source_scroll")
                    .show(&mut columns[0], |ui| match selected_preview {
                        Some(SourcePreview::Markdown(markdown)) => {
                            CommonMarkViewer::new().show(ui, markdown_cache, markdown);
                        }
                        Some(SourcePreview::Tree(value)) => render_json_tree(ui, value),
                        Some(SourcePreview::Text(text)) => {
                            ui.monospace(text);
                        }
                        None => {
                            ui.monospace("Select a file to preview its source.");
                        }
                    });
                columns[1].heading("Result");
                egui::ScrollArea::vertical()
                    .id_salt("result_scroll")
                    .show(&mut columns[1], |ui| {
                        ui.monospace(&result_preview);
                    });
            });
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

#[allow(dead_code)]
fn main() -> eframe::Result<()> {
    run()
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
        let SourcePreview::Markdown(markdown) = preview else {
            panic!("expected a Markdown preview for a converted DOCX file");
        };
        assert!(markdown.contains("# Project"));
        assert!(markdown.contains("**bold**"));

        fs::remove_file(markdown).ok();
        fs::remove_file(document).ok();
    }
}
