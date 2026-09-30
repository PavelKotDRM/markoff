use eframe::egui;
use markoff_core::{StyleColor, StyleTextAlign, StyleThemePreview, load_style_theme_preview};
use std::path::{Path, PathBuf};

pub(super) enum StylePreviewState {
    Loaded {
        path: PathBuf,
        theme: Box<StyleThemePreview>,
    },
    Error {
        path: PathBuf,
        message: String,
    },
}

impl StylePreviewState {
    pub(super) fn load(path: &Path) -> Self {
        match load_style_theme_preview(path) {
            Ok(theme) => Self::Loaded {
                path: path.to_path_buf(),
                theme: Box::new(theme),
            },
            Err(error) => Self::Error {
                path: path.to_path_buf(),
                message: error.to_string(),
            },
        }
    }
}

pub(super) fn show_style_preview(
    context: &egui::Context,
    open: &mut bool,
    state: &StylePreviewState,
) {
    egui::Window::new("Style preview")
        .open(open)
        .default_width(680.0)
        .default_height(720.0)
        .resizable(true)
        .show(context, |ui| {
            let (path, theme) = match state {
                StylePreviewState::Loaded { path, theme } => (path, theme),
                StylePreviewState::Error { path, message } => {
                    ui.heading("Style theme error");
                    ui.monospace(path.display().to_string());
                    ui.separator();
                    ui.colored_label(ui.visuals().error_fg_color, message);
                    return;
                }
            };

            ui.horizontal_wrapped(|ui| {
                ui.strong("Theme:");
                ui.monospace(path.display().to_string());
            });
            ui.label(
                "The preview uses GUI fonts while preserving the configured sizes and colors. \
                 Font family names are applied by HTML, DOCX, and ODT output.",
            );
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("style_preview_scroll")
                .show(ui, |ui| {
                    render_document_sample(ui, theme);
                    ui.add_space(16.0);
                    render_theme_values(ui, theme);
                });
        });
}

fn render_document_sample(ui: &mut egui::Ui, theme: &StyleThemePreview) {
    let text_color = color(theme.text_color);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(600.0);
        ui.heading("Document sample");
        let (page_width, page_height) = theme.page_dimensions_pt();
        ui.small(format!(
            "Page: {} {} ({:.0} × {:.0} pt); margins: top {:.1}, right {:.1}, bottom {:.1}, left {:.1} pt",
            theme.page_size.name(),
            theme.page_orientation.name(),
            page_width,
            page_height,
            theme.margin_top_pt,
            theme.margin_right_pt,
            theme.margin_bottom_pt,
            theme.margin_left_pt
        ));
        ui.separator();

        render_page_text(ui, theme, &theme.header_text, "No header text");
        ui.add_space(8.0);

        for level in 1..=6u8 {
            ui.add_space(theme.heading_spacing_before_pt.min(24.0));
            let mut text = egui::RichText::new(format!("Heading {level} — Заголовок {level}"))
                .font(egui::FontId::proportional(points_to_pixels(
                    theme.heading_size_for(level),
                )))
                .color(color(theme.heading_color_for(level)));
            if theme.heading_bold {
                text = text.strong();
            }
            if theme.heading_italic {
                text = text.italics();
            }
            ui.label(text);
            ui.add_space(theme.heading_spacing_after_pt.min(16.0));
        }

        ui.add_space(theme.paragraph_spacing_before_pt.min(24.0));
        render_body_paragraph(ui, theme);
        ui.small(format!(
            "Line height: {:.2}; paragraph spacing: {:.1} pt before, {:.1} pt after; alignment: {}; first line: {:.1} pt",
            theme.line_height,
            theme.paragraph_spacing_before_pt,
            theme.paragraph_spacing_after_pt,
            theme.text_align.name(),
            theme.first_line_indent_pt
        ));
        ui.add_space(theme.paragraph_spacing_after_pt.min(24.0));

        render_quote(ui, theme);
        ui.add_space(theme.paragraph_spacing_after_pt.min(24.0));

        for (index, item) in ["First list item", "Second list item"].into_iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_space(points_to_pixels(theme.list_indent_pt) * index as f32);
                ui.label(
                    egui::RichText::new(format!("{} {item}", theme.list_bullet))
                        .font(egui::FontId::proportional(points_to_pixels(
                            theme.font_size_pt,
                        )))
                        .color(text_color),
                );
            });
        }
        ui.add_space(theme.paragraph_spacing_after_pt.min(24.0));

        egui::Frame::new()
            .fill(color(theme.code_background))
            .inner_margin(egui::Margin::same(
                points_to_pixels(theme.code_padding_pt).clamp(0.0, 40.0) as i8,
            ))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("fn main() {\n    println!(\"Styled code\");\n}")
                        .font(egui::FontId::monospace(points_to_pixels(
                            theme.code_font_size_pt,
                        )))
                        .color(color(theme.code_text_color)),
                );
            });
        ui.small(format!(
            "Code font: {} ({:.1} pt)",
            theme.code_font_family, theme.code_font_size_pt
        ));
        ui.add_space(12.0);

        render_sample_table(ui, theme);
        ui.add_space(12.0);

        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), points_to_pixels(theme.rule_width_pt) + 8.0),
            egui::Sense::hover(),
        );
        ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            egui::Stroke::new(
                points_to_pixels(theme.rule_width_pt),
                color(theme.rule_color),
            ),
        );
        ui.add_space(8.0);

        ui.label(
            egui::RichText::new("[^1]: Footnote text uses its own size.")
                .font(egui::FontId::proportional(points_to_pixels(
                    theme.footnote_font_size_pt,
                )))
                .color(text_color),
        );
        ui.small(format!(
            "Images: up to {:.0}% of the text width (PDF and HTML)",
            theme.image_max_width_percent
        ));
        ui.add_space(8.0);
        render_page_text(ui, theme, &theme.footer_text, "No footer text");
    });
}

fn render_page_text(ui: &mut egui::Ui, theme: &StyleThemePreview, text: &str, empty: &str) {
    ui.vertical_centered(|ui| {
        if text.is_empty() {
            ui.weak(empty);
        } else {
            ui.label(
                egui::RichText::new(text.replace("{page}", "1").replace("{pages}", "3"))
                    .font(egui::FontId::proportional(points_to_pixels(
                        theme.header_footer_font_size_pt,
                    )))
                    .color(color(theme.text_color)),
            );
        }
    });
}

fn render_body_paragraph(ui: &mut egui::Ui, theme: &StyleThemePreview) {
    let body_font = egui::FontId::proportional(points_to_pixels(theme.font_size_pt));
    let body = egui::TextFormat {
        font_id: body_font.clone(),
        color: color(theme.text_color),
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob {
        halign: match theme.text_align {
            StyleTextAlign::Left | StyleTextAlign::Justify => egui::Align::LEFT,
            StyleTextAlign::Center => egui::Align::Center,
            StyleTextAlign::Right => egui::Align::RIGHT,
        },
        justify: theme.text_align == StyleTextAlign::Justify,
        ..Default::default()
    };
    job.wrap.max_width = ui.available_width();
    job.append(
        "Body text demonstrates the selected size, color, alignment, and spacing. \
         Основной текст показывает размер, цвет, выравнивание и интервалы темы. See ",
        points_to_pixels(theme.first_line_indent_pt),
        body.clone(),
    );
    job.append(
        "a styled link",
        0.0,
        egui::TextFormat {
            color: color(theme.link_color),
            underline: if theme.link_underline {
                egui::Stroke::new(1.0, color(theme.link_color))
            } else {
                egui::Stroke::NONE
            },
            ..body.clone()
        },
    );
    job.append(" and ", 0.0, body.clone());
    job.append(
        "inline_code()",
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::monospace(points_to_pixels(theme.code_font_size_pt)),
            color: color(theme.code_text_color),
            background: theme
                .code_inline_background
                .map_or(egui::Color32::TRANSPARENT, color),
            ..body.clone()
        },
    );
    job.append(
        ", repeated so that the paragraph wraps over several lines and shows justification.",
        0.0,
        body,
    );
    ui.label(job);
}

fn render_quote(ui: &mut egui::Ui, theme: &StyleThemePreview) {
    let indent = points_to_pixels(theme.quote_indent_pt);
    let response = egui::Frame::new()
        .fill(
            theme
                .quote_background
                .map_or(egui::Color32::TRANSPARENT, color),
        )
        .inner_margin(egui::Margin {
            left: indent.clamp(0.0, 120.0) as i8,
            right: 4,
            top: 4,
            bottom: 4,
        })
        .show(ui, |ui| {
            let mut text = egui::RichText::new(
                "Blockquote sample with its own color, border, background, and indent.",
            )
            .font(egui::FontId::proportional(points_to_pixels(
                theme.font_size_pt,
            )))
            .color(color(theme.quote_text_color));
            if theme.quote_italic {
                text = text.italics();
            }
            ui.label(text);
        })
        .response;
    let x = response.rect.left() + indent / 2.0;
    ui.painter().vline(
        x,
        response.rect.y_range(),
        egui::Stroke::new(
            points_to_pixels(theme.quote_border_width_pt),
            color(theme.quote_border_color),
        ),
    );
}

fn render_sample_table(ui: &mut egui::Ui, theme: &StyleThemePreview) {
    let border = egui::Stroke::new(
        points_to_pixels(theme.table_border_width_pt).max(0.5),
        color(theme.table_border_color),
    );
    let rows = [
        ["Name", "Score"],
        ["Ada", "42"],
        ["Grace", "37"],
        ["Linus", "29"],
    ];
    egui::Grid::new("style_preview_table")
        .spacing(egui::vec2(0.0, 0.0))
        .show(ui, |ui| {
            for (row_index, row) in rows.into_iter().enumerate() {
                let (background, foreground) = if row_index == 0 {
                    (
                        color(theme.table_header_background),
                        color(theme.table_header_color),
                    )
                } else if row_index.is_multiple_of(2)
                    && let Some(stripe) = theme.table_stripe_background
                {
                    (color(stripe), color(theme.text_color))
                } else {
                    (ui.visuals().panel_fill, color(theme.text_color))
                };
                for text in row {
                    table_cell(
                        ui,
                        theme,
                        text,
                        (background, foreground),
                        border,
                        row_index == 0,
                    );
                }
                ui.end_row();
            }
        });
    ui.small(format!(
        "Table text {:.1} pt, cell padding {:.1} pt, border {:.2} pt",
        theme.table_font_size_pt, theme.table_cell_padding_pt, theme.table_border_width_pt
    ));
}

fn table_cell(
    ui: &mut egui::Ui,
    theme: &StyleThemePreview,
    text: &str,
    (background, foreground): (egui::Color32, egui::Color32),
    border: egui::Stroke,
    strong: bool,
) {
    let padding = points_to_pixels(theme.table_cell_padding_pt).clamp(0.0, 40.0) as i8;
    egui::Frame::new()
        .fill(background)
        .stroke(border)
        .inner_margin(egui::Margin::symmetric(padding.saturating_mul(2), padding))
        .show(ui, |ui| {
            let text = egui::RichText::new(text)
                .font(egui::FontId::proportional(points_to_pixels(
                    theme.table_font_size_pt,
                )))
                .color(foreground);
            ui.label(if strong { text.strong() } else { text });
        });
}

fn optional_hex(value: Option<StyleColor>) -> String {
    value.map_or_else(|| "none".to_string(), hex)
}

fn render_theme_values(ui: &mut egui::Ui, theme: &StyleThemePreview) {
    ui.heading("Resolved theme values");
    egui::Grid::new("resolved_style_values")
        .striped(true)
        .show(ui, |ui| {
            property(ui, "Body font", &theme.font_family);
            property(ui, "Body size", &format!("{:.1} pt", theme.font_size_pt));
            property(ui, "Body color", &hex(theme.text_color));
            property(ui, "Text alignment", theme.text_align.name());
            property(
                ui,
                "First-line indent",
                &format!("{:.1} pt", theme.first_line_indent_pt),
            );
            property(ui, "Heading font", &theme.heading_font_family);
            property(
                ui,
                "Heading colors",
                &theme
                    .heading_colors
                    .iter()
                    .map(|value| hex(*value))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            property(
                ui,
                "Heading sizes",
                &theme
                    .heading_sizes_pt
                    .iter()
                    .map(|size| format!("{size:.1}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            property(
                ui,
                "Heading weight / slant",
                &format!(
                    "{} / {}",
                    if theme.heading_bold { "bold" } else { "normal" },
                    if theme.heading_italic {
                        "italic"
                    } else {
                        "normal"
                    }
                ),
            );
            property(
                ui,
                "Page",
                &format!(
                    "{} {}",
                    theme.page_size.name(),
                    theme.page_orientation.name()
                ),
            );
            property(ui, "Header text", &theme.header_text);
            property(ui, "Footer text", &theme.footer_text);
            property(
                ui,
                "Link",
                &format!(
                    "{}{}",
                    hex(theme.link_color),
                    if theme.link_underline {
                        ", underlined"
                    } else {
                        ""
                    }
                ),
            );
            property(
                ui,
                "Blockquote",
                &format!(
                    "text {}, background {}, border {} {:.1} pt, indent {:.1} pt",
                    hex(theme.quote_text_color),
                    optional_hex(theme.quote_background),
                    hex(theme.quote_border_color),
                    theme.quote_border_width_pt,
                    theme.quote_indent_pt
                ),
            );
            property(
                ui,
                "Lists",
                &format!(
                    "indent {:.1} pt, bullet {}",
                    theme.list_indent_pt, theme.list_bullet
                ),
            );
            property(
                ui,
                "Code",
                &format!(
                    "{} {:.1} pt, text {}, block {}, inline {}, padding {:.1} pt",
                    theme.code_font_family,
                    theme.code_font_size_pt,
                    hex(theme.code_text_color),
                    hex(theme.code_background),
                    optional_hex(theme.code_inline_background),
                    theme.code_padding_pt
                ),
            );
            property(
                ui,
                "Table",
                &format!(
                    "header {} / {}, border {}, stripe {}",
                    hex(theme.table_header_background),
                    hex(theme.table_header_color),
                    hex(theme.table_border_color),
                    optional_hex(theme.table_stripe_background)
                ),
            );
            property(
                ui,
                "Horizontal rule",
                &format!("{} {:.1} pt", hex(theme.rule_color), theme.rule_width_pt),
            );
            property(
                ui,
                "Footnotes",
                &format!("{:.1} pt", theme.footnote_font_size_pt),
            );
            property(
                ui,
                "Image max width",
                &format!("{:.0}%", theme.image_max_width_percent),
            );
        });
}

fn property(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.strong(name);
    ui.monospace(value);
    ui.end_row();
}

fn color(value: StyleColor) -> egui::Color32 {
    egui::Color32::from_rgb(value.red, value.green, value.blue)
}

fn hex(value: StyleColor) -> String {
    format!("#{:02X}{:02X}{:02X}", value.red, value.green, value.blue)
}

fn points_to_pixels(points: f32) -> f32 {
    points * 96.0 / 72.0
}
