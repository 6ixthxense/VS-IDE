//! CSV / XLSX export of a query or table result (port of `createCsvContent`,
//! `exportRowsAsCsv`, `exportRowsAsXlsx` and `buildExportFileName` from `DatabasePanel.tsx`).

use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rust_xlsxwriter::Workbook;

use crate::error::Result;
use crate::types::{SqlQueryResult, SqlValue};

/// Name of the worksheet written by [`export_xlsx`].
pub const XLSX_SHEET_NAME: &str = "Export";
/// Excel's hard limit for the text length of one cell.
const XLSX_MAX_CELL_CHARS: usize = 32_767;

/// One exported column: `key` selects the source column, `label` is the header text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportColumn {
    pub key: String,
    pub label: String,
}

impl ExportColumn {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self { key: key.into(), label: label.into() }
    }

    /// Column whose header equals its key.
    pub fn same(key: impl Into<String>) -> Self {
        let key = key.into();
        Self { label: key.clone(), key }
    }

    /// All columns of a result, in order.
    pub fn all(source_columns: &[String]) -> Vec<Self> {
        source_columns.iter().map(Self::same).collect()
    }
}

/// Resolve each export column to an index into `source_columns` (`None` -> empty cells,
/// like `row[column.key]` being `undefined`).
fn resolve(source_columns: &[String], columns: &[ExportColumn]) -> Vec<Option<usize>> {
    columns
        .iter()
        .map(|c| source_columns.iter().position(|s| s == &c.key))
        .collect()
}

fn cell(row: &[SqlValue], index: Option<usize>) -> String {
    index
        .and_then(|i| row.get(i))
        .map(SqlValue::to_export_string)
        .unwrap_or_default()
}

fn escape_csv(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

/// Options for [`export_csv_with`] / [`csv_string_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvOptions {
    /// Prefix `'` to text fields (and headers) starting with `=`, `+`, `-`, `@`, tab or CR so
    /// spreadsheet apps do not evaluate them as formulas.  On by default.
    pub neutralize_formulas: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self { neutralize_formulas: true }
    }
}

fn neutralize(value: &str) -> String {
    if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_string()
    }
}

/// CSV text with default options (formula neutralisation on).
pub fn csv_string(
    source_columns: &[String],
    columns: &[ExportColumn],
    rows: &[Vec<SqlValue>],
) -> String {
    csv_string_with(source_columns, columns, rows, CsvOptions::default())
}

/// CSV text: every field quoted, `\r\n` between lines, no trailing newline.
pub fn csv_string_with(
    source_columns: &[String],
    columns: &[ExportColumn],
    rows: &[Vec<SqlValue>],
    options: CsvOptions,
) -> String {
    let guard = |text: String, is_text: bool| {
        if options.neutralize_formulas && is_text { neutralize(&text) } else { text }
    };
    let indexes = resolve(source_columns, columns);
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(
        columns
            .iter()
            .map(|c| escape_csv(&guard(c.label.clone(), true)))
            .collect::<Vec<_>>()
            .join(","),
    );
    for row in rows {
        lines.push(
            indexes
                .iter()
                .map(|i| {
                    let is_text = matches!(i.and_then(|i| row.get(i)), Some(SqlValue::Text(_)));
                    escape_csv(&guard(cell(row, *i), is_text))
                })
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    lines.join("\r\n")
}

/// Write CSV to `path` (UTF-8, no BOM) with default options.
pub fn export_csv(
    path: &Path,
    source_columns: &[String],
    columns: &[ExportColumn],
    rows: &[Vec<SqlValue>],
) -> Result<()> {
    export_csv_with(path, source_columns, columns, rows, CsvOptions::default())
}

/// Write CSV to `path` (UTF-8, no BOM).
pub fn export_csv_with(
    path: &Path,
    source_columns: &[String],
    columns: &[ExportColumn],
    rows: &[Vec<SqlValue>],
    options: CsvOptions,
) -> Result<()> {
    let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
    file.write_all(csv_string_with(source_columns, columns, rows, options).as_bytes())?;
    file.flush()?;
    Ok(())
}

/// Write an XLSX workbook with a single `Export` sheet.  Like the original (SheetJS
/// `json_to_sheet` over stringified values) every cell is written as text.
pub fn export_xlsx(
    path: &Path,
    source_columns: &[String],
    columns: &[ExportColumn],
    rows: &[Vec<SqlValue>],
) -> Result<()> {
    let indexes = resolve(source_columns, columns);
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name(XLSX_SHEET_NAME)?;
    let clip = |s: String| -> String {
        if s.chars().count() > XLSX_MAX_CELL_CHARS {
            s.chars().take(XLSX_MAX_CELL_CHARS).collect()
        } else {
            s
        }
    };
    for (col, column) in columns.iter().enumerate() {
        sheet.write_string(0, u16::try_from(col).unwrap_or(u16::MAX), clip(column.label.clone()))?;
    }
    for (r, row) in rows.iter().enumerate() {
        let excel_row = u32::try_from(r + 1).unwrap_or(u32::MAX);
        for (col, index) in indexes.iter().enumerate() {
            let text = cell(row, *index);
            if text.is_empty() {
                continue;
            }
            sheet.write_string(excel_row, u16::try_from(col).unwrap_or(u16::MAX), clip(text))?;
        }
    }
    workbook.save(path)?;
    Ok(())
}

/// Convenience: export every column of a query result as CSV.
pub fn export_result_csv(path: &Path, result: &SqlQueryResult) -> Result<()> {
    export_csv(path, &result.columns, &ExportColumn::all(&result.columns), &result.rows)
}

/// Convenience: export every column of a query result as XLSX.
pub fn export_result_xlsx(path: &Path, result: &SqlQueryResult) -> Result<()> {
    export_xlsx(path, &result.columns, &ExportColumn::all(&result.columns), &result.rows)
}

/// Port of `buildExportFileName`: `<safe-base>-<UTC timestamp>.<ext>`.
pub fn build_export_file_name(base_name: &str, extension: &str) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    build_export_file_name_at(base_name, extension, secs)
}

fn build_export_file_name_at(base_name: &str, extension: &str, unix_secs: u64) -> String {
    let days = i64::try_from(unix_secs / 86_400).unwrap_or(0);
    let rem = unix_secs % 86_400;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let timestamp = format!(
        "{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    );

    let mut safe = String::new();
    let mut pending_dash = false;
    for ch in base_name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            if pending_dash && !safe.is_empty() {
                safe.push('-');
            }
            pending_dash = false;
            safe.push(ch);
        } else {
            pending_dash = true;
        }
    }
    let safe = safe.trim_matches('-');
    let safe = if safe.is_empty() { "database-export" } else { safe };
    format!("{safe}-{timestamp}.{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_everything_and_uses_crlf() {
        let source = vec!["id".to_string(), "name".to_string()];
        let columns = vec![ExportColumn::new("name", "Name"), ExportColumn::new("missing", "Gone")];
        let rows = vec![
            vec![SqlValue::Integer(1), SqlValue::Text("a \"q\", b".into())],
            vec![SqlValue::Integer(2), SqlValue::Null],
        ];
        assert_eq!(
            csv_string(&source, &columns, &rows),
            "\"Name\",\"Gone\"\r\n\"a \"\"q\"\", b\",\"\"\r\n\"\",\"\""
        );
    }

    #[test]
    fn csv_formula_injection_is_neutralised_by_default() {
        let source = vec!["a".to_string()];
        let columns = vec![ExportColumn::same("a")];
        let rows: Vec<Vec<SqlValue>> = ["=1+1", "+1", "-1", "@SUM(A1)", "\tx", "\rx", "safe", "a=b"]
            .iter()
            .map(|v| vec![SqlValue::Text((*v).into())])
            .chain([vec![SqlValue::Integer(-5)], vec![SqlValue::Real(-1.5)]])
            .collect();
        let csv = csv_string(&source, &columns, &rows);
        let lines: Vec<&str> = csv.split("\r\n").collect();
        assert_eq!(lines[1], "\"'=1+1\"");
        assert_eq!(lines[2], "\"'+1\"");
        assert_eq!(lines[3], "\"'-1\"");
        assert_eq!(lines[4], "\"'@SUM(A1)\"");
        assert!(csv.contains("\"'\tx\""));
        assert!(csv.contains("\"'\rx\""));
        assert!(csv.contains("\"safe\""));
        assert!(csv.contains("\"a=b\""));
        assert!(csv.ends_with("\"-5\"\r\n\"-1.5\"")); // real numbers are untouched

        let header = csv_string(&source, &[ExportColumn::new("a", "=HYPERLINK(1)")], &[]);
        assert_eq!(header, "\"'=HYPERLINK(1)\"");

        let raw = csv_string_with(&source, &columns, &rows[..1], CsvOptions { neutralize_formulas: false });
        assert!(raw.ends_with("\"=1+1\""));
    }

    #[test]
    fn file_name_is_sanitised_and_timestamped() {
        // 2024-02-29T13:05:09Z
        assert_eq!(
            build_export_file_name_at(" My Table!! / x ", "csv", 1_709_211_909),
            "My-Table-x-2024-02-29T13-05-09.csv"
        );
        assert_eq!(
            build_export_file_name_at("!!!", "xlsx", 0),
            "database-export-1970-01-01T00-00-00.xlsx"
        );
    }
}
