//! vsm-db — port of `sqliteAdapter.ts`, `sql.ts`, `shared/types/sql.ts` and the XLSX/CSV
//! export (`DatabasePanel.tsx`) of the Electron app, on top of `rusqlite` (bundled SQLite).
//!
//! * [`SqlService`] / [`SqlAdapter`]: dialect registry that routes by connection id prefix.
//! * [`SqliteAdapter`]: connections, schema/indexes, query execution (paging, row cap,
//!   timeout, interrupt), table browsing with filters/sort, row insert/update/delete.
//! * [`export`]: CSV and XLSX writers.  [`import`]: CSV/XLSX readers + bulk insert.

mod error;
pub mod export;
pub mod import;
mod service;
mod sqlite;
mod types;

pub use error::{DbError, Result};
pub use export::{
    build_export_file_name, csv_string, csv_string_with, export_csv, export_csv_with,
    export_result_csv, export_result_xlsx, export_xlsx, CsvOptions, ExportColumn,
};
pub use import::{
    import_csv, import_file, import_xlsx, xlsx_sheet_names, ImportOptions, ImportedTable,
};
pub use service::{SqlAdapter, SqlService};
pub use sqlite::{SqliteAdapter, DEFAULT_ROW_LIMIT, MAX_ROW_LIMIT};
pub use types::*;
