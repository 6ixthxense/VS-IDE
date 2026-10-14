//! CSV / XLSX import: parse into an [`ImportedTable`], then bulk insert with
//! [`crate::SqliteAdapter::import_table`].

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use calamine::{open_workbook_auto, Data, Reader};
use rusqlite::Connection;

use crate::error::{DbError, Result};
use crate::sqlite::quote_identifier;
use crate::types::SqlValue;

#[derive(Debug, Clone, PartialEq)]
pub struct ImportOptions {
    /// First row holds column names (otherwise `column_1`, `column_2`, ...).
    pub has_header: bool,
    /// Worksheet name for XLSX (default: first sheet).
    pub sheet: Option<String>,
    /// `CREATE TABLE IF NOT EXISTS` with TEXT columns before inserting.
    pub create_table: bool,
    /// Store empty strings as NULL.
    pub empty_as_null: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self { has_header: true, sheet: None, create_table: true, empty_as_null: true }
    }
}

/// Parsed tabular data.  CSV cells are `Text`; XLSX cells keep their numeric type.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportedTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
}

fn build_table(raw_rows: Vec<Vec<SqlValue>>, has_header: bool) -> ImportedTable {
    let width = raw_rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut iter = raw_rows.into_iter();
    let header: Vec<String> = if has_header {
        iter.next()
            .unwrap_or_default()
            .iter()
            .map(|v| v.to_export_string().trim().to_string())
            .collect()
    } else {
        Vec::new()
    };
    let mut rows: Vec<Vec<SqlValue>> = iter.collect();
    let width = width.max(header.len());

    let mut seen = HashSet::new();
    let columns = (0..width)
        .map(|i| {
            let base = header.get(i).filter(|h| !h.is_empty()).cloned().unwrap_or_else(|| format!("column_{}", i + 1));
            let mut name = base.clone();
            let mut suffix = 2;
            while !seen.insert(name.to_lowercase()) {
                name = format!("{base}_{suffix}");
                suffix += 1;
            }
            name
        })
        .collect();
    for row in &mut rows {
        row.resize(width, SqlValue::Null);
    }
    ImportedTable { columns, rows }
}

/// Parse a CSV file (comma separated, RFC 4180 quoting, optional UTF-8 BOM).
pub fn import_csv(path: &Path, options: &ImportOptions) -> Result<ImportedTable> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)?;
    let mut raw = Vec::new();
    for record in reader.records() {
        let record = record?;
        raw.push(record.iter().map(|f| SqlValue::Text(f.to_string())).collect::<Vec<_>>());
    }
    if let Some(SqlValue::Text(first)) = raw.first_mut().and_then(|r| r.first_mut()) {
        if let Some(stripped) = first.strip_prefix('\u{feff}') {
            *first = stripped.to_string();
        }
    }
    Ok(build_table(raw, options.has_header))
}

fn data_to_value(data: &Data) -> SqlValue {
    match data {
        Data::Empty => SqlValue::Null,
        Data::String(s) => SqlValue::Text(s.clone()),
        Data::Int(i) => SqlValue::Integer(*i),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 9.0e15 {
                SqlValue::Integer(*f as i64)
            } else {
                SqlValue::Real(*f)
            }
        }
        Data::Bool(b) => SqlValue::Integer(i64::from(*b)),
        Data::DateTime(dt) => match dt.as_datetime() {
            // ISO-8601; pure dates (midnight) are emitted without a time part.
            Some(naive) if !dt.is_duration() => {
                let text = naive.to_string().replacen(' ', "T", 1);
                SqlValue::Text(text.strip_suffix("T00:00:00").map_or(text.clone(), str::to_string))
            }
            _ => SqlValue::Real(dt.as_f64()),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => SqlValue::Text(s.clone()),
        Data::Error(e) => SqlValue::Text(e.to_string()),
    }
}

/// Sheet names of a workbook.
pub fn xlsx_sheet_names(path: &Path) -> Result<Vec<String>> {
    Ok(open_workbook_auto(path)?.sheet_names())
}

/// Parse one worksheet of an XLSX (or XLS/ODS) file.
pub fn import_xlsx(path: &Path, options: &ImportOptions) -> Result<ImportedTable> {
    let mut workbook = open_workbook_auto(path)?;
    let sheet = match &options.sheet {
        Some(name) => name.clone(),
        None => workbook
            .sheet_names()
            .into_iter()
            .next()
            .ok_or_else(|| DbError::invalid("The workbook has no worksheets."))?,
    };
    let range = workbook.worksheet_range(&sheet)?;
    let raw = range
        .rows()
        .map(|row| row.iter().map(data_to_value).collect::<Vec<_>>())
        .collect();
    Ok(build_table(raw, options.has_header))
}

/// Dispatch on the file extension (`csv`, `xlsx`, `xlsm`, `xls`, `ods`).
pub fn import_file(path: &Path, options: &ImportOptions) -> Result<ImportedTable> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "csv" => import_csv(path, options),
        "xlsx" | "xlsm" | "xls" | "ods" => import_xlsx(path, options),
        _ => Err(DbError::UnsupportedFile(path.to_path_buf())),
    }
}

/// Create (optionally) the table and insert all rows inside a single transaction.
pub(crate) fn insert_imported(
    conn: &Mutex<Connection>,
    table_name: &str,
    table: &ImportedTable,
    options: &ImportOptions,
) -> Result<usize> {
    if table_name.trim().is_empty() {
        return Err(DbError::invalid("Enter a table name for the import."));
    }
    if table.columns.is_empty() {
        return Err(DbError::invalid("The import file does not contain any columns."));
    }
    let mut conn = conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let quoted_table = quote_identifier(table_name);
    let tx = conn.transaction()?;
    if options.create_table {
        let defs = table
            .columns
            .iter()
            .map(|c| format!("{} TEXT", quote_identifier(c)))
            .collect::<Vec<_>>()
            .join(", ");
        tx.execute_batch(&format!("CREATE TABLE IF NOT EXISTS {quoted_table} ({defs})"))?;
    }
    let columns = table.columns.iter().map(|c| quote_identifier(c)).collect::<Vec<_>>().join(", ");
    let placeholders = vec!["?"; table.columns.len()].join(", ");
    let mut inserted = 0usize;
    {
        let mut stmt = tx.prepare(&format!("INSERT INTO {quoted_table} ({columns}) VALUES ({placeholders})"))?;
        for row in &table.rows {
            let params = row.iter().map(|v| match v {
                SqlValue::Text(s) if options.empty_as_null && s.is_empty() => &SqlValue::Null,
                other => other,
            });
            inserted += stmt.execute(rusqlite::params_from_iter(params))?;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_normalisation() {
        let raw = vec![
            vec![SqlValue::Text(" a ".into()), SqlValue::Text("".into()), SqlValue::Text("A".into())],
            vec![SqlValue::Integer(1)],
        ];
        let table = build_table(raw, true);
        assert_eq!(table.columns, vec!["a", "column_2", "A_2"]);
        assert_eq!(table.rows, vec![vec![SqlValue::Integer(1), SqlValue::Null, SqlValue::Null]]);
    }

    #[test]
    fn headerless() {
        let table = build_table(vec![vec![SqlValue::Integer(1), SqlValue::Integer(2)]], false);
        assert_eq!(table.columns, vec!["column_1", "column_2"]);
        assert_eq!(table.rows.len(), 1);
    }
}
