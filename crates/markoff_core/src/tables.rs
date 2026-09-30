use std::collections::BTreeMap;
use std::io::{Error, ErrorKind};

pub(crate) fn parse_markdown_table(markdown: &str) -> Result<Vec<Vec<String>>, Error> {
    let rows = markdown
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| line.contains('|'))
        .collect::<Vec<_>>();

    if rows.len() < 2 {
        return Ok(Vec::new());
    }

    let header = parse_markdown_table_row(rows[0]);
    let separator = parse_markdown_table_row(rows[1]);
    if header.is_empty()
        || separator.len() != header.len()
        || !separator.iter().all(|cell| {
            let cell = cell.trim().trim_matches(':');
            cell.len() >= 3 && cell.chars().all(|character| character == '-')
        })
    {
        return Ok(Vec::new());
    }

    let mut result = vec![header];
    for (index, row) in rows.iter().enumerate().skip(2) {
        let row = parse_markdown_table_row(row);
        if row.len() != result[0].len() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!(
                    "Markdown table row {} has {} cells; expected {}",
                    index + 1,
                    row.len(),
                    result[0].len()
                ),
            ));
        }
        result.push(row);
    }
    Ok(result)
}

fn parse_markdown_table_row(row: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut characters = row.trim().trim_matches('|').chars().peekable();

    while let Some(character) = characters.next() {
        if character == '\\' && characters.peek() == Some(&'|') {
            cell.push('|');
            characters.next();
        } else if character == '\\' && characters.peek() == Some(&'\\') {
            cell.push('\\');
            characters.next();
        } else if character == '|' {
            cells.push(cell.trim().replace("<br>", "\n"));
            cell.clear();
        } else {
            cell.push(character);
        }
    }
    cells.push(cell.trim().replace("<br>", "\n"));
    cells
}

pub(crate) fn parse_markdown_tables(
    markdown: &str,
) -> Result<BTreeMap<String, Vec<Vec<String>>>, Error> {
    let mut tables = BTreeMap::new();
    let mut sheet_name = "Sheet1".to_string();
    let mut table_lines = Vec::new();

    for line in markdown.lines() {
        if let Some(name) = line.trim().strip_prefix("## ") {
            if !table_lines.is_empty() {
                let rows = parse_markdown_table(&table_lines.join("\n"))?;
                if !rows.is_empty() {
                    tables.insert(sheet_name, rows);
                }
                table_lines.clear();
            }
            sheet_name = name.trim().to_string();
        } else if line.trim().contains('|') {
            table_lines.push(line.to_string());
        }
    }

    if !table_lines.is_empty() {
        let rows = parse_markdown_table(&table_lines.join("\n"))?;
        if !rows.is_empty() {
            tables.insert(sheet_name, rows);
        }
    }
    Ok(tables)
}

pub(crate) fn markdown_table_from_rows(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let format_row = |row: &[String]| {
        format!(
            "| {} |",
            row.iter()
                .map(|cell| cell
                    .replace('\\', "\\\\")
                    .replace('|', "\\|")
                    .replace('\n', "<br>"))
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
        assert_eq!(parse_markdown_table(&markdown).unwrap(), rows);
    }

    #[test]
    fn preserves_header_only_table() {
        let markdown = "| Name | Score |\n| --- | --- |";
        assert_eq!(
            parse_markdown_table(markdown).unwrap(),
            vec![vec!["Name".to_string(), "Score".to_string()]]
        );
    }

    #[test]
    fn rejects_body_rows_with_the_wrong_width() {
        let error =
            parse_markdown_table("| A | B |\n| --- | --- |\n| one | two | three |").unwrap_err();
        assert!(error.to_string().contains("row 3 has 3 cells; expected 2"));
    }
}
