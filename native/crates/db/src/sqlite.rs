//! Port of `src/main/services/sqliteAdapter.ts` on top of `rusqlite`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use fallible_iterator::FallibleIterator;
use rusqlite::types::{ToSql, ToSqlOutput, Value, ValueRef};
use rusqlite::{Connection, InterruptHandle};

use crate::error::{DbError, Result};
use crate::service::SqlAdapter;
use crate::types::*;

/// Row cap applied to `execute_query` when the request has no `limit`.
pub const DEFAULT_ROW_LIMIT: usize = 10_000;
/// Upper bound for any requested row cap.
pub const MAX_ROW_LIMIT: usize = 1_000_000;
const DEFAULT_BROWSE_LIMIT: usize = 100;
const MEMORY_DB: &str = ":memory:";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock only means another thread panicked mid-query; the connection is still usable.
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl ToSql for SqlValue {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self {
            Self::Null => ToSqlOutput::Owned(Value::Null),
            Self::Integer(v) => ToSqlOutput::Owned(Value::Integer(*v)),
            Self::Real(v) => ToSqlOutput::Owned(Value::Real(*v)),
            Self::Text(v) => ToSqlOutput::Borrowed(ValueRef::Text(v.as_bytes())),
            Self::Blob(v) => ToSqlOutput::Borrowed(ValueRef::Blob(v)),
        })
    }
}

/// Port of `normalizeCellValue` (blobs become `0x...` previews).
fn display_value(value: ValueRef<'_>) -> SqlValue {
    match value {
        ValueRef::Null => SqlValue::Null,
        ValueRef::Integer(v) => SqlValue::Integer(v),
        ValueRef::Real(v) => SqlValue::Real(v),
        ValueRef::Text(bytes) => SqlValue::Text(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => SqlValue::Text(blob_preview(bytes)),
    }
}

fn raw_value(value: ValueRef<'_>) -> SqlValue {
    match value {
        ValueRef::Blob(bytes) => SqlValue::Blob(bytes.to_vec()),
        other => display_value(other),
    }
}

pub(crate) fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn build_table_id(schema: Option<&str>, name: &str) -> String {
    format!("{}.{name}", schema.unwrap_or("main"))
}

/// `(schema, table)` from `schema.table`; the table may itself contain dots.
fn parse_table_id(table_id: &str) -> (&str, &str) {
    match table_id.find('.') {
        None => ("main", table_id),
        Some(i) => (&table_id[..i], &table_id[i + 1..]),
    }
}

/// Lexical `path.resolve` (no symlink resolution, like Node).
fn resolve_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Canonicalise `path`; when it does not exist yet, canonicalise the parent directory and
/// re-append the file name (so a symlinked parent cannot be used to escape).
fn canonicalize_lenient(path: &Path) -> PathBuf {
    if let Ok(real) = std::fs::canonicalize(path) {
        return real;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => match std::fs::canonicalize(parent) {
            Ok(real_parent) => real_parent.join(name),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
    }
}

/// Key under which a connection is stored: canonical absolute path (or `:memory:`).
/// Pure path math, no policy check.
fn database_key(connection_id: &str) -> PathBuf {
    let raw = connection_id.strip_prefix("sqlite:").unwrap_or(connection_id);
    if raw == MEMORY_DB {
        return PathBuf::from(MEMORY_DB);
    }
    canonicalize_lenient(&resolve_path(Path::new(raw)))
}

fn is_within(parent: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        let lower = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
        lower(target).starts_with(lower(parent))
    }
    #[cfg(not(windows))]
    {
        target.starts_with(parent)
    }
}

#[derive(Debug, Clone)]
struct ColumnInfo {
    name: String,
    data_type: String,
    notnull: bool,
    pk: i64,
}

struct TableDescription {
    table_name: String,
    table_type: TableType,
    columns: Vec<SqlColumnMetadata>,
    primary_key_columns: Vec<String>,
}

/// An open database: the connection plus an interrupt handle usable while it is locked.
struct Entry {
    conn: Mutex<Connection>,
    interrupt: InterruptHandle,
}

enum PathPolicy {
    /// Databases must live inside the workspace root (`None` = no workspace open = error).
    Workspace(Option<PathBuf>),
    Unrestricted,
}

/// SQLite implementation of [`SqlAdapter`].  Connection ids look like `sqlite:/abs/path.db`.
///
/// **Scope of the workspace confinement:** it only decides which database *files* the UI may
/// open (a guard against typos and accidental symlink escapes, with the parent directory
/// canonicalised before the check).  It is *not* a security boundary: users type their own SQL,
/// so statements such as `ATTACH DATABASE` and `VACUUM INTO` are deliberately allowed and can
/// reach any file the process can.
pub struct SqliteAdapter {
    policy: Mutex<PathPolicy>,
    databases: Mutex<HashMap<PathBuf, Arc<Entry>>>,
}

impl Default for SqliteAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl SqliteAdapter {
    /// Adapter confined to the workspace set by [`Self::set_workspace_root`] (as in the TS code,
    /// no workspace means every connection fails).
    pub fn new() -> Self {
        Self {
            policy: Mutex::new(PathPolicy::Workspace(None)),
            databases: Mutex::new(HashMap::new()),
        }
    }

    /// Adapter that may open any path (and `:memory:`).
    pub fn unrestricted() -> Self {
        Self {
            policy: Mutex::new(PathPolicy::Unrestricted),
            databases: Mutex::new(HashMap::new()),
        }
    }

    /// Port of `workspaceService.setRootPath` as far as the adapter is concerned.
    ///
    /// Connections whose database lies outside the new root (all of them for `None`) are closed.
    pub fn set_workspace_root(&self, root: Option<&Path>) {
        let root = root.map(|r| canonicalize_lenient(&resolve_path(r)));
        *lock(&self.policy) = PathPolicy::Workspace(root.clone());
        lock(&self.databases).retain(|path, _| {
            path.as_os_str() == MEMORY_DB
                || root.as_ref().is_some_and(|r| is_within(r, path))
        });
    }

    /// Open (or reuse) the connection for `connection_id`.
    pub fn open(&self, connection_id: &str) -> Result<()> {
        self.entry(connection_id).map(|_| ())
    }

    /// Close one connection; returns whether it was open.
    pub fn close(&self, connection_id: &str) -> bool {
        lock(&self.databases).remove(&database_key(connection_id)).is_some()
    }

    /// Number of currently open connections.
    pub fn open_count(&self) -> usize {
        lock(&self.databases).len()
    }

    /// Abort the statement currently running on this connection (no-op when idle).
    /// May be called from any thread while another thread is inside `execute_query`.
    pub fn interrupt(&self, connection_id: &str) {
        // Looked up by resolved path only: no workspace-policy check, so this keeps working
        // after the workspace changed.
        if let Some(entry) = lock(&self.databases).get(&database_key(connection_id)) {
            entry.interrupt.interrupt();
        }
    }

    /// Indexes of a table with their columns (`PRAGMA index_list` / `index_info`).
    pub fn list_indexes(&self, connection_id: &str, table_id: &str) -> Result<Vec<SqlIndexInfo>> {
        let entry = self.entry(connection_id)?;
        let conn = lock(&entry.conn);
        let table = describe_table(&conn, table_id)?;
        let quoted = quote_identifier(&table.table_name);
        let mut list = conn.prepare(&format!("PRAGMA index_list({quoted})"))?;
        let raw: Vec<(String, bool, String, bool)> = list
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? != 0,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)? != 0,
                ))
            })?
            .collect::<std::result::Result<_, _>>()?;
        let mut indexes = Vec::with_capacity(raw.len());
        for (name, unique, origin, partial) in raw {
            let mut info = conn.prepare(&format!("PRAGMA index_info({})", quote_identifier(&name)))?;
            let mut columns: Vec<(i64, String)> = info
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(2)?.unwrap_or_default()))
                })?
                .collect::<std::result::Result<_, _>>()?;
            columns.sort_by_key(|(seq, _)| *seq);
            indexes.push(SqlIndexInfo {
                name,
                unique,
                origin,
                partial,
                columns: columns.into_iter().map(|(_, c)| c).collect(),
            });
        }
        Ok(indexes)
    }

    fn resolve_database_path(&self, connection_id: &str) -> Result<PathBuf> {
        let raw = connection_id.strip_prefix("sqlite:").unwrap_or(connection_id);
        match &*lock(&self.policy) {
            PathPolicy::Unrestricted => Ok(database_key(connection_id)),
            PathPolicy::Workspace(None) => Err(DbError::NoWorkspace),
            PathPolicy::Workspace(Some(root)) => {
                if raw == MEMORY_DB {
                    return Err(DbError::OutsideWorkspace { label: "SQLite database path".into() });
                }
                let resolved = database_key(connection_id);
                if is_within(root, &resolved) {
                    Ok(resolved)
                } else {
                    Err(DbError::OutsideWorkspace { label: "SQLite database path".into() })
                }
            }
        }
    }

    fn entry(&self, connection_id: &str) -> Result<Arc<Entry>> {
        let path = self.resolve_database_path(connection_id)?;
        let mut databases = lock(&self.databases);
        if let Some(entry) = databases.get(&path) {
            return Ok(Arc::clone(entry));
        }
        let conn = if path.as_os_str() == MEMORY_DB {
            Connection::open_in_memory()?
        } else {
            Connection::open(&path)?
        };
        // better-sqlite3 defaults: 5 s busy timeout, foreign keys enforced.
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        // Bundled SQLite enables double-quoted string literals by default, which turns a typo'd
        // identifier like "ghost" into the string 'ghost' (and `"ghost" = 'ghost'` into TRUE).
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DQS_DML, false)?;
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DQS_DDL, false)?;
        let entry = Arc::new(Entry {
            interrupt: conn.get_interrupt_handle(),
            conn: Mutex::new(conn),
        });
        databases.insert(path, Arc::clone(&entry));
        Ok(entry)
    }

    fn run_query(&self, request: &SqlQueryRequest) -> Result<SqlQueryResult> {
        let sql = request.sql.trim();
        let started = Instant::now();
        if sql.is_empty() {
            return Err(DbError::invalid("Enter a SQL statement before running the console."));
        }
        let entry = self.entry(&request.connection_id)?;
        let conn = lock(&entry.conn);

        let timed_out = Arc::new(AtomicBool::new(false));
        if let Some(timeout_ms) = request.timeout_ms.filter(|ms| *ms > 0) {
            // The deadline starts once we own the connection, not while queued behind another query.
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let flag = Arc::clone(&timed_out);
            conn.progress_handler(
                1000,
                Some(move || {
                    let expired = Instant::now() >= deadline;
                    if expired {
                        flag.store(true, Ordering::Relaxed);
                    }
                    expired
                }),
            )?;
        }
        let outcome = execute_statement(&conn, sql, request, started);
        let _ = conn.progress_handler(0, None::<fn() -> bool>);
        drop(conn);

        match outcome {
            Err(DbError::Sqlite(rusqlite::Error::SqliteFailure(err, _)))
                if err.code == rusqlite::ErrorCode::OperationInterrupted =>
            {
                if timed_out.load(Ordering::Relaxed) {
                    Err(DbError::Timeout(request.timeout_ms.unwrap_or_default()))
                } else {
                    Err(DbError::Cancelled)
                }
            }
            other => other,
        }
    }

    fn run_schema(&self, connection_id: &str) -> Result<SqlSchemaSummary> {
        let entry = self.entry(connection_id)?;
        let conn = lock(&entry.conn);
        let mut stmt = conn.prepare(
            "SELECT name, type FROM sqlite_master \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
             ORDER BY type, name",
        )?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        drop(stmt);

        let mut tables = Vec::with_capacity(rows.len());
        let mut columns_by_table = BTreeMap::new();
        for (name, kind) in rows {
            let id = build_table_id(Some("main"), &name);
            let description = describe_table(&conn, &id)?;
            columns_by_table.insert(id.clone(), description.columns);
            tables.push(SqlTableSummary {
                id,
                schema: Some("main".into()),
                name,
                table_type: if kind == "view" { TableType::View } else { TableType::Table },
            });
        }
        Ok(SqlSchemaSummary {
            connection_id: connection_id.to_string(),
            tables,
            columns_by_table,
            error: None,
        })
    }

    fn run_table_rows(&self, request: &SqlBrowseTableRequest) -> Result<SqlTableRowsResult> {
        let entry = self.entry(&request.connection_id)?;
        let conn = lock(&entry.conn);
        let table = describe_table(&conn, &request.table_id)?;
        let limit = request.limit.unwrap_or(DEFAULT_BROWSE_LIMIT);
        let offset = request.offset.unwrap_or(0);
        let browse = build_browse_query_parts(request, &table.columns)?;
        let quoted_table = quote_identifier(&table.table_name);

        let count_sql = format!("SELECT COUNT(*) FROM {quoted_table} {}", browse.where_clause);
        let total_rows: i64 = conn.query_row(
            &count_sql,
            rusqlite::params_from_iter(browse.params.iter()),
            |row| row.get(0),
        )?;

        let mut result = SqlTableRowsResult {
            connection_id: request.connection_id.clone(),
            table_id: request.table_id.clone(),
            columns: table.columns.clone(),
            rows: Vec::new(),
            writable: false,
            limit: Some(limit),
            offset: Some(offset),
            total_rows: u64::try_from(total_rows).unwrap_or(0),
            sort_column: request.sort_column.clone(),
            sort_direction: request.sort_direction,
            filters: request.filters.clone(),
            error: None,
            message: None,
        };

        let column_list = table
            .columns
            .iter()
            .map(|c| quote_identifier(&c.name))
            .collect::<Vec<_>>()
            .join(", ");
        let tail = format!(
            "FROM {quoted_table} {} {} LIMIT ? OFFSET ?",
            browse.where_clause, browse.order_clause
        );
        let mut params: Vec<SqlValue> = browse.params.clone();
        params.push(SqlValue::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
        params.push(SqlValue::Integer(i64::try_from(offset).unwrap_or(i64::MAX)));

        let pk_indexes: Vec<usize> = table
            .primary_key_columns
            .iter()
            .filter_map(|pk| table.columns.iter().position(|c| &c.name == pk))
            .collect();

        if table.table_type == TableType::View {
            result.rows = read_rows(&conn, &format!("SELECT {column_list} {tail}"), &params, RowKind::Plain)?;
            result.message = Some("Views are shown read-only.".into());
            return Ok(result);
        }

        if !table.primary_key_columns.is_empty() {
            result.rows = read_rows(
                &conn,
                &format!("SELECT {column_list} {tail}"),
                &params,
                RowKind::PrimaryKey(&table.primary_key_columns, &pk_indexes),
            )?;
            result.writable = true;
            result.message = Some(loaded_message(result.rows.len()));
            return Ok(result);
        }

        let rowid_sql = format!("SELECT rowid AS __sql_rowid, {column_list} {tail}");
        match read_rows(&conn, &rowid_sql, &params, RowKind::Rowid) {
            Ok(rows) => {
                result.rows = rows;
                result.writable = true;
                result.message = Some(loaded_message(result.rows.len()));
            }
            Err(_) => {
                result.rows = read_rows(&conn, &format!("SELECT {column_list} {tail}"), &params, RowKind::Plain)?;
                result.message = Some(
                    "Loaded rows in read-only mode because the table has no primary key or rowid access."
                        .into(),
                );
            }
        }
        Ok(result)
    }

    fn run_update(&self, request: &SqlUpdateRowRequest) -> Result<SqlMutationResult> {
        let entry = self.entry(&request.connection_id)?;
        let mut conn = lock(&entry.conn);
        let table = describe_table(&conn, &request.table_id)?;
        if table.table_type == TableType::View {
            return Err(DbError::invalid("Views are read-only."));
        }
        let valid = valid_column_names(&table);
        let next: Vec<(&String, &SqlValue)> =
            request.values.iter().filter(|(name, _)| valid.contains(name.as_str())).collect();
        if next.is_empty() {
            return Err(DbError::invalid("Choose at least one editable column before saving the row."));
        }
        let set_clause = next
            .iter()
            .map(|(name, _)| format!("{} = ?", quote_identifier(name)))
            .collect::<Vec<_>>()
            .join(", ");
        let (where_clause, where_params) = build_where_clause(&request.locator, &table)?;
        let sql = format!(
            "UPDATE {} SET {set_clause} WHERE {where_clause}",
            quote_identifier(&table.table_name)
        );
        let mut params: Vec<&SqlValue> = next.iter().map(|(_, v)| *v).collect();
        params.extend(where_params.iter());
        let changes = execute_single_row_change(&mut conn, &sql, params)?;
        Ok(mutation_ok(request.connection_id.clone(), request.table_id.clone(), changes, "updated", "Row updated."))
    }

    fn run_insert(&self, request: &SqlInsertRowRequest) -> Result<SqlMutationResult> {
        let entry = self.entry(&request.connection_id)?;
        let conn = lock(&entry.conn);
        let table = describe_table(&conn, &request.table_id)?;
        if table.table_type == TableType::View {
            return Err(DbError::invalid("Views are read-only."));
        }
        let valid = valid_column_names(&table);
        let next: Vec<(&String, &SqlValue)> =
            request.values.iter().filter(|(name, _)| valid.contains(name.as_str())).collect();
        let quoted_table = quote_identifier(&table.table_name);
        let changes = if next.is_empty() {
            conn.execute(&format!("INSERT INTO {quoted_table} DEFAULT VALUES"), [])?
        } else {
            let columns = next.iter().map(|(n, _)| quote_identifier(n)).collect::<Vec<_>>().join(", ");
            let placeholders = vec!["?"; next.len()].join(", ");
            conn.execute(
                &format!("INSERT INTO {quoted_table} ({columns}) VALUES ({placeholders})"),
                rusqlite::params_from_iter(next.iter().map(|(_, v)| *v)),
            )?
        };
        Ok(mutation_ok(request.connection_id.clone(), request.table_id.clone(), changes, "inserted", "Row inserted."))
    }

    fn run_delete(&self, request: &SqlDeleteRowRequest) -> Result<SqlMutationResult> {
        let entry = self.entry(&request.connection_id)?;
        let mut conn = lock(&entry.conn);
        let table = describe_table(&conn, &request.table_id)?;
        if table.table_type == TableType::View {
            return Err(DbError::invalid("Views are read-only."));
        }
        let (where_clause, params) = build_where_clause(&request.locator, &table)?;
        let changes = execute_single_row_change(
            &mut conn,
            &format!("DELETE FROM {} WHERE {where_clause}", quote_identifier(&table.table_name)),
            params.iter().collect(),
        )?;
        Ok(mutation_ok(request.connection_id.clone(), request.table_id.clone(), changes, "deleted", "Row deleted."))
    }

    /// Bulk-insert imported rows (see [`crate::import`]) into `table_name` in one transaction.
    /// Returns the number of inserted rows.
    pub fn import_table(
        &self,
        connection_id: &str,
        table_name: &str,
        table: &crate::import::ImportedTable,
        options: &crate::import::ImportOptions,
    ) -> Result<usize> {
        crate::import::insert_imported(&self.entry(connection_id)?.conn, table_name, table, options)
    }
}

/// Run an UPDATE/DELETE in a transaction; if it touched more than one row (a locator that does
/// not identify a single row, e.g. a duplicate rowid-less key) roll everything back.
fn execute_single_row_change(
    conn: &mut Connection,
    sql: &str,
    params: Vec<&SqlValue>,
) -> Result<usize> {
    let tx = conn.transaction()?;
    let changes = tx.execute(sql, rusqlite::params_from_iter(params))?;
    if changes > 1 {
        drop(tx); // rolls back
        return Err(DbError::invalid(format!(
            "The row locator matched {changes} rows; no changes were made."
        )));
    }
    tx.commit()?;
    Ok(changes)
}

fn valid_column_names(table: &TableDescription) -> HashSet<&str> {
    table.columns.iter().map(|c| c.name.as_str()).collect()
}

fn loaded_message(count: usize) -> String {
    if count == 1 {
        "Loaded 1 row.".into()
    } else {
        format!("Loaded {count} rows.")
    }
}

fn mutation_ok(
    connection_id: String,
    table_id: String,
    changes: usize,
    verb: &str,
    single: &str,
) -> SqlMutationResult {
    SqlMutationResult {
        connection_id,
        table_id,
        success: true,
        row_count: changes,
        error: None,
        message: Some(if changes == 1 { single.to_string() } else { format!("{changes} rows {verb}.") }),
    }
}

fn mutation_err(connection_id: &str, table_id: &str, error: &DbError) -> SqlMutationResult {
    SqlMutationResult {
        connection_id: connection_id.to_string(),
        table_id: table_id.to_string(),
        success: false,
        row_count: 0,
        error: Some(error.to_string()),
        message: None,
    }
}

/// Run one statement (or a `;`-separated script) and package the outcome.
///
/// Statements run in order.  The rows of the last row-returning statement are the result, so a
/// script may end with a `SELECT`; a script without one reports a plain success message
/// (like `database.exec` in the TS adapter).
fn execute_statement(
    conn: &Connection,
    sql: &str,
    request: &SqlQueryRequest,
    started: Instant,
) -> Result<SqlQueryResult> {
    let elapsed = |s: Instant| u64::try_from(s.elapsed().as_millis()).unwrap_or(u64::MAX);
    let limit = request.limit.unwrap_or(DEFAULT_ROW_LIMIT).clamp(1, MAX_ROW_LIMIT);
    let offset = request.offset.unwrap_or(0);

    let mut result = SqlQueryResult {
        connection_id: request.connection_id.clone(),
        success: true,
        ..SqlQueryResult::default()
    };
    let mut statements = 0usize;
    let mut changes = 0usize;
    let mut has_rows = false;

    let mut batch = rusqlite::Batch::new(conn, sql);
    while let Some(mut statement) = batch.next()? {
        statements += 1;
        if statement.column_count() == 0 {
            changes = statement.execute([])?;
            continue;
        }

        has_rows = true;
        result.columns = statement.column_names().into_iter().map(str::to_string).collect();
        result.rows.clear();
        result.truncated = false;
        let column_count = result.columns.len();
        let mut rows = statement.query([])?;
        let mut skipped = 0usize;
        while let Some(row) = rows.next()? {
            if skipped < offset {
                skipped += 1;
                continue;
            }
            if result.rows.len() >= limit {
                result.truncated = true;
                break;
            }
            let mut cells = Vec::with_capacity(column_count);
            for i in 0..column_count {
                cells.push(display_value(row.get_ref(i)?));
            }
            result.rows.push(cells);
        }
    }

    result.duration_ms = elapsed(started);
    if has_rows {
        let count = result.rows.len();
        result.row_count = count;
        let mut message = if count == 1 { "Returned 1 row.".to_string() } else { format!("Returned {count} rows.") };
        if result.truncated {
            message.push_str(&format!(" Result capped at {limit} rows; more rows are available."));
        }
        result.message = Some(message);
    } else if statements > 1 {
        result.message = Some("Executed SQL script successfully.".into());
    } else if statements == 0 {
        result.message = Some("Nothing to execute.".into());
    } else {
        result.row_count = changes;
        result.message = Some(if changes == 1 {
            "1 row affected.".into()
        } else {
            format!("{changes} rows affected.")
        });
    }
    Ok(result)
}

enum RowKind<'a> {
    Plain,
    PrimaryKey(&'a [String], &'a [usize]),
    Rowid,
}

fn read_rows(
    conn: &Connection,
    sql: &str,
    params: &[SqlValue],
    kind: RowKind<'_>,
) -> Result<Vec<SqlTableRow>> {
    let mut statement = conn.prepare(sql)?;
    let mut rows = statement.query(rusqlite::params_from_iter(params.iter()))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let column_count = row.as_ref().column_count();
        let (locator, first) = match &kind {
            RowKind::Plain => (None, 0),
            RowKind::Rowid => {
                let mut values = BTreeMap::new();
                values.insert("rowid".to_string(), raw_value(row.get_ref(0)?));
                (Some(SqlRowLocator { kind: RowLocatorKind::Rowid, values }), 1)
            }
            RowKind::PrimaryKey(names, indexes) => {
                let mut values = BTreeMap::new();
                for (name, index) in names.iter().zip(indexes.iter()) {
                    values.insert(name.clone(), raw_value(row.get_ref(*index)?));
                }
                (Some(SqlRowLocator { kind: RowLocatorKind::PrimaryKey, values }), 0)
            }
        };
        let mut cells = Vec::with_capacity(column_count - first);
        for i in first..column_count {
            cells.push(display_value(row.get_ref(i)?));
        }
        out.push(SqlTableRow { locator, values: cells });
    }
    Ok(out)
}

fn describe_table(conn: &Connection, table_id: &str) -> Result<TableDescription> {
    let (_, table_name) = parse_table_id(table_id);
    let meta: Option<String> = {
        let mut stmt = conn.prepare(
            "SELECT type FROM sqlite_master WHERE name = ? AND type IN ('table', 'view') LIMIT 1",
        )?;
        let mut rows = stmt.query([table_name])?;
        match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        }
    };
    let Some(kind) = meta else {
        return Err(DbError::invalid("The selected table could not be found in this SQLite file."));
    };

    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", quote_identifier(table_name)))?;
    let raw: Vec<ColumnInfo> = stmt
        .query_map([], |row| {
            Ok(ColumnInfo {
                name: row.get(1)?,
                data_type: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                notnull: row.get::<_, i64>(3)? != 0,
                pk: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    if raw.is_empty() {
        return Err(DbError::invalid("The selected table does not expose any columns."));
    }

    let mut pk_columns: Vec<&ColumnInfo> = raw.iter().filter(|c| c.pk > 0).collect();
    pk_columns.sort_by_key(|c| c.pk);
    let primary_key_columns = pk_columns.iter().map(|c| c.name.clone()).collect();

    // Port of `mapColumnMetadata`: primary key columns first (by pk order), stable otherwise.
    let mut ordered = raw.clone();
    ordered.sort_by_key(|c| if c.pk > 0 { c.pk } else { i64::MAX });
    let columns = ordered
        .into_iter()
        .map(|c| SqlColumnMetadata {
            data_type: if c.data_type.is_empty() { "TEXT".to_string() } else { c.data_type },
            nullable: !c.notnull,
            is_primary_key: c.pk > 0,
            name: c.name,
        })
        .collect();

    Ok(TableDescription {
        table_name: table_name.to_string(),
        table_type: if kind == "view" { TableType::View } else { TableType::Table },
        columns,
        primary_key_columns,
    })
}

/// WHERE clause for a row locator.  The locator must name *exactly* the table's primary key
/// columns (or exactly `rowid`); anything else could match arbitrary rows.
fn build_where_clause(
    locator: &SqlRowLocator,
    table: &TableDescription,
) -> Result<(String, Vec<SqlValue>)> {
    match locator.kind {
        RowLocatorKind::Rowid => {
            let value = locator
                .values
                .get("rowid")
                .ok_or_else(|| DbError::invalid("Row locator is missing the SQLite rowid value."))?;
            if locator.values.len() != 1 {
                return Err(DbError::invalid("Row locator must contain only the rowid value."));
            }
            Ok(("rowid = ?".to_string(), vec![value.clone()]))
        }
        RowLocatorKind::PrimaryKey => {
            if locator.values.is_empty() {
                return Err(DbError::invalid("Primary key locator must include at least one key column."));
            }
            let expected: std::collections::BTreeSet<&str> =
                table.primary_key_columns.iter().map(String::as_str).collect();
            let actual: std::collections::BTreeSet<&str> =
                locator.values.keys().map(String::as_str).collect();
            if expected.is_empty() || expected != actual {
                return Err(DbError::invalid(
                    "Primary key locator must match the table's primary key columns exactly.",
                ));
            }
            let clause = locator
                .values
                .keys()
                .map(|k| format!("{} = ?", quote_identifier(k)))
                .collect::<Vec<_>>()
                .join(" AND ");
            Ok((clause, locator.values.values().cloned().collect()))
        }
    }
}

fn escape_like_pattern(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Port of `parseFilterValue`.
fn parse_filter_value(raw: &str, column: &SqlColumnMetadata) -> SqlValue {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return SqlValue::Text(raw.to_string());
    }
    let data_type = column.data_type.to_uppercase();
    if data_type.contains("BOOL") {
        match trimmed.to_lowercase().as_str() {
            "true" | "1" => return SqlValue::Integer(1),
            "false" | "0" => return SqlValue::Integer(0),
            _ => {}
        }
    }
    let digits = trimmed.strip_prefix('-').unwrap_or(trimmed);
    if data_type.contains("INT") && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(v) = trimmed.parse::<i64>() {
            return SqlValue::Integer(v);
        }
    }
    if ["REAL", "FLOA", "DOUB", "NUM", "DEC"].iter().any(|t| data_type.contains(t)) {
        if let Ok(v) = trimmed.parse::<f64>() {
            if v.is_finite() {
                return SqlValue::Real(v);
            }
        }
    }
    SqlValue::Text(raw.to_string())
}

struct Clause {
    clause: String,
    params: Vec<SqlValue>,
}

fn build_column_filter_clause(
    column: &SqlColumnMetadata,
    operator: SqlFilterOperator,
    raw_value: Option<&str>,
) -> Result<Option<Clause>> {
    let expr = quote_identifier(&column.name);
    match operator {
        SqlFilterOperator::IsNull => {
            return Ok(Some(Clause { clause: format!("{expr} IS NULL"), params: vec![] }))
        }
        SqlFilterOperator::IsNotNull => {
            return Ok(Some(Clause { clause: format!("{expr} IS NOT NULL"), params: vec![] }))
        }
        _ => {}
    }
    let Some(raw) = raw_value.filter(|v| !v.trim().is_empty()) else {
        return Ok(None);
    };

    if matches!(
        operator,
        SqlFilterOperator::Contains | SqlFilterOperator::StartsWith | SqlFilterOperator::EndsWith
    ) {
        let escaped = escape_like_pattern(raw.trim());
        let pattern = match operator {
            SqlFilterOperator::StartsWith => format!("{escaped}%"),
            SqlFilterOperator::EndsWith => format!("%{escaped}"),
            _ => format!("%{escaped}%"),
        };
        return Ok(Some(Clause {
            clause: format!("CAST({expr} AS TEXT) LIKE ? ESCAPE '\\'"),
            params: vec![SqlValue::Text(pattern)],
        }));
    }

    let comparison = match operator {
        SqlFilterOperator::Equals => "=",
        SqlFilterOperator::NotEquals => "!=",
        SqlFilterOperator::GreaterThan => ">",
        SqlFilterOperator::GreaterThanOrEqual => ">=",
        SqlFilterOperator::LessThan => "<",
        SqlFilterOperator::LessThanOrEqual => "<=",
        _ => {
            return Err(DbError::invalid(
                "The selected filter operator is not supported by SQLite browsing.",
            ))
        }
    };
    Ok(Some(Clause {
        clause: format!("{expr} {comparison} ?"),
        params: vec![parse_filter_value(raw, column)],
    }))
}

fn build_filter_clause(
    filter: &SqlBrowseFilter,
    columns: &[SqlColumnMetadata],
) -> Result<Option<Clause>> {
    let find = |name: &str| {
        columns
            .iter()
            .find(|c| c.name == name)
            .ok_or_else(|| DbError::invalid("The selected filter column is no longer available."))
    };
    let operator = filter.operator;

    if matches!(operator, SqlFilterOperator::IsNull | SqlFilterOperator::IsNotNull) {
        let name = filter
            .column
            .as_deref()
            .filter(|c| !c.is_empty())
            .ok_or_else(|| DbError::invalid("Choose a column before using null filters."))?;
        return build_column_filter_clause(find(name)?, operator, filter.value.as_deref());
    }

    let Some(value) = filter.value.as_deref().filter(|v| !v.trim().is_empty()) else {
        return Ok(None);
    };

    if let Some(name) = filter.column.as_deref().filter(|c| !c.is_empty()) {
        return build_column_filter_clause(find(name)?, operator, Some(value));
    }

    if operator != SqlFilterOperator::Contains {
        return Err(DbError::invalid("Choose a column before using this filter operator."));
    }

    let mut predicates = Vec::new();
    for column in columns {
        if let Some(clause) = build_column_filter_clause(column, operator, Some(value))? {
            predicates.push(clause);
        }
    }
    if predicates.is_empty() {
        return Ok(None);
    }
    Ok(Some(Clause {
        clause: format!(
            "({})",
            predicates.iter().map(|p| p.clause.as_str()).collect::<Vec<_>>().join(" OR ")
        ),
        params: predicates.into_iter().flat_map(|p| p.params).collect(),
    }))
}

struct BrowseQueryParts {
    where_clause: String,
    params: Vec<SqlValue>,
    order_clause: String,
}

fn build_browse_query_parts(
    request: &SqlBrowseTableRequest,
    columns: &[SqlColumnMetadata],
) -> Result<BrowseQueryParts> {
    let mut params = Vec::new();
    let mut clauses = Vec::new();
    for filter in request.filters.iter().flatten() {
        if let Some(clause) = build_filter_clause(filter, columns)? {
            params.extend(clause.params);
            clauses.push(clause.clause);
        }
    }
    let where_clause = if clauses.is_empty() { String::new() } else { format!("WHERE {}", clauses.join(" AND ")) };

    let order_clause = match request.sort_column.as_deref().filter(|c| !c.is_empty()) {
        None => String::new(),
        Some(column) => {
            if !columns.iter().any(|c| c.name == column) {
                return Err(DbError::invalid("The selected sort column is no longer available."));
            }
            let direction = if request.sort_direction == Some(SortDirection::Desc) { "DESC" } else { "ASC" };
            format!("ORDER BY {} {direction}", quote_identifier(column))
        }
    };
    Ok(BrowseQueryParts { where_clause, params, order_clause })
}

impl SqlAdapter for SqliteAdapter {
    fn dialect(&self) -> SqlDialect {
        SqlDialect::Sqlite
    }

    fn execute_query(&self, request: &SqlQueryRequest) -> SqlQueryResult {
        let started = Instant::now();
        match self.run_query(request) {
            Ok(result) => result,
            Err(error) => SqlQueryResult {
                connection_id: request.connection_id.clone(),
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                error: Some(error.to_string()),
                ..SqlQueryResult::default()
            },
        }
    }

    fn get_schema(&self, connection_id: &str) -> SqlSchemaSummary {
        self.run_schema(connection_id).unwrap_or_else(|error| SqlSchemaSummary {
            connection_id: connection_id.to_string(),
            error: Some(error.to_string()),
            ..SqlSchemaSummary::default()
        })
    }

    fn get_table_rows(&self, request: &SqlBrowseTableRequest) -> SqlTableRowsResult {
        self.run_table_rows(request).unwrap_or_else(|error| SqlTableRowsResult {
            connection_id: request.connection_id.clone(),
            table_id: request.table_id.clone(),
            error: Some(error.to_string()),
            ..SqlTableRowsResult::default()
        })
    }

    fn update_row(&self, request: &SqlUpdateRowRequest) -> SqlMutationResult {
        self.run_update(request)
            .unwrap_or_else(|e| mutation_err(&request.connection_id, &request.table_id, &e))
    }

    fn insert_row(&self, request: &SqlInsertRowRequest) -> SqlMutationResult {
        self.run_insert(request)
            .unwrap_or_else(|e| mutation_err(&request.connection_id, &request.table_id, &e))
    }

    fn delete_row(&self, request: &SqlDeleteRowRequest) -> SqlMutationResult {
        self.run_delete(request)
            .unwrap_or_else(|e| mutation_err(&request.connection_id, &request.table_id, &e))
    }

    fn dispose(&self) {
        lock(&self.databases).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_row_change_is_rolled_back() {
        let mut conn = Connection::open_in_memory().expect("open");
        conn.execute_batch("CREATE TABLE d (a); INSERT INTO d VALUES (1), (1), (2);").expect("setup");
        let one = SqlValue::Integer(1);
        let err = execute_single_row_change(&mut conn, "DELETE FROM d WHERE a = ?", vec![&one]).unwrap_err();
        assert!(err.to_string().contains("matched 2 rows"), "{err}");
        let left: i64 = conn.query_row("SELECT count(*) FROM d", [], |r| r.get(0)).expect("count");
        assert_eq!(left, 3);
        let two = SqlValue::Integer(2);
        assert_eq!(execute_single_row_change(&mut conn, "DELETE FROM d WHERE a = ?", vec![&two]).expect("one"), 1);
    }
}
