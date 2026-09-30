use crate::MarkoffError;
use crate::document::expand_inline_footnotes;
use crate::style::{DocumentTheme, PageTextSegment, page_field_segments};
use crate::xml_utils::xml_escape;
use base64::Engine as _;
use pulldown_cmark::{CowStr, Event, Tag};
use std::path::Path;

pub(crate) fn convert_markdown_to_html(
    input: &Path,
    output: &Path,
    theme: &DocumentTheme,
) -> Result<(), MarkoffError> {
    use pulldown_cmark::{Options, Parser, html};

    let source = std::fs::read_to_string(input)?;
    let markdown = expand_inline_footnotes(&source);
    let title = source
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .unwrap_or("Document");

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_MATH);
    options.insert(Options::ENABLE_SUPERSCRIPT);
    options.insert(Options::ENABLE_SUBSCRIPT);
    let base_dir = input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let events = Parser::new_ext(&markdown, options)
        .map(|event| match event {
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => embed_local_image(dest_url.as_ref(), base_dir).map(|embedded| {
                Event::Start(Tag::Image {
                    link_type,
                    dest_url: embedded
                        .map(|destination| CowStr::Boxed(destination.into_boxed_str()))
                        .unwrap_or(dest_url),
                    title,
                    id,
                })
            }),
            event => Ok(event),
        })
        .collect::<Result<Vec<_>, MarkoffError>>()?;
    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    let style = if theme.enabled {
        format!("<style>{}</style>\n", theme_css(theme))
    } else {
        String::new()
    };
    let document = format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n{style}</head>\n<body>\n{body}</body>\n</html>\n",
        xml_escape(title)
    );
    std::fs::write(output, document)?;
    Ok(())
}

fn theme_css(theme: &DocumentTheme) -> String {
    let heading_rules = (1..=6u8)
        .map(|level| {
            format!(
                "h{level}{{font-family:{font};font-size:{size}pt;color:{color};font-weight:{weight};font-style:{slant};margin:{before}pt 0 {after}pt;}}",
                font = css_font_family(&theme.heading_font_family),
                size = theme.heading_size_for(level),
                color = theme.heading_color_for(level).css(),
                weight = if theme.heading_bold { "bold" } else { "normal" },
                slant = if theme.heading_italic { "italic" } else { "normal" },
                before = theme.heading_spacing_before_pt,
                after = theme.heading_spacing_after_pt,
            )
        })
        .collect::<String>();
    let (page_width, page_height) = theme.page_dimensions_pt();
    let page_box = |name: &str, text: &str| {
        if text.is_empty() {
            String::new()
        } else {
            format!(
                "@{name}{{content:{};font-family:{};font-size:{}pt;color:{};}}",
                css_page_content(text),
                css_font_family(&theme.font_family),
                theme.header_footer_font_size_pt,
                theme.text_color.css(),
            )
        }
    };
    let quote_background = theme
        .quote_background
        .map_or_else(|| "transparent".to_string(), |color| color.css());
    let inline_code_background = theme
        .code_inline_background
        .map_or_else(|| "transparent".to_string(), |color| color.css());
    let stripe = theme
        .table_stripe_background
        .map(|color| format!("tbody tr:nth-child(even) td{{background:{};}}", color.css()))
        .unwrap_or_default();
    format!(
        "@page{{size:{page_width}pt {page_height}pt;margin:{top}pt {right}pt {bottom}pt {left}pt;{header}{footer}}}\
body{{font-family:{body_font};font-size:{body_size}pt;color:{text_color};line-height:{line_height};margin:{top}pt {right}pt {bottom}pt {left}pt;}}\
p{{margin:{paragraph_before}pt 0 {paragraph_after}pt;text-align:{align};text-indent:{first_line}pt;}}\
li p,td p,th p,blockquote p,.footnote-definition p{{text-indent:0;}}\
{heading_rules}\
a{{color:{link_color};text-decoration:{link_decoration};}}\
blockquote{{margin:0 0 {paragraph_after}pt {quote_margin}pt;padding:2pt 0 2pt {quote_padding}pt;border-left:{quote_border_width}pt solid {quote_border};color:{quote_color};background:{quote_background};font-style:{quote_slant};}}\
ul,ol{{padding-left:{list_indent}pt;}}ul{{list-style-type:{bullet};}}\
pre,code{{font-family:{code_font};font-size:{code_size}pt;color:{code_color};}}\
code{{background:{inline_code_background};padding:0 2pt;}}\
pre{{background:{code_background};padding:{code_padding}pt;white-space:pre-wrap;}}pre code{{background:transparent;padding:0;}}\
table{{border-collapse:collapse;font-size:{table_size}pt;}}\
th,td{{border:{border_width}pt solid {border};padding:{cell_padding}pt;}}\
th{{background:{header_background};color:{header_color};}}{stripe}\
hr{{border:none;border-top:{rule_width}pt solid {rule_color};}}\
.footnote-definition{{font-size:{footnote_size}pt;}}\
img{{max-width:{image_width}%;height:auto;}}",
        top = theme.margin_top_pt,
        right = theme.margin_right_pt,
        bottom = theme.margin_bottom_pt,
        left = theme.margin_left_pt,
        header = page_box("top-center", &theme.header_text),
        footer = page_box("bottom-center", &theme.footer_text),
        body_font = css_font_family(&theme.font_family),
        body_size = theme.font_size_pt,
        text_color = theme.text_color.css(),
        line_height = theme.line_height,
        paragraph_before = theme.paragraph_spacing_before_pt,
        paragraph_after = theme.paragraph_spacing_after_pt,
        align = theme.text_align.name(),
        first_line = theme.first_line_indent_pt,
        link_color = theme.link_color.css(),
        link_decoration = if theme.link_underline {
            "underline"
        } else {
            "none"
        },
        quote_margin = theme.quote_indent_pt / 2.0,
        quote_padding = theme.quote_indent_pt / 2.0,
        quote_border_width = theme.quote_border_width_pt,
        quote_border = theme.quote_border_color.css(),
        quote_color = theme.quote_text_color.css(),
        quote_slant = if theme.quote_italic {
            "italic"
        } else {
            "normal"
        },
        list_indent = theme.list_indent_pt,
        bullet = css_string(&format!("{} ", theme.list_bullet)),
        code_font = css_font_family(&theme.code_font_family),
        code_size = theme.code_font_size_pt,
        code_color = theme.code_text_color.css(),
        code_background = theme.code_background.css(),
        code_padding = theme.code_padding_pt,
        table_size = theme.table_font_size_pt,
        border_width = theme.table_border_width_pt,
        border = theme.table_border_color.css(),
        cell_padding = theme.table_cell_padding_pt,
        header_background = theme.table_header_background.css(),
        header_color = theme.table_header_color.css(),
        rule_width = theme.rule_width_pt,
        rule_color = theme.rule_color.css(),
        footnote_size = theme.footnote_font_size_pt,
        image_width = theme.image_max_width_percent,
    )
}

fn css_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' | '\\' => {
                escaped.push('\\');
                escaped.push(character);
            }
            '<' => escaped.push_str("\\3C "),
            '>' => escaped.push_str("\\3E "),
            '&' => escaped.push_str("\\26 "),
            _ => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn css_page_content(text: &str) -> String {
    page_field_segments(text)
        .into_iter()
        .map(|segment| match segment {
            PageTextSegment::Text(text) => css_string(text),
            PageTextSegment::Page => "counter(page)".to_string(),
            PageTextSegment::Pages => "counter(pages)".to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn css_font_family(font_family: &str) -> String {
    format!(
        "\"{}\"",
        font_family.replace(['\\', '"', '<', '>', '&'], "")
    )
}

fn embed_local_image(destination: &str, base_dir: &Path) -> Result<Option<String>, MarkoffError> {
    if destination.is_empty()
        || destination.contains("://")
        || destination.starts_with("data:")
        || destination.starts_with('#')
    {
        return Ok(None);
    }
    let path = destination.split(['?', '#']).next().unwrap_or(destination);
    let image_path = base_dir.join(path);
    match std::fs::read(&image_path) {
        Ok(bytes) => {
            let media_type = match image_path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "svg" => "image/svg+xml",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                _ => "application/octet-stream",
            };
            Ok(Some(format!(
                "data:{media_type};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
