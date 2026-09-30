use super::content::{
    estimate_run_width, normalize_anchor, reorder_runs_for_display, slugify, uses_emoji_font,
    wrap_runs,
};
use super::*;
use crate::error::invalid_data;
use base64::Engine as _;
use epaint_default_fonts::HACK_REGULAR;
use pdfium_bundled::pdfium_render::prelude::{
    PdfPageAnnotationCommon, PdfPageContentRegenerationStrategy, PdfPageObjectCommon,
    PdfPageObjectsCommon, PdfPagePaperSize, PdfPoints, PdfRect,
};

fn split_runs_by_font(runs: &[TextRun]) -> Vec<TextRun> {
    let mut output: Vec<TextRun> = Vec::new();
    for run in runs {
        for character in run.text.chars() {
            let emoji = uses_emoji_font(character, run.style.code);
            if let Some(previous) = output.last_mut()
                && previous.style == run.style
                && previous
                    .text
                    .chars()
                    .next()
                    .is_some_and(|first| uses_emoji_font(first, previous.style.code) == emoji)
            {
                previous.text.push(character);
            } else {
                output.push(TextRun {
                    text: character.to_string(),
                    style: run.style.clone(),
                });
            }
        }
    }
    output
}

impl<'a> PdfWriter<'a> {
    pub(super) fn new(pdfium: &'a Pdfium) -> Result<Self, MarkoffError> {
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
            .create_page_at_end(PdfPagePaperSize::a4())
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
            cursor_y: page_height - PDF_MARGIN_TOP,
            body_font,
            body_bold_font,
            body_italic_font,
            body_bold_italic_font,
            code_font,
            headings: Vec::new(),
            anchors: HashMap::new(),
            heading_slugs: HashMap::new(),
            has_internal_links: false,
        })
    }

    fn page_mut(&mut self) -> &mut PdfPage<'a> {
        self.page
            .as_mut()
            .expect("PDF writer always has an active page")
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
            .create_page_at_end(PdfPagePaperSize::a4())
            .map_err(invalid_data)?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        self.page_width = page.width().value;
        self.page_height = page.height().value;
        self.cursor_y = self.page_height - PDF_MARGIN_TOP;
        self.page_index += 1;
        self.page = Some(page);
        Ok(())
    }

    fn ensure_space(&mut self, height: f32) -> Result<(), MarkoffError> {
        if self.cursor_y - height < PDF_MARGIN_BOTTOM {
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
                let left = PDF_MARGIN_LEFT + indent;
                let right = self.page_width - PDF_MARGIN_RIGHT;
                let y = self.cursor_y - 5.0;
                self.draw_line(left, y, right, y, TABLE_BORDER, PdfPoints::new(0.7))?;
                self.cursor_y -= 14.0;
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
        let available_width =
            self.page_width - PDF_MARGIN_LEFT - PDF_MARGIN_RIGHT - paragraph.options.indent;
        let lines = wrap_runs(
            &paragraph.runs,
            available_width,
            paragraph.options.font_size,
            paragraph.options.preserve_whitespace,
        );
        let line_height = paragraph.options.font_size * 1.35;

        for (line_index, logical_line) in lines.iter().enumerate() {
            let line = reorder_runs_for_display(logical_line);
            self.ensure_space(line_height)?;
            if line_index == 0
                && let Some((level, title)) = paragraph.options.heading.as_ref()
            {
                self.record_heading(*level, title);
            }

            let default_x = PDF_MARGIN_LEFT + paragraph.options.indent;
            let line_width = line
                .iter()
                .map(|run| estimate_run_width(run, paragraph.options.font_size))
                .sum::<f32>();
            let x = match paragraph.options.alignment {
                TextAlignment::Left => default_x,
                TextAlignment::Center => {
                    default_x + ((available_width - line_width) / 2.0).max(0.0)
                }
            };

            if paragraph.options.quote_depth > 0 {
                let quote_x = (default_x - 7.0).max(PDF_MARGIN_LEFT - 10.0);
                self.draw_line(
                    quote_x,
                    self.cursor_y - paragraph.options.font_size * 0.2,
                    quote_x,
                    self.cursor_y + paragraph.options.font_size * 0.8,
                    PdfColor::GREY_70,
                    PdfPoints::new(1.5),
                )?;
            }
            self.draw_runs(
                &line,
                x,
                self.cursor_y,
                paragraph.options.font_size,
                paragraph.options.color,
                false,
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
        code_block: bool,
    ) -> Result<(), MarkoffError> {
        let mut x = start_x;
        for run in split_runs_by_font(runs) {
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
                LINK_COLOR
            } else if run.style.code {
                PdfColor::GREY_20
            } else {
                base_color
            };
            let estimated_width = estimate_run_width(&run, base_size);
            let is_emoji = run
                .text
                .chars()
                .next()
                .is_some_and(|character| uses_emoji_font(character, run.style.code));

            if run.style.code && !code_block {
                self.draw_rectangle(
                    PdfRect::new_from_values(
                        y - font_size * 0.22,
                        x - 2.0,
                        y + font_size * 0.84,
                        x + estimated_width + 2.0,
                    ),
                    Some((TABLE_BORDER, PdfPoints::new(0.35))),
                    Some(CODE_BACKGROUND),
                )?;
            }

            if is_emoji {
                let width = self.draw_emoji_run(&run.text, x, y, font_size)?;
                let width = width.max(estimated_width);
                if run.style.underline || run.style.link.is_some() {
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
                x += width;
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
                .page_mut()
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

            if run.style.underline || run.style.link.is_some() {
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
            x += width;
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
        for character in text.chars().filter(|character| *character != '\u{fe0f}') {
            let emoji = character.to_string();
            let Some(asset) = twemoji_assets::png::PngTwemojiAsset::from_emoji(&emoji) else {
                x += font_size;
                continue;
            };
            let image = image::load_from_memory(asset.data.0).map_err(invalid_data)?;
            self.page_mut()
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
        let size = 9.5;
        let line_height = size * 1.4;
        let left = PDF_MARGIN_LEFT + indent;
        let width = (self.page_width - PDF_MARGIN_RIGHT - left).max(1.0);
        let content_width = (width - 14.0).max(1.0);
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
                    self.cursor_y + 3.0,
                    left + width,
                ),
                None,
                Some(CODE_BACKGROUND),
            )?;
            self.draw_runs(
                &line,
                left + 7.0,
                self.cursor_y,
                size,
                PdfColor::GREY_20,
                true,
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
        let left = PDF_MARGIN_LEFT + table.indent;
        let available_width = (self.page_width - PDF_MARGIN_RIGHT - left).max(1.0);
        let column_width = available_width / column_count as f32;
        let font_size = 9.5;
        let line_height = font_size * 1.3;
        let padding = 4.0;
        let layout = TableLayout {
            column_count,
            left,
            column_width,
            font_size,
            line_height,
            padding,
        };
        let max_lines_per_page =
            ((self.page_height - PDF_MARGIN_TOP - PDF_MARGIN_BOTTOM - padding * 2.0) / line_height)
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
                let line_count = (row_line_count - line_start).min(max_lines_per_segment);
                let row_height = line_count as f32 * line_height + padding * 2.0;
                if self.cursor_y - row_height < PDF_MARGIN_BOTTOM {
                    self.next_page()?;
                    if row_index > 0 {
                        let header = table.rows.first().cloned().unwrap_or_default();
                        if !header.is_empty() {
                            self.write_repeated_table_header(&header, layout)?;
                        }
                    }
                }
                self.draw_table_segment(
                    &wrapped,
                    line_start,
                    line_count,
                    table.alignments.as_slice(),
                    row_index == 0,
                    layout,
                )?;
                self.cursor_y -= row_height;
                line_start += line_count;
            }
        }
        self.cursor_y -= 8.0;
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
                .into_iter()
                .next()
                .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let height = layout.line_height + layout.padding * 2.0;
        self.ensure_space(height)?;
        self.draw_table_segment(
            &wrapped
                .iter()
                .map(|line| vec![line.clone()])
                .collect::<Vec<_>>(),
            0,
            1,
            &[],
            true,
            layout,
        )?;
        self.cursor_y -= height;
        Ok(())
    }

    fn draw_table_segment(
        &mut self,
        cells: &[Vec<Vec<TextRun>>],
        line_start: usize,
        line_count: usize,
        alignments: &[TableAlignment],
        header: bool,
        layout: TableLayout,
    ) -> Result<(), MarkoffError> {
        let row_height = line_count as f32 * layout.line_height + layout.padding * 2.0;
        let top = self.cursor_y;
        let bottom = top - row_height;
        let fill = if header {
            TABLE_HEADER_BACKGROUND
        } else {
            PdfColor::WHITE
        };

        for column in 0..layout.column_count {
            let cell_left = layout.left + column as f32 * layout.column_width;
            let cell_right = if column + 1 == layout.column_count {
                self.page_width - PDF_MARGIN_RIGHT
            } else {
                cell_left + layout.column_width
            };
            self.draw_rectangle(
                PdfRect::new_from_values(bottom, cell_left, top, cell_right),
                Some((TABLE_BORDER, PdfPoints::new(0.45))),
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
                    PdfColor::BLACK,
                    false,
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
        let left = PDF_MARGIN_LEFT + image.indent;
        let available_width = (self.page_width - PDF_MARGIN_RIGHT - left).max(1.0);
        let available_height =
            (self.page_height - PDF_MARGIN_TOP - PDF_MARGIN_BOTTOM - 35.0).max(1.0);
        let scale = (available_width / source_width)
            .min(available_height / source_height)
            .min(1.0);
        let width = source_width * scale;
        let height = source_height * scale;
        self.ensure_space(height + 8.0)?;
        let bottom = self.cursor_y - height;
        self.page_mut()
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
            .page_mut()
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
        self.page_mut()
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
        self.page_mut()
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
        self.document.save_to_file(output).map_err(invalid_data)?;
        if !self.headings.is_empty() || self.has_internal_links {
            navigation::add_pdf_navigation(output, &self.headings, &self.anchors)?;
        }
        Ok(())
    }
}
