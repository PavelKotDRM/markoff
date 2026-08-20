use std::collections::BTreeMap;

pub(crate) fn parse_markdown_table(markdown: &str) -> Vec<Vec<String>> {
    let rows = markdown
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| line.starts_with('|') && line.ends_with('|'))
        .collect::<Vec<_>>();

    if rows.len() < 3 {
        return Vec::new();
    }

    let header = rows[0]
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect::<Vec<_>>();

    let body = rows[1..]
        .iter()
        .skip_while(|row| row.contains("---"))
        .map(|row| {
            row.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|row| !row.is_empty() && row.len() == header.len())
        .collect::<Vec<_>>();

    let mut result = Vec::new();
    if !header.is_empty() {
        result.push(header);
    }
    result.extend(body);
    result
}

pub(crate) fn parse_markdown_tables(markdown: &str) -> BTreeMap<String, Vec<Vec<String>>> {
    let mut tables = BTreeMap::new();
    let mut sheet_name = "Sheet1".to_string();
    let mut table_lines = Vec::new();

    for line in markdown.lines() {
        if let Some(name) = line.trim().strip_prefix("## ") {
            if !table_lines.is_empty() {
                let rows = parse_markdown_table(&table_lines.join("\n"));
                if !rows.is_empty() {
                    tables.insert(sheet_name, rows);
                }
                table_lines.clear();
            }
            sheet_name = name.trim().to_string();
        } else if line.trim().starts_with('|') && line.trim().ends_with('|') {
            table_lines.push(line.to_string());
        }
    }

    if !table_lines.is_empty() {
        let rows = parse_markdown_table(&table_lines.join("\n"));
        if !rows.is_empty() {
            tables.insert(sheet_name, rows);
        }
    }
    tables
}

pub(crate) fn markdown_table_from_rows(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let format_row = |row: &[String]| format!("| {} |", row.join(" | "));
    let separator = (0..rows[0].len())
        .map(|_| "---")
        .collect::<Vec<_>>()
        .join(" | ");

    let mut output = Vec::new();
    output.push(format_row(&rows[0]));
    output.push(format!("| {} |", separator));

    for row in rows.iter().skip(1) {
        output.push(format_row(row));
    }

    output.join("\n")
}
