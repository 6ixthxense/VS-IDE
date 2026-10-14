//! Port of `src/test/main/sqliteAdapter.test.ts` plus coverage for the native-only features
//! (paging, row cap, timeout/interrupt, indexes, export, import).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use vsm_db::*;

fn workspace() -> (TempDir, PathBuf, SqliteAdapter) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let adapter = SqliteAdapter::new();
    adapter.set_workspace_root(Some(&root));
    (dir, root, adapter)
}

fn query(adapter: &SqliteAdapter, connection_id: &str, sql: &str) -> SqlQueryResult {
    adapter.execute_query(&SqlQueryRequest {
        connection_id: connection_id.to_string(),
        sql: sql.to_string(),
        ..SqlQueryRequest::default()
    })
}

fn browse(connection_id: &str, filters: Option<Vec<SqlBrowseFilter>>) -> SqlBrowseTableRequest {
    SqlBrowseTableRequest {
        connection_id: connection_id.to_string(),
        table_id: "main.users".into(),
        limit: Some(10),
        filters,
        ..SqlBrowseTableRequest::default()
    }
}

fn filter(column: Option<&str>, operator: SqlFilterOperator, value: Option<&str>) -> SqlBrowseFilter {
    SqlBrowseFilter {
        column: column.map(str::to_string),
        operator,
        value: value.map(str::to_string),
    }
}

fn names(result: &SqlTableRowsResult) -> Vec<String> {
    result
        .rows
        .iter()
        .map(|row| row.values[1].as_str().unwrap_or_default().to_string())
        .collect()
}

fn text(v: &str) -> SqlValue {
    SqlValue::Text(v.into())
}

fn seed(adapter: &SqliteAdapter, id: &str) {
    let create = query(
        adapter,
        id,
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER, nickname TEXT);",
    );
    assert!(create.success, "{create:?}");
    let insert = query(
        adapter,
        id,
        "
        INSERT INTO users (name, age, nickname) VALUES
          ('Mickey', 32, NULL),
          ('Mini', 14, 'min'),
          ('Donald', 52, 'duck');
    ",
    );
    assert!(insert.success, "{insert:?}");
}

#[test]
fn executes_queries_and_introspects_schema_for_workspace_sqlite_files() {
    let (_dir, root, adapter) = workspace();
    let database_path = root.join("sample.sqlite");
    let id = format!("sqlite:{}", database_path.display());

    seed(&adapter, &id);

    let result = query(&adapter, &id, "SELECT id, name FROM users ORDER BY id;");
    assert!(result.success, "{result:?}");
    assert_eq!(result.columns, vec!["id", "name"]);
    assert_eq!(
        result.rows,
        vec![
            vec![SqlValue::Integer(1), text("Mickey")],
            vec![SqlValue::Integer(2), text("Mini")],
            vec![SqlValue::Integer(3), text("Donald")],
        ]
    );
    assert_eq!(result.row_count, 3);
    assert_eq!(result.message.as_deref(), Some("Returned 3 rows."));

    let schema = adapter.get_schema(&id);
    assert_eq!(schema.error, None);
    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.tables[0].name, "users");
    assert_eq!(schema.tables[0].table_type, TableType::Table);
    assert_eq!(schema.tables[0].id, "main.users");
    let columns = &schema.columns_by_table["main.users"];
    let column = |name: &str| columns.iter().find(|c| c.name == name).expect("column");
    assert!(column("id").is_primary_key);
    assert_eq!(column("name").data_type, "TEXT");
    assert!(!column("name").nullable);
    assert_eq!(column("age").data_type, "INTEGER");
    assert_eq!(column("nickname").data_type, "TEXT");
    assert!(column("nickname").nullable);

    let rows = adapter.get_table_rows(&browse(&id, None));
    assert!(rows.writable);
    assert_eq!(
        rows.rows[0].values,
        vec![SqlValue::Integer(1), text("Mickey"), SqlValue::Integer(32), SqlValue::Null]
    );
    assert_eq!(rows.total_rows, 3);
    assert_eq!(rows.rows[0].locator.as_ref().map(|l| l.kind), Some(RowLocatorKind::PrimaryKey));

    let contains = adapter.get_table_rows(&browse(&id, Some(vec![filter(None, SqlFilterOperator::Contains, Some("Duck"))])));
    assert_eq!(names(&contains), ["Donald"]);

    let starts = adapter.get_table_rows(&browse(&id, Some(vec![filter(Some("name"), SqlFilterOperator::StartsWith, Some("Mi"))])));
    assert_eq!(names(&starts), ["Mickey", "Mini"]);

    let numeric = adapter.get_table_rows(&browse(&id, Some(vec![filter(Some("age"), SqlFilterOperator::GreaterThan, Some("20"))])));
    assert_eq!(names(&numeric), ["Mickey", "Donald"]);

    let nulls = adapter.get_table_rows(&browse(&id, Some(vec![filter(Some("nickname"), SqlFilterOperator::IsNull, None)])));
    assert_eq!(names(&nulls), ["Mickey"]);

    let multi = adapter.get_table_rows(&browse(
        &id,
        Some(vec![
            filter(Some("age"), SqlFilterOperator::GreaterThanOrEqual, Some("30")),
            filter(Some("name"), SqlFilterOperator::Contains, Some("ck")),
        ]),
    ));
    assert_eq!(names(&multi), ["Mickey"]);

    let update = adapter.update_row(&SqlUpdateRowRequest {
        connection_id: id.clone(),
        table_id: "main.users".into(),
        locator: SqlRowLocator {
            kind: RowLocatorKind::PrimaryKey,
            values: BTreeMap::from([("id".to_string(), SqlValue::Integer(1))]),
        },
        values: BTreeMap::from([("name".to_string(), text("Mouse"))]),
    });
    assert!(update.success, "{update:?}");
    assert_eq!(update.row_count, 1);
    assert_eq!(update.message.as_deref(), Some("Row updated."));

    let insert = adapter.insert_row(&SqlInsertRowRequest {
        connection_id: id.clone(),
        table_id: "main.users".into(),
        values: BTreeMap::from([
            ("name".to_string(), text("Daisy")),
            ("age".to_string(), SqlValue::Integer(28)),
        ]),
    });
    assert!(insert.success, "{insert:?}");

    let delete = adapter.delete_row(&SqlDeleteRowRequest {
        connection_id: id.clone(),
        table_id: "main.users".into(),
        locator: SqlRowLocator {
            kind: RowLocatorKind::PrimaryKey,
            values: BTreeMap::from([("id".to_string(), SqlValue::Integer(4))]),
        },
    });
    assert!(delete.success, "{delete:?}");

    let final_rows = adapter.get_table_rows(&browse(&id, None));
    let values: Vec<_> = final_rows.rows.iter().map(|r| r.values.clone()).collect();
    assert_eq!(
        values,
        vec![
            vec![SqlValue::Integer(1), text("Mouse"), SqlValue::Integer(32), SqlValue::Null],
            vec![SqlValue::Integer(2), text("Mini"), SqlValue::Integer(14), text("min")],
            vec![SqlValue::Integer(3), text("Donald"), SqlValue::Integer(52), text("duck")],
        ]
    );

    adapter.dispose();
    assert_eq!(adapter.open_count(), 0);
}

#[test]
fn rejects_paths_outside_workspace_and_requires_a_workspace() {
    let (_dir, root, adapter) = workspace();
    let outside = tempfile::tempdir().expect("tempdir");
    let id = format!("sqlite:{}", outside.path().join("x.db").display());
    let result = query(&adapter, &id, "SELECT 1");
    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("SQLite database path must stay inside the open workspace"));
    assert!(!outside.path().join("x.db").exists());

    // `..` traversal that resolves outside
    let sneaky = format!("sqlite:{}/../escape.db", root.display());
    assert!(!query(&adapter, &sneaky, "SELECT 1").success);

    let no_workspace = SqliteAdapter::new();
    let result = query(&no_workspace, &id, "SELECT 1");
    assert_eq!(result.error.as_deref(), Some("No workspace folder is open"));
}

#[test]
fn query_errors_empty_sql_scripts_and_non_reader_statements() {
    let (_dir, root, adapter) = workspace();
    let id = format!("sqlite:{}", root.join("a.db").display());

    let empty = query(&adapter, &id, "   ");
    assert!(!empty.success);
    assert_eq!(empty.error.as_deref(), Some("Enter a SQL statement before running the console."));

    let bad = query(&adapter, &id, "SELEC 1");
    assert!(!bad.success);
    assert!(bad.error.as_deref().unwrap_or_default().contains("syntax error"), "{bad:?}");

    let missing = query(&adapter, &id, "SELECT * FROM nope");
    assert!(missing.error.as_deref().unwrap_or_default().contains("no such table"), "{missing:?}");

    let script = query(&adapter, &id, "CREATE TABLE t (a); INSERT INTO t VALUES (1); INSERT INTO t VALUES (2);");
    assert!(script.success, "{script:?}");
    assert_eq!(script.message.as_deref(), Some("Executed SQL script successfully."));

    // trailing semicolon/whitespace/comment is still one statement
    let one = query(&adapter, &id, "SELECT a FROM t ORDER BY a; -- done\n");
    assert!(one.success, "{one:?}");
    assert_eq!(one.rows.len(), 2);

    // A ';' inside a string literal is not a script separator
    let literal = query(&adapter, &id, "SELECT ';' AS semi");
    assert_eq!(literal.rows, vec![vec![text(";")]]);

    let update = query(&adapter, &id, "UPDATE t SET a = a + 10 WHERE a = 1");
    assert_eq!(update.row_count, 1);
    assert_eq!(update.message.as_deref(), Some("1 row affected."));
    let update = query(&adapter, &id, "UPDATE t SET a = 0");
    assert_eq!(update.message.as_deref(), Some("2 rows affected."));

    let one_row = query(&adapter, &id, "SELECT 1");
    assert_eq!(one_row.message.as_deref(), Some("Returned 1 row."));
    let none = query(&adapter, &id, "SELECT a FROM t WHERE 0");
    assert!(none.success);
    assert_eq!(none.columns, vec!["a"]);
    assert!(none.rows.is_empty());
}

#[test]
fn typed_values_and_blob_normalisation() {
    let adapter = SqliteAdapter::unrestricted();
    let id = "sqlite::memory:";
    let result = query(
        &adapter,
        id,
        "SELECT 1 AS i, 2.5 AS r, 'x' AS t, NULL AS n, X'DEADBEEF' AS b, zeroblob(40) AS big",
    );
    assert!(result.success, "{result:?}");
    assert_eq!(result.rows[0][0], SqlValue::Integer(1));
    assert_eq!(result.rows[0][1], SqlValue::Real(2.5));
    assert_eq!(result.rows[0][2], text("x"));
    assert_eq!(result.rows[0][3], SqlValue::Null);
    assert_eq!(result.rows[0][4], text("0xdeadbeef"));
    assert_eq!(result.rows[0][5], text(&format!("0x{}...", "00".repeat(24))));
    let json = serde_json::to_string(&result.rows[0][..4]).expect("json");
    assert_eq!(json, r#"[1,2.5,"x",null]"#);
}

#[test]
fn row_cap_and_paging() {
    let adapter = SqliteAdapter::unrestricted();
    let id = "sqlite::memory:";
    let seq = "WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < 250) SELECT n FROM s";

    let capped = adapter.execute_query(&SqlQueryRequest {
        connection_id: id.into(),
        sql: seq.into(),
        limit: Some(100),
        ..SqlQueryRequest::default()
    });
    assert!(capped.success, "{capped:?}");
    assert_eq!(capped.rows.len(), 100);
    assert!(capped.truncated);
    assert!(capped.message.as_deref().unwrap_or_default().contains("capped at 100"));

    let page = adapter.execute_query(&SqlQueryRequest {
        connection_id: id.into(),
        sql: seq.into(),
        limit: Some(100),
        offset: Some(200),
        ..SqlQueryRequest::default()
    });
    assert_eq!(page.rows.len(), 50);
    assert!(!page.truncated);
    assert_eq!(page.rows[0][0], SqlValue::Integer(201));
    assert_eq!(page.rows[49][0], SqlValue::Integer(250));

    let exact = adapter.execute_query(&SqlQueryRequest {
        connection_id: id.into(),
        sql: seq.into(),
        limit: Some(250),
        ..SqlQueryRequest::default()
    });
    assert!(!exact.truncated);

    let default_cap = query(&adapter, id, seq);
    assert_eq!(default_cap.rows.len(), 250); // below DEFAULT_ROW_LIMIT
    const { assert!(DEFAULT_ROW_LIMIT <= MAX_ROW_LIMIT) };
}

const SLOW_SQL: &str = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 2000000000) SELECT count(*) FROM c";

#[test]
fn statement_timeout_aborts_long_queries_and_connection_stays_usable() {
    let adapter = SqliteAdapter::unrestricted();
    let id = "sqlite::memory:";
    let started = Instant::now();
    let result = adapter.execute_query(&SqlQueryRequest {
        connection_id: id.into(),
        sql: SLOW_SQL.into(),
        timeout_ms: Some(150),
        ..SqlQueryRequest::default()
    });
    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("Query timed out after 150 ms"));
    assert!(started.elapsed() < Duration::from_secs(10));

    // handler removed: later queries are unaffected
    let ok = query(&adapter, id, "SELECT 42");
    assert!(ok.success, "{ok:?}");
}

#[test]
fn interrupt_cancels_a_running_statement_from_another_thread() {
    let adapter = Arc::new(SqliteAdapter::unrestricted());
    let id = "sqlite::memory:";
    adapter.open(id).expect("open");

    let worker = {
        let adapter = Arc::clone(&adapter);
        std::thread::spawn(move || {
            adapter.execute_query(&SqlQueryRequest {
                connection_id: id.into(),
                sql: SLOW_SQL.into(),
                ..SqlQueryRequest::default()
            })
        })
    };
    // Keep interrupting until the worker finishes (the statement may not have started yet).
    let started = Instant::now();
    while !worker.is_finished() && started.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(50));
        adapter.interrupt(id);
    }
    let result = worker.join().expect("join");
    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("Query was cancelled"));
    assert!(query(&adapter, id, "SELECT 1").success);
}

#[test]
fn open_close_and_connection_reuse() {
    let (_dir, root, adapter) = workspace();
    let id = format!("sqlite:{}", root.join("c.db").display());
    assert_eq!(adapter.open_count(), 0);
    adapter.open(&id).expect("open");
    adapter.open(&id).expect("open again");
    assert_eq!(adapter.open_count(), 1);
    assert!(root.join("c.db").exists());
    // bare path form (no `sqlite:` prefix) maps to the same connection
    adapter.open(&root.join("c.db").display().to_string()).expect("open raw");
    assert_eq!(adapter.open_count(), 1);
    assert!(adapter.close(&id));
    assert!(!adapter.close(&id));
    assert_eq!(adapter.open_count(), 0);
    assert!(adapter.open(&format!("sqlite:{}", root.join("no/such/dir/x.db").display())).is_err());
}

#[test]
fn views_composite_keys_rowid_and_without_rowid_tables() {
    let adapter = SqliteAdapter::unrestricted();
    let id = "sqlite::memory:";
    let setup = query(
        &adapter,
        id,
        "CREATE TABLE pairs (a INTEGER, b TEXT, v TEXT, PRIMARY KEY (b, a));
         CREATE TABLE plain (x TEXT, y INTEGER);
         CREATE TABLE nr (k TEXT PRIMARY KEY, v) WITHOUT ROWID;
         CREATE VIEW pv AS SELECT a, b FROM pairs;
         INSERT INTO pairs VALUES (1, 'x', 'one'), (2, 'y', 'two');
         INSERT INTO plain VALUES ('p', 1), ('q', 2);
         INSERT INTO nr VALUES ('k1', 'v1');",
    );
    assert!(setup.success, "{setup:?}");

    let schema = adapter.get_schema(id);
    let table_names: Vec<_> = schema.tables.iter().map(|t| (t.name.as_str(), t.table_type)).collect();
    assert_eq!(
        table_names,
        vec![
            ("nr", TableType::Table),
            ("pairs", TableType::Table),
            ("plain", TableType::Table),
            ("pv", TableType::View)
        ]
    );
    // primary key columns first, in pk order (b is pk #1, a is pk #2)
    let pair_cols: Vec<_> = schema.columns_by_table["main.pairs"].iter().map(|c| c.name.as_str()).collect();
    assert_eq!(pair_cols, ["b", "a", "v"]);

    let request = |table: &str| SqlBrowseTableRequest {
        connection_id: id.into(),
        table_id: format!("main.{table}"),
        ..SqlBrowseTableRequest::default()
    };

    let pairs = adapter.get_table_rows(&request("pairs"));
    assert!(pairs.writable);
    assert_eq!(pairs.rows[0].values, vec![text("x"), SqlValue::Integer(1), text("one")]);
    let locator = pairs.rows[0].locator.clone().expect("locator");
    assert_eq!(locator.values.len(), 2);

    let plain = adapter.get_table_rows(&request("plain"));
    assert!(plain.writable);
    assert_eq!(plain.rows[0].locator.as_ref().map(|l| l.kind), Some(RowLocatorKind::Rowid));
    assert_eq!(plain.rows[0].values, vec![text("p"), SqlValue::Integer(1)]);
    let rowid_locator = plain.rows[1].locator.clone().expect("locator");
    let update = adapter.update_row(&SqlUpdateRowRequest {
        connection_id: id.into(),
        table_id: "main.plain".into(),
        locator: rowid_locator.clone(),
        values: BTreeMap::from([("y".to_string(), SqlValue::Integer(99)), ("bogus".to_string(), text("ignored"))]),
    });
    assert!(update.success, "{update:?}");
    assert_eq!(query(&adapter, id, "SELECT y FROM plain WHERE x = 'q'").rows[0][0], SqlValue::Integer(99));
    let delete = adapter.delete_row(&SqlDeleteRowRequest {
        connection_id: id.into(),
        table_id: "main.plain".into(),
        locator: rowid_locator,
    });
    assert!(delete.success);
    assert_eq!(adapter.insert_row(&SqlInsertRowRequest {
        connection_id: id.into(),
        table_id: "main.plain".into(),
        values: BTreeMap::new(),
    }).row_count, 1);

    let nr = adapter.get_table_rows(&request("nr"));
    assert!(nr.writable); // has a primary key
    let view = adapter.get_table_rows(&request("pv"));
    assert!(!view.writable);
    assert_eq!(view.message.as_deref(), Some("Views are shown read-only."));
    assert!(view.rows[0].locator.is_none());
    let view_update = adapter.update_row(&SqlUpdateRowRequest {
        connection_id: id.into(),
        table_id: "main.pv".into(),
        locator: locator.clone(),
        values: BTreeMap::from([("a".to_string(), SqlValue::Integer(5))]),
    });
    assert!(!view_update.success);
    assert_eq!(view_update.error.as_deref(), Some("Views are read-only."));

    let missing = adapter.get_table_rows(&request("ghost"));
    assert_eq!(missing.error.as_deref(), Some("The selected table could not be found in this SQLite file."));

    // locator validation
    let bad = adapter.delete_row(&SqlDeleteRowRequest {
        connection_id: id.into(),
        table_id: "main.pairs".into(),
        locator: SqlRowLocator { kind: RowLocatorKind::PrimaryKey, values: BTreeMap::new() },
    });
    assert_eq!(bad.error.as_deref(), Some("Primary key locator must include at least one key column."));
    let bad = adapter.delete_row(&SqlDeleteRowRequest {
        connection_id: id.into(),
        table_id: "main.plain".into(),
        locator: SqlRowLocator { kind: RowLocatorKind::Rowid, values: BTreeMap::new() },
    });
    assert_eq!(bad.error.as_deref(), Some("Row locator is missing the SQLite rowid value."));
    let none = adapter.update_row(&SqlUpdateRowRequest {
        connection_id: id.into(),
        table_id: "main.plain".into(),
        locator: SqlRowLocator { kind: RowLocatorKind::Rowid, values: BTreeMap::from([("rowid".to_string(), SqlValue::Integer(1))]) },
        values: BTreeMap::from([("bogus".to_string(), text("x"))]),
    });
    assert_eq!(none.error.as_deref(), Some("Choose at least one editable column before saving the row."));
}

#[test]
fn browse_sorting_paging_filters_validation_and_like_escaping() {
    let adapter = SqliteAdapter::unrestricted();
    let id = "sqlite::memory:";
    seed(&adapter, id);
    query(&adapter, id, "INSERT INTO users (name, age) VALUES ('100%_real', 1), ('Back\\slash', 2)");

    let sorted = adapter.get_table_rows(&SqlBrowseTableRequest {
        sort_column: Some("age".into()),
        sort_direction: Some(SortDirection::Desc),
        limit: Some(2),
        offset: Some(1),
        ..browse(id, None)
    });
    assert_eq!(sorted.total_rows, 5);
    assert_eq!(names(&sorted), ["Mickey", "Mini"]);
    assert_eq!(sorted.limit, Some(2));
    assert_eq!(sorted.offset, Some(1));

    // LIKE wildcards in the user's text are literal
    let pct = adapter.get_table_rows(&browse(id, Some(vec![filter(Some("name"), SqlFilterOperator::Contains, Some("%_"))])));
    assert_eq!(names(&pct), ["100%_real"]);
    let slash = adapter.get_table_rows(&browse(id, Some(vec![filter(Some("name"), SqlFilterOperator::EndsWith, Some("\\slash"))])));
    assert_eq!(names(&slash), ["Back\\slash"]);

    let eq = adapter.get_table_rows(&browse(id, Some(vec![filter(Some("age"), SqlFilterOperator::Equals, Some("14"))])));
    assert_eq!(names(&eq), ["Mini"]);
    let ne = adapter.get_table_rows(&browse(id, Some(vec![
        filter(Some("age"), SqlFilterOperator::NotEquals, Some("14")),
        filter(Some("age"), SqlFilterOperator::LessThan, Some("40")),
        filter(Some("nickname"), SqlFilterOperator::IsNotNull, None),
    ])));
    assert!(names(&ne).is_empty());
    let le = adapter.get_table_rows(&browse(id, Some(vec![filter(Some("age"), SqlFilterOperator::LessThanOrEqual, Some("14"))])));
    assert_eq!(names(&le), ["Mini", "100%_real", "Back\\slash"]);

    // blank values are ignored
    let blank = adapter.get_table_rows(&browse(id, Some(vec![filter(Some("age"), SqlFilterOperator::Equals, Some("  "))])));
    assert_eq!(blank.rows.len(), 5);

    // validation errors
    let err = |filters| adapter.get_table_rows(&browse(id, Some(filters))).error;
    assert_eq!(err(vec![filter(None, SqlFilterOperator::IsNull, None)]).as_deref(), Some("Choose a column before using null filters."));
    assert_eq!(err(vec![filter(Some("zzz"), SqlFilterOperator::Equals, Some("1"))]).as_deref(), Some("The selected filter column is no longer available."));
    assert_eq!(err(vec![filter(None, SqlFilterOperator::Equals, Some("1"))]).as_deref(), Some("Choose a column before using this filter operator."));
    let bad_sort = adapter.get_table_rows(&SqlBrowseTableRequest { sort_column: Some("nope".into()), ..browse(id, None) });
    assert_eq!(bad_sort.error.as_deref(), Some("The selected sort column is no longer available."));
}

#[test]
fn list_indexes_reports_columns_in_order() {
    let adapter = SqliteAdapter::unrestricted();
    let id = "sqlite::memory:";
    seed(&adapter, id);
    query(&adapter, id, "CREATE UNIQUE INDEX idx_name_age ON users (name, age); CREATE INDEX idx_nick ON users (nickname) WHERE nickname IS NOT NULL;");
    let mut indexes = adapter.list_indexes(id, "main.users").expect("indexes");
    indexes.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(indexes.len(), 2);
    assert_eq!(indexes[0].name, "idx_name_age");
    assert!(indexes[0].unique && !indexes[0].partial);
    assert_eq!(indexes[0].columns, ["name", "age"]);
    assert_eq!(indexes[0].origin, "c");
    assert!(!indexes[1].unique && indexes[1].partial);
    assert!(adapter.list_indexes(id, "main.ghost").is_err());
}

#[test]
fn foreign_keys_are_enforced_like_better_sqlite3() {
    let adapter = SqliteAdapter::unrestricted();
    let id = "sqlite::memory:";
    let setup = query(&adapter, id, "CREATE TABLE p (id INTEGER PRIMARY KEY); CREATE TABLE c (pid INTEGER REFERENCES p(id));");
    assert!(setup.success);
    let bad = query(&adapter, id, "INSERT INTO c VALUES (7)");
    assert!(!bad.success);
    assert!(bad.error.as_deref().unwrap_or_default().contains("FOREIGN KEY"), "{bad:?}");
}

#[test]
fn sql_service_routes_by_prefix_and_reports_status() {
    let mut service = SqlService::new();
    let status = service.get_status();
    assert_eq!(status.phase, ServicePhase::Scaffold);
    assert!(!status.features.query_runner);
    assert!(status.features.editor);
    assert_eq!(status.supported_dialects, [SqlDialect::Sqlite, SqlDialect::Postgres, SqlDialect::Mysql]);
    assert!(status.message.starts_with("SQL service scaffolded."));

    let unconfigured = service.execute_query(&SqlQueryRequest { connection_id: "sqlite::memory:".into(), sql: "SELECT 1".into(), ..Default::default() });
    assert!(!unconfigured.success);
    assert_eq!(unconfigured.message.as_deref(), Some("No SQL adapters are registered yet."));
    assert!(service.get_schema("x").error.is_some());
    assert_eq!(service.get_table_rows(&browse("x", None)).error.as_deref(), Some("SQL table browser is not configured yet."));

    service.register_adapter(Arc::new(SqliteAdapter::unrestricted()));
    let status = service.get_status();
    assert_eq!(status.phase, ServicePhase::Active);
    assert_eq!(status.registered_dialects, [SqlDialect::Sqlite]);
    assert!(status.features.query_runner && status.features.schema_browser && status.features.row_editing);

    // bare ids default to sqlite; postgres:/mysql: have no adapter
    let ok = service.execute_query(&SqlQueryRequest { connection_id: "sqlite::memory:".into(), sql: "SELECT 1".into(), ..Default::default() });
    assert!(ok.success);
    let pg = service.execute_query(&SqlQueryRequest { connection_id: "postgres://h/db".into(), sql: "SELECT 1".into(), ..Default::default() });
    assert!(!pg.success);
    service.dispose();
}

#[test]
fn export_csv_and_xlsx_then_import_back() {
    let (dir, root, adapter) = workspace();
    let id = format!("sqlite:{}", root.join("e.db").display());
    seed(&adapter, &id);
    let result = query(&adapter, &id, "SELECT id, name, nickname FROM users ORDER BY id");

    let csv_path = dir.path().join("out.csv");
    export_csv(
        &csv_path,
        &result.columns,
        &[ExportColumn::new("name", "Name"), ExportColumn::same("nickname")],
        &result.rows,
    )
    .expect("csv");
    let csv_text = std::fs::read_to_string(&csv_path).expect("read");
    assert_eq!(csv_text, "\"Name\",\"nickname\"\r\n\"Mickey\",\"\"\r\n\"Mini\",\"min\"\r\n\"Donald\",\"duck\"");

    // CSV import round trip (with BOM, embedded quotes/newlines)
    let tricky = dir.path().join("tricky.csv");
    std::fs::write(&tricky, "\u{feff}id,note\r\n1,\"he said \"\"hi\"\"\nsecond line\"\r\n2,\r\n").expect("write");
    let imported = import_csv(&tricky, &ImportOptions::default()).expect("import csv");
    assert_eq!(imported.columns, ["id", "note"]);
    assert_eq!(imported.rows[0], vec![text("1"), text("he said \"hi\"\nsecond line")]);
    let inserted = adapter
        .import_table(&id, "notes", &imported, &ImportOptions::default())
        .expect("insert");
    assert_eq!(inserted, 2);
    let rows = query(&adapter, &id, "SELECT id, note FROM notes ORDER BY id");
    assert_eq!(rows.rows[1], vec![text("2"), SqlValue::Null]); // empty -> NULL

    // XLSX export / import
    let xlsx_path = dir.path().join("out.xlsx");
    export_result_xlsx(&xlsx_path, &result).expect("xlsx");
    assert_eq!(xlsx_sheet_names(&xlsx_path).expect("sheets"), ["Export"]);
    let from_xlsx = import_file(&xlsx_path, &ImportOptions::default()).expect("import xlsx");
    assert_eq!(from_xlsx.columns, ["id", "name", "nickname"]);
    assert_eq!(from_xlsx.rows.len(), 3);
    assert_eq!(from_xlsx.rows[0], vec![text("1"), text("Mickey"), SqlValue::Null]);
    assert_eq!(from_xlsx.rows[2], vec![text("3"), text("Donald"), text("duck")]);
    let inserted = adapter
        .import_table(&id, "users_copy", &from_xlsx, &ImportOptions::default())
        .expect("insert xlsx");
    assert_eq!(inserted, 3);
    let copy = query(&adapter, &id, "SELECT name FROM users_copy ORDER BY name");
    assert_eq!(copy.rows.len(), 3);

    // named sheet that does not exist -> error; unsupported extension -> error
    let missing_sheet = import_xlsx(&xlsx_path, &ImportOptions { sheet: Some("Nope".into()), ..Default::default() });
    assert!(missing_sheet.is_err());
    assert!(matches!(import_file(&dir.path().join("x.txt"), &ImportOptions::default()), Err(DbError::UnsupportedFile(_))));

    // table name validation
    assert!(adapter.import_table(&id, " ", &imported, &ImportOptions::default()).is_err());
}

#[test]
fn xlsx_numeric_cells_are_typed_on_import() {
    use rust_xlsxwriter::Workbook;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("num.xlsx");
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    for (col, header) in ["n", "f", "s"].iter().enumerate() {
        sheet.write_string(0, col as u16, *header).expect("header");
    }
    sheet.write_number(1, 0, 5.0).expect("n");
    sheet.write_number(1, 1, 2.5).expect("f");
    sheet.write_string(1, 2, "txt").expect("s");
    book.save(&path).expect("save");

    let table = import_xlsx(&path, &ImportOptions::default()).expect("import");
    assert_eq!(table.columns, ["n", "f", "s"]);
    assert_eq!(table.rows[0], vec![SqlValue::Integer(5), SqlValue::Real(2.5), text("txt")]);
}

// ---- review fixes -------------------------------------------------------------------------

fn pk_locator(pairs: &[(&str, SqlValue)]) -> SqlRowLocator {
    SqlRowLocator {
        kind: RowLocatorKind::PrimaryKey,
        values: pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect(),
    }
}

fn count(adapter: &SqliteAdapter, id: &str, table: &str) -> SqlValue {
    query(adapter, id, &format!("SELECT count(*) FROM {table}")).rows[0][0].clone()
}

#[test]
fn locator_must_match_the_primary_key_exactly() {
    let adapter = SqliteAdapter::unrestricted();
    let id = "sqlite::memory:";
    seed(&adapter, id);
    query(&adapter, id, "CREATE TABLE pairs (a INTEGER, b TEXT, v TEXT, PRIMARY KEY (a, b)); INSERT INTO pairs VALUES (1,'x','one'),(2,'y','two');");

    let delete = |table: &str, locator| {
        adapter.delete_row(&SqlDeleteRowRequest { connection_id: id.into(), table_id: format!("main.{table}"), locator })
    };
    let update = |table: &str, locator| {
        adapter.update_row(&SqlUpdateRowRequest {
            connection_id: id.into(),
            table_id: format!("main.{table}"),
            locator,
            values: BTreeMap::from([("name".to_string(), text("pwned"))]),
        })
    };

    // The probe from the review: a bogus key column whose value equals its own name.
    let ghost = pk_locator(&[("ghost", text("ghost"))]);
    let r = delete("users", ghost.clone());
    assert!(!r.success, "{r:?}");
    assert_eq!(count(&adapter, id, "users"), SqlValue::Integer(3));
    assert!(!update("users", ghost).success);

    // pk plus an extra column, a wrong column, and a partial composite key are all rejected
    for bad in [
        pk_locator(&[("id", SqlValue::Integer(1)), ("name", text("Mickey"))]),
        pk_locator(&[("name", text("Mickey"))]),
        pk_locator(&[("id", SqlValue::Integer(1)), ("id2", SqlValue::Integer(1))]),
    ] {
        let r = delete("users", bad.clone());
        assert_eq!(r.error.as_deref(), Some("Primary key locator must match the table's primary key columns exactly."), "{r:?}");
        assert!(!update("users", bad).success);
    }
    let partial = delete("pairs", pk_locator(&[("a", SqlValue::Integer(1))]));
    assert!(!partial.success);
    assert_eq!(count(&adapter, id, "pairs"), SqlValue::Integer(2));

    // a pk locator on a table without a primary key is rejected, as is a rowid locator with extras
    query(&adapter, id, "CREATE TABLE plain (x TEXT); INSERT INTO plain VALUES ('a'),('b');");
    assert!(!delete("plain", pk_locator(&[("x", text("a"))])).success);
    let extra = SqlRowLocator {
        kind: RowLocatorKind::Rowid,
        values: BTreeMap::from([("rowid".to_string(), SqlValue::Integer(1)), ("x".to_string(), text("a"))]),
    };
    assert!(!delete("plain", extra).success);
    assert_eq!(count(&adapter, id, "plain"), SqlValue::Integer(2));

    // exact composite key works
    let ok = delete("pairs", pk_locator(&[("a", SqlValue::Integer(1)), ("b", text("x"))]));
    assert!(ok.success, "{ok:?}");
    assert_eq!(count(&adapter, id, "pairs"), SqlValue::Integer(1));
}

#[test]
fn double_quoted_string_literals_are_disabled() {
    let adapter = SqliteAdapter::unrestricted();
    let id = "sqlite::memory:";
    seed(&adapter, id);
    let r = query(&adapter, id, "SELECT * FROM users WHERE \"ghost\" = 'ghost'");
    assert!(!r.success);
    assert!(r.error.as_deref().unwrap_or_default().contains("no such column"), "{r:?}");
    let r = query(&adapter, id, "DELETE FROM users WHERE \"ghost\" = 'ghost'");
    assert!(!r.success);
    assert_eq!(count(&adapter, id, "users"), SqlValue::Integer(3));
    // DDL: a double-quoted unknown column in an index must not become a string literal
    assert!(!query(&adapter, id, "CREATE INDEX bad_idx ON users (\"ghost\")").success);
    // real string literals and quoted identifiers still work
    assert!(query(&adapter, id, "SELECT \"name\" FROM users WHERE name = 'Mini'").success);
}

#[cfg(unix)]
#[test]
fn symlinked_parent_cannot_escape_the_workspace() {
    let (_dir, root, adapter) = workspace();
    let outside = tempfile::tempdir().expect("tempdir");
    std::os::unix::fs::symlink(outside.path(), root.join("link")).expect("symlink");

    let id = format!("sqlite:{}", root.join("link/x.db").display());
    let r = query(&adapter, &id, "SELECT 1");
    assert_eq!(r.error.as_deref(), Some("SQLite database path must stay inside the open workspace"));
    assert!(!outside.path().join("x.db").exists());

    // an existing file reached through the symlink is also refused
    let existing = outside.path().join("y.db");
    std::fs::write(&existing, b"").expect("write");
    assert!(!query(&adapter, &format!("sqlite:{}", root.join("link/y.db").display()), "SELECT 1").success);
    assert_eq!(adapter.open_count(), 0);

    // a workspace that is itself opened through a symlink still works
    let real = tempfile::tempdir().expect("tempdir");
    let via = root.join("ws-link");
    std::os::unix::fs::symlink(real.path(), &via).expect("symlink");
    let other = SqliteAdapter::new();
    other.set_workspace_root(Some(&via));
    assert!(query(&other, &format!("sqlite:{}", via.join("ok.db").display()), "SELECT 1").success);
}

#[test]
fn close_and_interrupt_work_after_the_workspace_changes() {
    let (_dir, root, adapter) = workspace();
    let id = format!("sqlite:{}", root.join("a.db").display());
    adapter.open(&id).expect("open");

    // moving to another workspace closes connections that fall outside it
    let other = tempfile::tempdir().expect("tempdir");
    adapter.set_workspace_root(Some(other.path()));
    assert_eq!(adapter.open_count(), 0);

    // connections inside the new root survive
    let inner = format!("sqlite:{}", other.path().join("b.db").display());
    adapter.open(&inner).expect("open inner");
    adapter.set_workspace_root(Some(other.path()));
    assert_eq!(adapter.open_count(), 1);

    // close/interrupt do not need the policy to accept the path
    adapter.set_workspace_root(None);
    assert_eq!(adapter.open_count(), 0);
    assert!(!adapter.close(&inner));
    adapter.interrupt(&inner); // no panic

    // `close` finds a connection even when the policy would now reject it
    let unrestricted_like = SqliteAdapter::unrestricted();
    let path_id = format!("sqlite:{}", root.join("c.db").display());
    unrestricted_like.open(&path_id).expect("open");
    assert!(unrestricted_like.close(&path_id));
}

#[test]
fn timeout_clock_starts_after_the_connection_lock_is_acquired() {
    let adapter = Arc::new(SqliteAdapter::unrestricted());
    let id = "sqlite::memory:";
    adapter.open(id).expect("open");

    let blocker = {
        let adapter = Arc::clone(&adapter);
        std::thread::spawn(move || {
            adapter.execute_query(&SqlQueryRequest { connection_id: id.into(), sql: SLOW_SQL.into(), ..Default::default() })
        })
    };
    std::thread::sleep(Duration::from_millis(300)); // let the blocker take the lock

    let waiter = {
        let adapter = Arc::clone(&adapter);
        std::thread::spawn(move || {
            adapter.execute_query(&SqlQueryRequest {
                connection_id: id.into(),
                sql: "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 3000000) SELECT count(*) FROM c".into(),
                timeout_ms: Some(1500),
                ..Default::default()
            })
        })
    };
    // keep the waiter queued for longer than its own timeout, then release the lock
    std::thread::sleep(Duration::from_millis(2000));
    adapter.interrupt(id);
    assert_eq!(blocker.join().expect("join").error.as_deref(), Some("Query was cancelled"));
    let result = waiter.join().expect("join");
    assert!(result.success, "queue time must not count against the timeout: {result:?}");
}

#[test]
fn raw_blob_locators_round_trip_through_json() {
    let adapter = SqliteAdapter::unrestricted();
    let id = "sqlite::memory:";
    query(&adapter, id, "CREATE TABLE b (k BLOB PRIMARY KEY, v TEXT); INSERT INTO b VALUES (X'00FF10', 'one'), (X'0102', 'two');");
    let rows = adapter.get_table_rows(&SqlBrowseTableRequest { connection_id: id.into(), table_id: "main.b".into(), ..Default::default() });
    assert!(rows.writable);
    // display value is the preview, locator carries the raw bytes
    assert_eq!(rows.rows[0].values[0], text("0x00ff10"));
    let locator = rows.rows[0].locator.clone().expect("locator");
    assert_eq!(locator.values["k"], SqlValue::Blob(vec![0, 255, 16]));

    // through JSON and back, then used to delete exactly that row
    let json = serde_json::to_string(&locator).expect("json");
    assert!(json.contains("$blob"), "{json}");
    let back: SqlRowLocator = serde_json::from_str(&json).expect("back");
    assert_eq!(back, locator);
    let r = adapter.delete_row(&SqlDeleteRowRequest { connection_id: id.into(), table_id: "main.b".into(), locator: back });
    assert!(r.success && r.row_count == 1, "{r:?}");
    assert_eq!(count(&adapter, id, "b"), SqlValue::Integer(1));
}

#[test]
fn xlsx_date_cells_import_as_iso_8601_text() {
    use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("dates.xlsx");
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    sheet.write_string(0, 0, "when").expect("h");
    sheet.write_string(0, 1, "day").expect("h");
    let stamp = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
    let day = Format::new().set_num_format("yyyy-mm-dd");
    sheet
        .write_datetime_with_format(1, 0, ExcelDateTime::from_ymd(2024, 2, 29).expect("d").and_hms(13, 5, 9).expect("t"), &stamp)
        .expect("w");
    sheet
        .write_datetime_with_format(1, 1, ExcelDateTime::from_ymd(2024, 2, 29).expect("d"), &day)
        .expect("w");
    book.save(&path).expect("save");

    let table = import_xlsx(&path, &ImportOptions::default()).expect("import");
    assert_eq!(table.rows[0], vec![text("2024-02-29T13:05:09"), text("2024-02-29")]);
}
