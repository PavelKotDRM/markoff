use crate::docx_resources::ListKind;
use std::collections::BTreeMap;

pub(super) fn textual_list_item(paragraph: &str) -> Option<(String, String)> {
    let (marker, value) = ["**", "__", "~~", "*", "_"]
        .into_iter()
        .find_map(|marker| {
            paragraph
                .strip_prefix(marker)
                .and_then(|value| value.strip_suffix(marker))
                .map(|value| (marker, value))
        })
        .unwrap_or(("", paragraph));
    let separator = value.find(char::is_whitespace)?;
    let label = &value[..separator];
    let (label, parenthesized) = match (label.strip_suffix('.'), label.strip_suffix(')')) {
        (Some(label), _) => (label, false),
        (_, Some(label)) => (label, true),
        _ => return None,
    };
    let levels = label
        .split('.')
        .map(str::parse::<usize>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let (&number, parents) = levels.split_last()?;
    (!marker.is_empty() || parenthesized || !parents.is_empty()).then(|| {
        (
            format!("{}{}. ", "    ".repeat(parents.len()), number),
            format!("{marker}{}{marker}", value[separator..].trim_start()),
        )
    })
}

pub(super) fn list_prefix(
    numbering_id: Option<&str>,
    level: usize,
    numbering: &BTreeMap<(String, usize), ListKind>,
    counters: &mut BTreeMap<(String, usize), usize>,
) -> String {
    let Some(numbering_id) = numbering_id else {
        return String::new();
    };
    let kind = numbering
        .get(&(numbering_id.to_string(), level))
        .copied()
        .unwrap_or(if numbering_id == "1" {
            ListKind::Bullet
        } else {
            ListKind::Decimal { start: 1 }
        });
    let indentation = "    ".repeat(level);

    match kind {
        ListKind::Bullet => format!("{indentation}- "),
        ListKind::Decimal { start } => {
            let key = (numbering_id.to_string(), level);
            let number = *counters.entry(key).or_insert(start);
            counters.insert((numbering_id.to_string(), level), number + 1);
            counters.retain(|(id, child_level), _| id != numbering_id || *child_level <= level);
            format!("{indentation}{number}. ")
        }
    }
}

pub(super) fn table_from_rows(rows: &[Vec<String>]) -> String {
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if column_count == 0 {
        return String::new();
    }

    let format_row = |row: &[String]| {
        let cells = (0..column_count)
            .map(|index| {
                row.get(index)
                    .map_or("", String::as_str)
                    .replace("  \n", "<br>")
                    .replace('\n', "<br>")
                    .replace('|', "\\|")
            })
            .collect::<Vec<_>>();
        format!("| {} |", cells.join(" | "))
    };

    let mut markdown = vec![format_row(&rows[0])];
    markdown.push(format!("| {} |", vec!["---"; column_count].join(" | ")));
    markdown.extend(rows.iter().skip(1).map(|row| format_row(row)));
    markdown.join("\n")
}
