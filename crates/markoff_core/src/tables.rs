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

    let header = parse_markdown_table_row(rows[0]);

    let body = rows[1..]
        .iter()
        .skip_while(|row| row.contains("---"))
        .map(|row| parse_markdown_table_row(row))
        .filter(|row| !row.is_empty() && row.len() == header.len())
        .collect::<Vec<_>>();

    let mut result = Vec::new();
    if !header.is_empty() {
        result.push(header);
    }
    result.extend(body);
    result
}

fn parse_markdown_table_row(row: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut characters = row.trim().trim_matches('|').chars().peekable();

    while let Some(character) = characters.next() {
        if character == '\\' && characters.peek() == Some(&'|') {
            cell.push('|');
            characters.next();
        } else if character == '|' {
            cells.push(cell.trim().to_string());
            cell.clear();
        } else {
            cell.push(character);
        }
    }
    cells.push(cell.trim().to_string());
    cells
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

    let format_row = |row: &[String]| {
        format!(
            "| {} |",
            row.iter()
                .map(|cell| cell.replace('\\', "\\\\").replace('|', "\\|"))
                .collect::<Vec<_>>()
                .join(" | ")
        )
    };
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

#[cfg(test)]
mod tests {
    use super::{markdown_table_from_rows, parse_markdown_table};

    #[test]
    fn parses_and_writes_escaped_pipes() {
        let rows = vec![
            vec!["Quarter | Sales".to_string(), "Profit".to_string()],
            vec!["Q1".to_string(), "40".to_string()],
        ];
        let markdown = markdown_table_from_rows(&rows);

        assert!(markdown.contains("Quarter \\| Sales"));
        assert_eq!(parse_markdown_table(&markdown), rows);
    }
}
