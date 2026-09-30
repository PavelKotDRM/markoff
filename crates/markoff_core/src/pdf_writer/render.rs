use super::content::{
    estimate_run_width, normalize_anchor, reorder_runs_for_display, slugify, uses_emoji_font,
    wrap_runs, wrap_runs_with_first_line_indent,
};
use super::pdf_color;
use super::*;
use crate::error::invalid_data;
use base64::Engine as _;
use epaint_default_fonts::HACK_REGULAR;
use pdfium_bundled::pdfium_render::prelude::{
    PdfPageAnnotationCommon, PdfPageContentRegenerationStrategy, PdfPageObjectCommon,
    PdfPageObjectsCommon, PdfPagePaperSize, PdfPoints, PdfRect,
};
use unicode_segmentation::UnicodeSegmentation;

fn segment_line_counts(total_lines: usize, max_lines: usize) -> Vec<usize> {
    let max_lines = max_lines.max(1);
    let mut remaining = total_lines;
    let mut segments = Vec::new();
    while remaining > 0 {
        let count = remaining.min(max_lines);
        segments.push(count);
        remaining -= count;
    }
    segments
}

fn image_scale_to_fit(
    source_width: f32,
    source_height: f32,
    available_width: f32,
    available_height: f32,
) -> f32 {
    (available_width / source_width)
        .min(available_height / source_height)
        .min(1.0)
}

fn require_active_page<T>(page: Option<&mut T>) -> std::io::Result<&mut T> {
    page.ok_or_else(|| std::io::Error::other("PDF writer has no active page"))
}

fn split_runs_by_font(runs: &[TextRun]) -> Vec<TextRun> {
    let mut output: Vec<TextRun> = Vec::new();
    for run in runs {
        for grapheme in run.text.graphemes(true) {
            let emoji = uses_emoji_font(grapheme, run.style.code);
            if let Some(previous) = output.last_mut()
                && previous.style == run.style
                && previous
                    .text
                    .graphemes(true)
                    .next()
                    .is_some_and(|first| uses_emoji_font(first, previous.style.code) == emoji)
            {
                previous.text.push_str(grapheme);
            } else {
                output.push(TextRun {
                    text: grapheme.to_string(),
                    style: run.style.clone(),
                });
            }
        }
    }
    output
}

#[derive(Clone, Copy, Default)]
struct RunLayout {
    code_block: bool,
    word_spacing: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableRowKind {
    Header,
    Body,
    Striped,
}

impl TableRowKind {
    fn for_index(row_index: usize) -> Self {
        match row_index {
            0 => Self::Header,
            index if index % 2 == 0 => Self::Striped,
            _ => Self::Body,
        }
    }
}

fn split_runs_after_spaces(runs: Vec<TextRun>) -> Vec<TextRun> {
    let mut output = Vec::with_capacity(runs.len());
    for run in runs {
        let mut rest = run.text.as_str();
        while let Some(position) = rest.find(' ') {
            let (piece, tail) = rest.split_at(position + 1);
            output.push(TextRun {
                text: piece.to_string(),
                style: run.style.clone(),
            });
            rest = tail;
        }
        if !rest.is_empty() {
            output.push(TextRun {
                text: rest.to_string(),
                style: run.style.clone(),
            });
        }
    }
    output
}

fn paper_size(theme: &DocumentTheme) -> PdfPagePaperSize {
    let (width, height) = theme.page_dimensions_pt();
    if theme.page_size == crate::style::StylePageSize::A4
        && theme.page_orientation == crate::style::StylePageOrientation::Portrait
    {
        PdfPagePaperSize::a4()
    } else {
        PdfPagePaperSize::new_custom(PdfPoints::new(width), PdfPoints::new(height))
    }
}

impl<'a> PdfWriter<'a> {
    pub(super) fn new(pdfium: &'a Pdfium, theme: &DocumentTheme) -> Result<Self, MarkoffError> {
        let mut document = pdfium.create_new_pdf().map_err(invalid_data)?;
        let body_font = document
            .fonts_mut()
            .load_true_type_from_bytes(dejavu::sans::regular(), true)
            .map_err(invalid_data)?;
        let body_bold_font = document
            .fonts_mut()
            .load_true_type_from_bytes(dejavu::sans::bold(), true)
            .map_err(invalid_data)?;
        let body_italic_font = document
            .fonts_mut()
            .load_true_type_from_bytes(dejavu::sans::oblique(), true)
            .map_err(invalid_data)?;
        let body_bold_italic_font = document
            .fonts_mut()
            .load_true_type_from_bytes(dejavu::sans::bold_oblique(), true)
            .map_err(invalid_data)?;
        let code_font = document
            .fonts_mut()
            .load_true_type_from_bytes(HACK_REGULAR, true)
            .map_err(invalid_data)?;
        let mut page = document
            .pages_mut()
            .create_page_at_end(paper_size(theme))
            .map_err(invalid_data)?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        let page_width = page.width().value;
        let page_height = page.height().value;

        Ok(Self {
            page: Some(page),
            document,
            page_width,
            page_height,
            page_index: 0,
            cursor_y: page_height - theme.margin_top_pt,
            body_font,
            body_bold_font,
            body_italic_font,
            body_bold_italic_font,
            code_font,
            headings: Vec::new(),
            anchors: HashMap::new(),
            heading_slugs: HashMap::new(),
            has_internal_links: false,
            theme: theme.clone(),
        })
    }

    fn page_mut(&mut self) -> Result<&mut PdfPage<'a>, MarkoffError> {
        require_active_page(self.page.as_mut())
            .map_err(invalid_data)
            .map_err(Into::into)
    }

    fn commit_page(&mut self) -> Result<(), MarkoffError> {
        if let Some(mut page) = self.page.take() {
            page.regenerate_content().map_err(invalid_data)?;
        }
        Ok(())
    }

    fn next_page(&mut self) -> Result<(), MarkoffError> {
        self.commit_page()?;
        let mut page = self
            .document
            .pages_mut()
            .create_page_at_end(paper_size(&self.theme))
            .map_err(invalid_data)?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        self.page_width = page.width().value;
        self.page_height = page.height().value;
        self.cursor_y = self.page_height - self.theme.margin_top_pt;
        self.page_index += 1;
        self.page = Some(page);
        Ok(())
    }

    fn ensure_space(&mut self, height: f32) -> Result<(), MarkoffError> {
        if self.cursor_y - height < self.theme.margin_bottom_pt {
            self.next_page()?;
        }
        Ok(())
    }

    pub(super) fn write_element(&mut self, element: PdfElement) -> Result<(), MarkoffError> {
        match element {
            PdfElement::Paragraph(paragraph) => self.write_paragraph(paragraph),
            PdfElement::CodeBlock { code, info, indent } => {
                self.write_code_block(&code, info.as_deref(), indent)
            }
            PdfElement::Table(table) => self.write_table(table),
            PdfElement::Image(image) => self.write_image(image),
            PdfElement::HorizontalRule { indent } => {
                self.ensure_space(16.0)?;
                let left = self.theme.margin_left_pt + indent;
                let right = self.page_width - self.theme.margin_right_pt;
                let y = self.cursor_y + 3.0;
                self.draw_line(
                    left,
                    y,
                    right,
                    y,
                    pdf_color(self.theme.rule_color),
                    PdfPoints::new(self.theme.rule_width_pt),
                )?;
                self.cursor_y -= 12.0 + self.theme.rule_width_pt;
                Ok(())
            }
            PdfElement::Anchor(name) => {
                self.anchors
                    .insert(normalize_anchor(&name), self.page_index);
                Ok(())
            }
        }
    }

    fn write_paragraph(&mut self, paragraph: PdfParagraph) -> Result<(), MarkoffError> {
        self.cursor_y -= paragraph.options.space_before;
        let available_width = self.page_width
            - self.theme.margin_left_pt
            - self.theme.margin_right_pt
            - paragraph.options.indent;
        let first_line_indent = match paragraph.options.alignment {
            TextAlignment::Left | TextAlignment::Justify => paragraph
                .options
                .first_line_indent
                .min(available_width / 2.0),
            TextAlignment::Center | TextAlignment::Right => 0.0,
        };
        let lines = wrap_runs_with_first_line_indent(
            &paragraph.runs,
            available_width,
            first_line_indent,
            paragraph.options.font_size,
            paragraph.options.preserve_whitespace,
        );
        let line_count = lines.len();
        let line_height = paragraph.options.font_size
            * if self.theme.enabled {
                self.theme.line_height
            } else {
                1.35
            };

        for (line_index, logical_line) in lines.iter().enumerate() {
            let line = reorder_runs_for_display(logical_line);
            self.ensure_space(line_height)?;
            if line_index == 0
                && let Some((level, title)) = paragraph.options.heading.as_ref()
            {
                self.record_heading(*level, title);
            }

            let default_x = self.theme.margin_left_pt + paragraph.options.indent;
            let line_width = line
                .iter()
                .map(|run| estimate_run_width(run, paragraph.options.font_size))
                .sum::<f32>();
            let line_indent = if line_index == 0 {
                first_line_indent
            } else {
                0.0
            };
            let free_width = (available_width - line_indent - line_width).max(0.0);
            let x = match paragraph.options.alignment {
                TextAlignment::Left | TextAlignment::Justify => default_x + line_indent,
                TextAlignment::Center => default_x + free_width / 2.0,
                TextAlignment::Right => default_x + free_width,
            };
            let word_spacing = if matches!(paragraph.options.alignment, TextAlignment::Justify)
                && !paragraph.options.preserve_whitespace
                && line_index + 1 < line_count
            {
                let gaps = line
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>()
                    .trim_end()
                    .matches(' ')
                    .count();
                if gaps > 0 {
                    free_width / gaps as f32
                } else {
                    0.0
                }
            } else {
                0.0
            };

            if paragraph.options.quote_depth > 0 {
                let quote_x = (default_x - self.theme.quote_indent_pt / 2.0)
                    .max(self.theme.margin_left_pt - 10.0);
                if let Some(background) = self.theme.quote_background.filter(|_| self.theme.enabled)
                {
                    self.draw_rectangle(
                        PdfRect::new_from_values(
                            self.cursor_y + paragraph.options.font_size * 0.8 - line_height,
                            quote_x,
                            self.cursor_y + paragraph.options.font_size * 0.8,
                            self.page_width - self.theme.margin_right_pt,
                        ),
                        None,
                        Some(pdf_color(background)),
                    )?;
                }
                self.draw_line(
                    quote_x,
                    self.cursor_y + paragraph.options.font_size * 0.8 - line_height,
                    quote_x,
                    self.cursor_y + paragraph.options.font_size * 0.8,
                    pdf_color(self.theme.quote_border_color),
                    PdfPoints::new(self.theme.quote_border_width_pt),
                )?;
            }
            self.draw_runs(
                &line,
                x,
                self.cursor_y,
                paragraph.options.font_size,
                paragraph.options.color,
                RunLayout {
                    code_block: false,
                    word_spacing,
                },
            )?;
            self.cursor_y -= line_height;
        }
        self.cursor_y -= paragraph.options.space_after;
        Ok(())
    }

    fn record_heading(&mut self, level: u8, title: &str) {
        if title.trim().is_empty() {
            return;
        }
        let base_slug = slugify(title);
        if !base_slug.is_empty() {
            let count = self.heading_slugs.entry(base_slug.clone()).or_default();
            let slug = if *count == 0 {
                base_slug
            } else {
                format!("{base_slug}-{}", *count)
            };
            *count += 1;
            self.anchors
                .insert(normalize_anchor(&slug), self.page_index);
        }
        self.headings.push(HeadingOutline {
            title: title.to_string(),
            level,
            page: self.page_index,
        });
    }

    fn draw_runs(
        &mut self,
        runs: &[TextRun],
        start_x: f32,
        baseline: f32,
        base_size: f32,
        base_color: PdfColor,
        layout: RunLayout,
    ) -> Result<(), MarkoffError> {
        let mut x = start_x;
        let theme_enabled = self.theme.enabled;
        let link_color = if theme_enabled {
            pdf_color(self.theme.link_color)
        } else {
            LINK_COLOR
        };
        let underline_links = !theme_enabled || self.theme.link_underline;
        let inline_code_background = if theme_enabled {
            self.theme.code_inline_background
        } else {
            Some(self.theme.code_background)
        };
        let runs = split_runs_by_font(runs);
        let runs = if layout.word_spacing > 0.0 {
            split_runs_after_spaces(runs)
        } else {
            runs
        };
        for run in runs {
            if run.text.is_empty() {
                continue;
            }
            let font_size = if run.style.superscript || run.style.subscript {
                base_size * 0.72
            } else {
                base_size
            };
            let y = if run.style.superscript {
                baseline + base_size * 0.32
            } else if run.style.subscript {
                baseline - base_size * 0.2
            } else {
                baseline
            };
            let color = if run.style.link.is_some() {
                link_color
            } else if run.style.code {
                pdf_color(self.theme.code_text_color)
            } else {
                base_color
            };
            let underline = run.style.underline || (run.style.link.is_some() && underline_links);
            let trailing_spacing = if run.text.ends_with(' ') {
                layout.word_spacing
            } else {
                0.0
            };
            let estimated_width = estimate_run_width(&run, base_size);
            let is_emoji = run
                .text
                .graphemes(true)
                .next()
                .is_some_and(|grapheme| uses_emoji_font(grapheme, run.style.code));

            if run.style.code
                && !layout.code_block
                && let Some(background) = inline_code_background
            {
                self.draw_rectangle(
                    PdfRect::new_from_values(
                        y - font_size * 0.22,
                        x - 2.0,
                        y + font_size * 0.84,
                        x + estimated_width + 2.0,
                    ),
                    Some((
                        pdf_color(self.theme.table_border_color),
                        PdfPoints::new(0.35),
                    )),
                    Some(pdf_color(background)),
                )?;
            }

            if is_emoji {
                let width = self.draw_emoji_run(&run.text, x, y, font_size)?;
                let width = width.max(estimated_width);
                if underline {
                    self.draw_line(x, y - 1.5, x + width, y - 1.5, color, PdfPoints::new(0.45))?;
                }
                if run.style.strikethrough {
                    self.draw_line(
                        x,
                        y + font_size * 0.31,
                        x + width,
                        y + font_size * 0.31,
                        color,
                        PdfPoints::new(0.45),
                    )?;
                }
                if let Some(destination) = run.style.link.as_deref() {
                    self.add_link_annotation(destination, x, y, width, font_size)?;
                }
                x += width + trailing_spacing;
                continue;
            }

            let font = if run.style.code {
                self.code_font
            } else if run.style.bold && run.style.italic {
                self.body_bold_italic_font
            } else if run.style.bold {
                self.body_bold_font
            } else if run.style.italic {
                self.body_italic_font
            } else {
                self.body_font
            };
            let mut object = self
                .page_mut()?
                .objects_mut()
                .create_text_object(
                    PdfPoints::new(x),
                    PdfPoints::new(y),
                    &run.text,
                    font,
                    PdfPoints::new(font_size),
                )
                .map_err(invalid_data)?;
            let text = object
                .as_text_object_mut()
                .ok_or_else(|| invalid_data(std::io::Error::other("expected a PDF text object")))?;
            text.set_fill_color(color).map_err(invalid_data)?;
            let width = object
                .bounds()
                .map_err(invalid_data)?
                .width()
                .value
                .max(estimated_width);
            drop(object);

            if underline {
                self.draw_line(x, y - 1.5, x + width, y - 1.5, color, PdfPoints::new(0.45))?;
            }
            if run.style.strikethrough {
                self.draw_line(
                    x,
                    y + font_size * 0.31,
                    x + width,
                    y + font_size * 0.31,
                    color,
                    PdfPoints::new(0.45),
                )?;
            }
            if let Some(destination) = run.style.link.as_deref() {
                self.add_link_annotation(destination, x, y, width, font_size)?;
            }
            x += width + trailing_spacing;
        }
        Ok(())
    }

    fn draw_emoji_run(
        &mut self,
        text: &str,
        start_x: f32,
        baseline: f32,
        font_size: f32,
    ) -> Result<f32, MarkoffError> {
        let mut x = start_x;
        for emoji in text.graphemes(true) {
            let Some(asset) = twemoji_assets::png::PngTwemojiAsset::from_emoji(emoji) else {
                x += font_size;
                continue;
            };
            let image = image::load_from_memory(asset.data.0).map_err(invalid_data)?;
            self.page_mut()?
                .objects_mut()
                .create_image_object(
                    PdfPoints::new(x),
                    PdfPoints::new(baseline - font_size * 0.22),
                    &image,
                    Some(PdfPoints::new(font_size)),
                    Some(PdfPoints::new(font_size)),
                )
                .map_err(invalid_data)?;
            x += font_size;
        }
        Ok(x - start_x)
    }

    fn write_code_block(
        &mut self,
        code: &str,
        info: Option<&str>,
        indent: f32,
    ) -> Result<(), MarkoffError> {
        if let Some(info) = info.filter(|info| !info.is_empty()) {
            let label = PdfParagraph {
                runs: vec![TextRun {
                    text: format!("[{info}]"),
                    style: InlineStyle::default(),
                }],
                options: ParagraphOptions {
                    font_size: 8.5,
                    indent,
                    space_before: 0.0,
                    space_after: 2.0,
                    first_line_indent: 0.0,
                    preserve_whitespace: false,
                    quote_depth: 0,
                    alignment: TextAlignment::Left,
                    color: PdfColor::GREY_50,
                    heading: None,
                },
            };
            self.write_paragraph(label)?;
        }

        let code = code.replace('\t', "    ");
        let size = self.theme.code_font_size_pt;
        let padding = self.theme.code_padding_pt;
        let line_height = size * 1.4;
        let left = self.theme.margin_left_pt + indent;
        let width = (self.page_width - self.theme.margin_right_pt - left).max(1.0);
        let content_width = (width - padding * 2.0).max(1.0);
        let source_lines = if code.is_empty() {
            vec![String::new()]
        } else {
            code.split('\n').map(str::to_string).collect()
        };
        let mut code_lines = Vec::new();
        for source_line in source_lines {
            let run = TextRun {
                text: source_line,
                style: InlineStyle {
                    code: true,
                    ..InlineStyle::default()
                },
            };
            code_lines.extend(wrap_runs(&[run], content_width, size, true));
        }

        self.cursor_y -= 2.0;
        for line in code_lines {
            self.ensure_space(line_height + 2.0)?;
            self.draw_rectangle(
                PdfRect::new_from_values(
                    self.cursor_y - line_height + 2.0,
                    left,
                    self.cursor_y + size,
                    left + width,
                ),
                None,
                Some(pdf_color(self.theme.code_background)),
            )?;
            self.draw_runs(
                &line,
                left + padding,
                self.cursor_y,
                size,
                pdf_color(self.theme.code_text_color),
                RunLayout {
                    code_block: true,
                    word_spacing: 0.0,
                },
            )?;
            self.cursor_y -= line_height;
        }
        self.cursor_y -= 7.0;
        Ok(())
    }

    fn write_table(&mut self, table: PdfTable) -> Result<(), MarkoffError> {
        let column_count = table.rows.iter().map(Vec::len).max().unwrap_or_default();
        if column_count == 0 {
            return Ok(());
        }
        let left = self.theme.margin_left_pt + table.indent;
        let available_width = (self.page_width - self.theme.margin_right_pt - left).max(1.0);
        let column_width = available_width / column_count as f32;
        let font_size = self.theme.table_font_size_pt;
        let line_height = font_size * 1.3;
        let padding = self.theme.table_cell_padding_pt;
        let layout = TableLayout {
            column_count,
            left,
            column_width,
            font_size,
            line_height,
            padding,
        };
        let max_lines_per_page = ((self.page_height
            - self.theme.margin_top_pt
            - self.theme.margin_bottom_pt
            - padding * 2.0)
            / line_height)
            .floor()
            .max(1.0) as usize;
        let max_lines_per_segment = if table.rows.len() > 1 {
            max_lines_per_page.saturating_sub(1).max(1)
        } else {
            max_lines_per_page
        };

        for (row_index, row) in table.rows.iter().enumerate() {
            let mut cells = row.clone();
            cells.resize_with(column_count, Vec::new);
            if row_index == 0 {
                for cell in &mut cells {
                    for run in cell {
                        run.style.bold = true;
                    }
                }
            }
            let wrapped = cells
                .iter()
                .map(|cell| {
                    let lines = wrap_runs(
                        cell,
                        (column_width - padding * 2.0).max(1.0),
                        font_size,
                        false,
                    );
                    if lines.is_empty() {
                        vec![Vec::new()]
                    } else {
                        lines
                    }
                })
                .collect::<Vec<_>>();
            let row_line_count = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let mut line_start = 0;

            while line_start < row_line_count {
                let mut line_count = (row_line_count - line_start).min(max_lines_per_segment);
                let mut row_height = line_count as f32 * line_height + padding * 2.0;
                if self.cursor_y - row_height < self.theme.margin_bottom_pt {
                    self.next_page()?;
                    if row_index > 0 {
                        let header = table.rows.first().cloned().unwrap_or_default();
                        if !header.is_empty() {
                            self.write_repeated_table_header(&header, layout)?;
                        }
                    }
                    let available_lines =
                        ((self.cursor_y - self.theme.margin_bottom_pt - padding * 2.0)
                            / line_height)
                            .floor()
                            .max(1.0) as usize;
                    line_count = line_count.min(available_lines);
                    row_height = line_count as f32 * line_height + padding * 2.0;
                }
                self.draw_table_segment(
                    &wrapped,
                    line_start,
                    line_count,
                    table.alignments.as_slice(),
                    TableRowKind::for_index(row_index),
                    layout,
                )?;
                self.cursor_y -= row_height;
                line_start += line_count;
            }
        }
        self.cursor_y -= TABLE_SPACE_AFTER;
        Ok(())
    }

    fn write_repeated_table_header(
        &mut self,
        header: &[Vec<TextRun>],
        layout: TableLayout,
    ) -> Result<(), MarkoffError> {
        let mut cells = header.to_vec();
        cells.resize_with(layout.column_count, Vec::new);
        for cell in &mut cells {
            for run in cell {
                run.style.bold = true;
            }
        }
        let wrapped = cells
            .iter()
            .map(|cell| {
                wrap_runs(
                    cell,
                    (layout.column_width - layout.padding * 2.0).max(1.0),
                    layout.font_size,
                    false,
                )
            })
            .collect::<Vec<_>>();
        let line_count = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let page_lines = ((self.page_height
            - self.theme.margin_top_pt
            - self.theme.margin_bottom_pt
            - layout.padding * 2.0)
            / layout.line_height)
            .floor()
            .max(2.0) as usize;
        let mut line_start = 0;
        for segment_lines in segment_line_counts(line_count, page_lines.saturating_sub(1)) {
            let height = segment_lines as f32 * layout.line_height + layout.padding * 2.0;
            if self.cursor_y - height < self.theme.margin_bottom_pt {
                self.next_page()?;
            }
            self.draw_table_segment(
                &wrapped,
                line_start,
                segment_lines,
                &[],
                TableRowKind::Header,
                layout,
            )?;
            self.cursor_y -= height;
            line_start += segment_lines;
        }
        Ok(())
    }

    fn draw_table_segment(
        &mut self,
        cells: &[Vec<Vec<TextRun>>],
        line_start: usize,
        line_count: usize,
        alignments: &[TableAlignment],
        row: TableRowKind,
        layout: TableLayout,
    ) -> Result<(), MarkoffError> {
        let row_height = line_count as f32 * layout.line_height + layout.padding * 2.0;
        let top = self.cursor_y;
        let bottom = top - row_height;
        let header = row == TableRowKind::Header;
        let fill = match row {
            TableRowKind::Header => pdf_color(self.theme.table_header_background),
            TableRowKind::Striped => self
                .theme
                .table_stripe_background
                .filter(|_| self.theme.enabled)
                .map_or(PdfColor::WHITE, pdf_color),
            TableRowKind::Body => PdfColor::WHITE,
        };
        let border_width = if self.theme.enabled {
            self.theme.table_border_width_pt
        } else {
            0.45
        };

        for column in 0..layout.column_count {
            let cell_left = layout.left + column as f32 * layout.column_width;
            let cell_right = if column + 1 == layout.column_count {
                self.page_width - self.theme.margin_right_pt
            } else {
                cell_left + layout.column_width
            };
            self.draw_rectangle(
                PdfRect::new_from_values(bottom, cell_left, top, cell_right),
                Some((
                    pdf_color(self.theme.table_border_color),
                    PdfPoints::new(border_width),
                )),
                Some(fill),
            )?;

            let lines = cells.get(column).map(Vec::as_slice).unwrap_or_default();
            for local_line in 0..line_count {
                let Some(line) = lines.get(line_start + local_line) else {
                    continue;
                };
                let width = line
                    .iter()
                    .map(|run| estimate_run_width(run, layout.font_size))
                    .sum::<f32>();
                let content_width = (cell_right - cell_left - layout.padding * 2.0).max(1.0);
                let alignment = alignments
                    .get(column)
                    .copied()
                    .unwrap_or(TableAlignment::None);
                let offset = match alignment {
                    TableAlignment::None | TableAlignment::Left => 0.0,
                    TableAlignment::Center => ((content_width - width) / 2.0).max(0.0),
                    TableAlignment::Right => (content_width - width).max(0.0),
                };
                let baseline = top
                    - layout.padding
                    - layout.font_size
                    - local_line as f32 * layout.line_height;
                self.draw_runs(
                    line,
                    cell_left + layout.padding + offset,
                    baseline,
                    layout.font_size,
                    if header {
                        pdf_color(self.theme.table_header_color)
                    } else {
                        pdf_color(self.theme.text_color)
                    },
                    RunLayout::default(),
                )?;
            }
        }
        Ok(())
    }

    fn write_image(&mut self, image: PdfImage) -> Result<(), MarkoffError> {
        let dynamic_image = image::load_from_memory(&image.bytes).map_err(invalid_data)?;
        let source_width = dynamic_image.width() as f32;
        let source_height = dynamic_image.height() as f32;
        if source_width <= 0.0 || source_height <= 0.0 {
            return Err(invalid_data(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "image has no drawable area",
            ))
            .into());
        }
        let left = self.theme.margin_left_pt + image.indent;
        let available_width = ((self.page_width - self.theme.margin_right_pt - left)
            * self.theme.image_max_width_percent
            / 100.0)
            .max(1.0);
        let available_height =
            (self.page_height - self.theme.margin_top_pt - self.theme.margin_bottom_pt - 35.0)
                .max(1.0);
        let scale = image_scale_to_fit(
            source_width,
            source_height,
            available_width,
            available_height,
        );
        let width = source_width * scale;
        let height = source_height * scale;
        self.ensure_space(height + 8.0)?;
        let bottom = self.cursor_y - height;
        self.page_mut()?
            .objects_mut()
            .create_image_object(
                PdfPoints::new(left),
                PdfPoints::new(bottom),
                &dynamic_image,
                Some(PdfPoints::new(width)),
                Some(PdfPoints::new(height)),
            )
            .map_err(invalid_data)?;
        if let Some(link) = image.link.as_deref() {
            self.add_link_annotation_bounds(
                link,
                PdfRect::new_from_values(bottom, left, bottom + height, left + width),
            )?;
        }
        self.cursor_y = bottom - 5.0;
        if !image.alt.is_empty() {
            let caption = PdfParagraph {
                runs: vec![TextRun {
                    text: image.alt,
                    style: InlineStyle {
                        italic: true,
                        ..InlineStyle::default()
                    },
                }],
                options: ParagraphOptions {
                    font_size: 9.0,
                    indent: image.indent,
                    space_before: 0.0,
                    space_after: 6.0,
                    first_line_indent: 0.0,
                    preserve_whitespace: false,
                    quote_depth: 0,
                    alignment: TextAlignment::Center,
                    color: PdfColor::GREY_30,
                    heading: None,
                },
            };
            self.write_paragraph(caption)?;
        }
        Ok(())
    }

    fn add_link_annotation(
        &mut self,
        destination: &str,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Result<(), MarkoffError> {
        if width <= 0.0 || height <= 0.0 {
            return Ok(());
        }
        self.add_link_annotation_bounds(
            destination,
            PdfRect::new_from_values(y - height * 0.25, x, y + height * 0.8, x + width),
        )
    }

    fn add_link_annotation_bounds(
        &mut self,
        destination: &str,
        bounds: PdfRect,
    ) -> Result<(), MarkoffError> {
        if bounds.width().value <= 0.0 || bounds.height().value <= 0.0 {
            return Ok(());
        }
        let uri = if let Some(anchor) = destination.strip_prefix('#') {
            self.has_internal_links = true;
            let encoded =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(anchor.as_bytes());
            format!("{INTERNAL_LINK_PREFIX}{encoded}")
        } else {
            destination.to_string()
        };
        let mut annotation = self
            .page_mut()?
            .annotations_mut()
            .create_link_annotation(&uri)
            .map_err(invalid_data)?;
        annotation.set_bounds(bounds).map_err(invalid_data)?;
        Ok(())
    }

    fn draw_rectangle(
        &mut self,
        bounds: PdfRect,
        stroke: Option<(PdfColor, PdfPoints)>,
        fill: Option<PdfColor>,
    ) -> Result<(), MarkoffError> {
        let (stroke_color, stroke_width) = stroke
            .map(|(color, width)| (Some(color), Some(width)))
            .unwrap_or((None, None));
        self.page_mut()?
            .objects_mut()
            .create_path_object_rect(bounds, stroke_color, stroke_width, fill)
            .map_err(invalid_data)?;
        Ok(())
    }

    fn draw_line(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: PdfColor,
        width: PdfPoints,
    ) -> Result<(), MarkoffError> {
        self.page_mut()?
            .objects_mut()
            .create_path_object_line(
                PdfPoints::new(x1),
                PdfPoints::new(y1),
                PdfPoints::new(x2),
                PdfPoints::new(y2),
                color,
                width,
            )
            .map_err(invalid_data)?;
        Ok(())
    }

    pub(super) fn finish(mut self, output: &Path) -> Result<(), MarkoffError> {
        self.commit_page()?;
        self.draw_page_furniture()?;
        self.document.save_to_file(output).map_err(invalid_data)?;
        if !self.headings.is_empty() || self.has_internal_links {
            navigation::add_pdf_navigation(output, &self.headings, &self.anchors)?;
        }
        Ok(())
    }

    /// Draws the configured header and footer text on every finished page.
    fn draw_page_furniture(&mut self) -> Result<(), MarkoffError> {
        let header = self.theme.header_text.clone();
        let footer = self.theme.footer_text.clone();
        if header.is_empty() && footer.is_empty() {
            return Ok(());
        }
        let size = self.theme.header_footer_font_size_pt;
        let color = pdf_color(self.theme.text_color);
        let font = self.body_font;
        let page_count = self.document.pages().len();
        for index in 0..page_count {
            let mut page = self.document.pages().get(index).map_err(invalid_data)?;
            page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            let page_width = page.width().value;
            let page_height = page.height().value;
            let placements = [
                (
                    &header,
                    page_height - self.theme.margin_top_pt / 2.0 - size * 0.35,
                ),
                (&footer, self.theme.margin_bottom_pt / 2.0 - size * 0.35),
            ];
            for (template, baseline) in placements {
                let text = expand_page_fields(template, index + 1, page_count);
                if text.trim().is_empty() {
                    continue;
                }
                let width = estimate_run_width(
                    &TextRun {
                        text: text.clone(),
                        style: InlineStyle::default(),
                    },
                    size,
                );
                let x = ((page_width - width) / 2.0).max(0.0);
                let mut object = page
                    .objects_mut()
                    .create_text_object(
                        PdfPoints::new(x),
                        PdfPoints::new(baseline.max(0.0)),
                        &text,
                        font,
                        PdfPoints::new(size),
                    )
                    .map_err(invalid_data)?;
                if let Some(text_object) = object.as_text_object_mut() {
                    text_object.set_fill_color(color).map_err(invalid_data)?;
                }
            }
            page.regenerate_content().map_err(invalid_data)?;
        }
        Ok(())
    }
}

fn expand_page_fields(template: &str, page: i32, pages: i32) -> String {
    crate::style::page_field_segments(template)
        .into_iter()
        .map(|segment| match segment {
            crate::style::PageTextSegment::Text(text) => text.to_string(),
            crate::style::PageTextSegment::Page => page.to_string(),
            crate::style::PageTextSegment::Pages => pages.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_table_content_is_split_without_dropping_lines() {
        let segments = segment_line_counts(251, 37);
        assert!(segments.iter().all(|count| *count <= 37));
        assert_eq!(segments.iter().sum::<usize>(), 251);
    }

    #[test]
    fn oversized_image_is_scaled_to_fit_both_page_dimensions() {
        let scale = image_scale_to_fit(800.0, 8_000.0, 500.0, 700.0);
        assert!(800.0 * scale <= 500.0);
        assert!(8_000.0 * scale <= 700.0);
        assert_eq!(scale, 0.0875);
    }

    #[test]
    fn missing_active_page_returns_an_error() {
        let error = require_active_page(None::<&mut ()>).unwrap_err();
        assert!(error.to_string().contains("no active page"));
    }
}
