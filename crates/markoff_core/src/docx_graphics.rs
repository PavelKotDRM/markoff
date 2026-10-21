use crate::MarkoffError;
use crate::docx_resources::read_media_part;
use crate::error::invalid_data;
use crate::xml_utils::{attribute_value, xml_escape};
use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Clone, Copy)]
enum GraphicKind {
    Shape,
    Diagram,
    Chart,
    Picture,
}

struct Graphic {
    kind: GraphicKind,
    name: String,
    description: Option<String>,
    geometry: String,
    width: f64,
    height: f64,
    colors: Vec<String>,
    relationship: Option<String>,
    labels: Vec<String>,
    chart_series: Vec<ChartSeries>,
}

#[derive(Clone)]
struct ChartSeries {
    name: String,
    categories: Vec<String>,
    values: Vec<f64>,
}

#[derive(Clone, Copy)]
enum ChartSection {
    Name,
    Category,
    Value,
}

pub(crate) fn extract_svg_graphics(
    document: &str,
    archive: &mut zip::ZipArchive<std::fs::File>,
    relationships: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<Option<String>>, MarkoffError> {
    let mut reader = Reader::from_str(document);
    reader.config_mut().trim_text(false);
    let mut graphics = Vec::new();
    let mut current = None;
    let mut in_text = false;

    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Empty(event) if event.local_name().as_ref() == "t" => {}
            Event::Start(event) | Event::Empty(event) => match event.local_name().as_ref() {
                "drawing" => current = Some(Graphic::default()),
                "docPr" => {
                    if let Some(graphic) = current.as_mut() {
                        graphic.name =
                            attribute_value(&event, "name")?.unwrap_or_else(|| "Graphic".into());
                        graphic.description = attribute_value(&event, "descr")?;
                    }
                }
                "extent" => {
                    if let Some(graphic) = current.as_mut() {
                        graphic.width = attribute_value(&event, "cx")?
                            .and_then(|value| value.parse::<f64>().ok())
                            .unwrap_or_default();
                        graphic.height = attribute_value(&event, "cy")?
                            .and_then(|value| value.parse::<f64>().ok())
                            .unwrap_or_default();
                    }
                }
                "graphicData" => {
                    if let Some(graphic) = current.as_mut() {
                        let uri = attribute_value(&event, "uri")?.unwrap_or_default();
                        graphic.kind = if uri.ends_with("/picture") {
                            GraphicKind::Picture
                        } else if uri.ends_with("/diagram") {
                            GraphicKind::Diagram
                        } else if uri.ends_with("/chart") {
                            GraphicKind::Chart
                        } else {
                            GraphicKind::Shape
                        };
                    }
                }
                "prstGeom" => {
                    if let Some(graphic) = current.as_mut() {
                        graphic.geometry =
                            attribute_value(&event, "prst")?.unwrap_or_else(|| "rect".into());
                    }
                }
                "srgbClr" => {
                    if let Some(graphic) = current.as_mut()
                        && let Some(color) = attribute_value(&event, "val")?
                        && is_hex_color(&color)
                    {
                        graphic.colors.push(color);
                    }
                }
                "relIds" => {
                    if let Some(graphic) = current.as_mut() {
                        graphic.relationship = attribute_value(&event, "dm")?;
                    }
                }
                "chart" => {
                    if let Some(graphic) = current.as_mut() {
                        graphic.relationship = attribute_value(&event, "id")?;
                    }
                }
                "t" => in_text = true,
                _ => {}
            },
            Event::Text(event) if in_text => {
                if let Some(graphic) = current.as_mut() {
                    let value =
                        quick_xml::escape::unescape(event.as_ref()).map_err(invalid_data)?;
                    let value = value.trim();
                    if !value.is_empty() {
                        graphic.labels.push(value.to_string());
                    }
                }
            }
            Event::End(event) => match event.local_name().as_ref() {
                "t" => in_text = false,
                "drawing" => {
                    if let Some(mut graphic) = current.take() {
                        if matches!(graphic.kind, GraphicKind::Picture) {
                            graphics.push(None);
                            continue;
                        }
                        if matches!(graphic.kind, GraphicKind::Diagram | GraphicKind::Chart) {
                            let relationship = graphic.relationship.as_ref().ok_or_else(|| {
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    format!("{} has no data relationship", graphic.name),
                                )
                            })?;
                            let target = relationships.get(relationship).ok_or_else(|| {
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    format!("missing drawing relationship {relationship}"),
                                )
                            })?;
                            let xml = read_media_part(archive, target).ok_or_else(|| {
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    format!("missing drawing data part {target}"),
                                )
                            })?;
                            let is_chart = matches!(graphic.kind, GraphicKind::Chart);
                            append_relationship_labels(&mut graphic, &xml, is_chart)?;
                        }
                        graphics.push(Some(render_svg(&graphic)));
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(graphics)
}

impl Default for Graphic {
    fn default() -> Self {
        Self {
            kind: GraphicKind::Shape,
            name: "Graphic".into(),
            description: None,
            geometry: "rect".into(),
            width: 0.0,
            height: 0.0,
            colors: Vec::new(),
            relationship: None,
            labels: Vec::new(),
            chart_series: Vec::new(),
        }
    }
}

fn append_relationship_labels(
    graphic: &mut Graphic,
    xml: &[u8],
    chart: bool,
) -> Result<(), MarkoffError> {
    let xml = std::str::from_utf8(xml).map_err(invalid_data)?;
    if chart {
        let series = read_chart_series(xml)?;
        if !series.is_empty() {
            graphic.chart_series = series;
            return Ok(());
        }
    }

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut capture = false;
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(event) if matches!(event.local_name().as_ref(), "t" | "v") => {
                capture = true;
            }
            Event::Text(event) if capture => {
                let value = quick_xml::escape::unescape(event.as_ref()).map_err(invalid_data)?;
                if !value.trim().is_empty() {
                    graphic.labels.push(value.trim().to_string());
                }
            }
            Event::End(event) if matches!(event.local_name().as_ref(), "t" | "v") => {
                capture = false;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn read_chart_series(xml: &str) -> Result<Vec<ChartSeries>, MarkoffError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut series = Vec::new();
    let mut current: Option<ChartSeries> = None;
    let mut section = None;
    let mut value_element = false;
    loop {
        match reader.read_event().map_err(invalid_data)? {
            Event::Start(event) => match event.local_name().as_ref() {
                "ser" => {
                    current = Some(ChartSeries {
                        name: String::new(),
                        categories: Vec::new(),
                        values: Vec::new(),
                    });
                }
                "tx" => section = Some(ChartSection::Name),
                "cat" => section = Some(ChartSection::Category),
                "val" => section = Some(ChartSection::Value),
                "v" => value_element = true,
                _ => {}
            },
            Event::Text(event) if value_element => {
                let value = quick_xml::escape::unescape(event.as_ref()).map_err(invalid_data)?;
                if let Some(current) = current.as_mut() {
                    match section {
                        Some(ChartSection::Name) if current.name.is_empty() => {
                            current.name = value.into_owned();
                        }
                        Some(ChartSection::Category) => {
                            current.categories.push(value.into_owned());
                        }
                        Some(ChartSection::Value) => {
                            if let Ok(value) = value.parse() {
                                current.values.push(value);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::End(event) => match event.local_name().as_ref() {
                "v" => value_element = false,
                "tx" | "cat" | "val" => section = None,
                "ser" => {
                    if let Some(current) = current.take() {
                        series.push(current);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(series)
}

fn render_svg(graphic: &Graphic) -> String {
    match graphic.kind {
        GraphicKind::Diagram => render_diagram(graphic),
        GraphicKind::Chart => render_chart(graphic),
        GraphicKind::Shape | GraphicKind::Picture => render_shape(graphic),
    }
}

fn render_shape(graphic: &Graphic) -> String {
    let width = dimension(graphic.width, 320.0);
    let height = dimension(graphic.height, 100.0).max(52.0);
    let fill = graphic
        .colors
        .first()
        .map_or("#f3f6fa".to_string(), |color| format!("#{color}"));
    let stroke = graphic
        .colors
        .get(1)
        .map_or("#334155".to_string(), |color| format!("#{color}"));
    let title = xml_escape(
        &graphic
            .description
            .clone()
            .unwrap_or_else(|| graphic.name.clone()),
    );
    let labels = if graphic.labels.is_empty() {
        vec![
            graphic
                .description
                .as_deref()
                .unwrap_or(&graphic.name)
                .to_string(),
        ]
    } else {
        graphic.labels.clone()
    };
    let center = labels.join(" ");
    let shape = if graphic.geometry.contains("ellipse") || graphic.geometry.contains("round") {
        format!(
            "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\"/>",
            width / 2.0,
            height / 2.0,
            (width / 2.0 - 2.0).max(1.0),
            (height / 2.0 - 2.0).max(1.0)
        )
    } else if graphic.geometry.contains("triangle") {
        format!(
            "<polygon points=\"{},2 {},{} 2,{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\"/>",
            width / 2.0,
            width - 2.0,
            height - 2.0,
            height - 2.0
        )
    } else {
        format!(
            "<rect x=\"1\" y=\"1\" width=\"{}\" height=\"{}\" rx=\"2\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\"/>",
            width - 2.0,
            height - 2.0
        )
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" role=\"img\" aria-label=\"{title}\"><title>{title}</title>{shape}<text x=\"50%\" y=\"50%\" dominant-baseline=\"middle\" text-anchor=\"middle\" font-family=\"Arial,sans-serif\" font-size=\"14\" fill=\"#111827\">{}</text></svg>",
        xml_escape(&center)
    )
}

fn render_diagram(graphic: &Graphic) -> String {
    let labels = nonempty_labels(&graphic.labels);
    let labels = if labels.is_empty() {
        vec![graphic.name.clone()]
    } else {
        labels
    };
    let width = (labels.len() as f64 * 152.0 + 24.0).max(360.0);
    let height = 110.0;
    let mut items = String::new();
    for (index, label) in labels.iter().enumerate() {
        let x = 12.0 + index as f64 * 152.0;
        if index > 0 {
            items.push_str(&format!(
                "<path d=\"M{} 55h12m-5-5 5 5-5 5\" fill=\"none\" stroke=\"#64748b\" stroke-width=\"2\"/>",
                x - 14.0
            ));
        }
        items.push_str(&format!(
            "<rect x=\"{x}\" y=\"24\" width=\"128\" height=\"62\" rx=\"8\" fill=\"#e8f1f8\" stroke=\"#156082\" stroke-width=\"2\"/><text x=\"{}\" y=\"55\" dominant-baseline=\"middle\" text-anchor=\"middle\" font-family=\"Arial,sans-serif\" font-size=\"14\" fill=\"#102a43\">{}</text>",
            x + 64.0,
            xml_escape(label)
        ));
    }
    svg_document(graphic, width, height, items)
}

fn render_chart(graphic: &Graphic) -> String {
    if graphic.chart_series.is_empty() {
        return render_diagram(graphic);
    }
    let categories = graphic
        .chart_series
        .iter()
        .find(|series| !series.categories.is_empty())
        .map(|series| series.categories.clone())
        .unwrap_or_else(|| {
            let count = graphic
                .chart_series
                .iter()
                .map(|series| series.values.len())
                .max()
                .unwrap_or_default();
            (1..=count).map(|index| index.to_string()).collect()
        });
    let max = graphic
        .chart_series
        .iter()
        .flat_map(|series| series.values.iter().copied())
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let width = 640.0;
    let height = 320.0;
    let baseline = 258.0;
    let plot_top = 44.0;
    let plot_height = baseline - plot_top;
    let plot_width = 550.0;
    let group_width = plot_width / categories.len().max(1) as f64;
    let bar_width = (group_width * 0.72 / graphic.chart_series.len().max(1) as f64).min(24.0);
    let colors = ["#156082", "#e97132", "#196b24", "#0f9ed5", "#a02b93"];
    let mut elements = format!(
        "<path d=\"M48 {plot_top}V{baseline}H620\" stroke=\"#475569\" fill=\"none\"/><text x=\"12\" y=\"20\" font-family=\"Arial,sans-serif\" font-size=\"13\" fill=\"#334155\">{}</text>",
        xml_escape(graphic.description.as_deref().unwrap_or(&graphic.name))
    );
    for (series_index, series) in graphic.chart_series.iter().enumerate() {
        elements.push_str(&format!(
            "<rect x=\"{}\" y=\"30\" width=\"10\" height=\"10\" fill=\"{}\"/><text x=\"{}\" y=\"39\" font-family=\"Arial,sans-serif\" font-size=\"10\" fill=\"#334155\">{}</text>",
            50.0 + series_index as f64 * 130.0,
            colors[series_index % colors.len()],
            64.0 + series_index as f64 * 130.0,
            xml_escape(&series.name)
        ));
    }
    for (category_index, category) in categories.iter().enumerate() {
        let category_start = 55.0 + category_index as f64 * group_width;
        for (series_index, series) in graphic.chart_series.iter().enumerate() {
            let Some(value) = series.values.get(category_index) else {
                continue;
            };
            let bar_height = value / max * plot_height;
            let x = category_start
                + (group_width - bar_width * graphic.chart_series.len() as f64) / 2.0
                + series_index as f64 * bar_width;
            let y = baseline - bar_height;
            elements.push_str(&format!(
                "<rect x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{bar_height}\" fill=\"{}\"><title>{}: {}</title></rect>",
                (bar_width - 2.0).max(1.0),
                colors[series_index % colors.len()],
                xml_escape(&series.name),
                xml_escape(&format_number(*value))
            ));
        }
        elements.push_str(&format!(
            "<text x=\"{}\" y=\"280\" text-anchor=\"middle\" font-family=\"Arial,sans-serif\" font-size=\"10\" fill=\"#334155\">{}</text>",
            category_start + group_width / 2.0,
            xml_escape(category)
        ));
    }
    svg_document(graphic, width, height, elements)
}

fn svg_document(graphic: &Graphic, width: f64, height: f64, content: String) -> String {
    let title = xml_escape(
        &graphic
            .description
            .clone()
            .unwrap_or_else(|| graphic.name.clone()),
    );
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" role=\"img\" aria-label=\"{title}\"><title>{title}</title>{content}</svg>"
    )
}

fn nonempty_labels(labels: &[String]) -> Vec<String> {
    labels
        .iter()
        .map(|label| label.trim())
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .collect()
}

fn dimension(emu: f64, fallback: f64) -> f64 {
    if emu > 0.0 {
        (emu / 9525.0).clamp(64.0, 960.0)
    } else {
        fallback
    }
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}
