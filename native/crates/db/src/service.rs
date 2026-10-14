//! Port of `src/main/services/sql.ts`: dialect registry routing by connection id prefix.

use std::collections::HashMap;
use std::sync::Arc;

use crate::types::*;

/// Port of the `SqlAdapter` interface (synchronous).
pub trait SqlAdapter: Send + Sync {
    fn dialect(&self) -> SqlDialect;
    fn execute_query(&self, request: &SqlQueryRequest) -> SqlQueryResult;
    fn get_schema(&self, connection_id: &str) -> SqlSchemaSummary;
    fn get_table_rows(&self, request: &SqlBrowseTableRequest) -> SqlTableRowsResult;
    fn update_row(&self, request: &SqlUpdateRowRequest) -> SqlMutationResult;
    fn insert_row(&self, request: &SqlInsertRowRequest) -> SqlMutationResult;
    fn delete_row(&self, request: &SqlDeleteRowRequest) -> SqlMutationResult;
    /// Close all connections.
    fn dispose(&self) {}
}

const SUPPORTED_DIALECTS: [SqlDialect; 3] = [SqlDialect::Sqlite, SqlDialect::Postgres, SqlDialect::Mysql];

#[derive(Default)]
pub struct SqlService {
    adapters: HashMap<SqlDialect, Arc<dyn SqlAdapter>>,
}

fn resolve_dialect(connection_id: &str) -> SqlDialect {
    if connection_id.starts_with("sqlite:") {
        SqlDialect::Sqlite
    } else if connection_id.starts_with("postgres:") {
        SqlDialect::Postgres
    } else if connection_id.starts_with("mysql:") {
        SqlDialect::Mysql
    } else {
        SqlDialect::Sqlite
    }
}

impl SqlService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_adapter(&mut self, adapter: Arc<dyn SqlAdapter>) {
        self.adapters.insert(adapter.dialect(), adapter);
    }

    fn adapter_for(&self, connection_id: &str) -> Option<&Arc<dyn SqlAdapter>> {
        self.adapters.get(&resolve_dialect(connection_id))
    }

    pub fn get_status(&self) -> SqlServiceStatus {
        let active = !self.adapters.is_empty();
        let registered: Vec<SqlDialect> = SUPPORTED_DIALECTS
            .iter()
            .copied()
            .filter(|d| self.adapters.contains_key(d))
            .collect();
        SqlServiceStatus {
            phase: if active { ServicePhase::Active } else { ServicePhase::Scaffold },
            supported_dialects: SUPPORTED_DIALECTS.to_vec(),
            registered_dialects: registered,
            connections: Vec::new(),
            features: SqlFeatureFlags {
                editor: true,
                query_runner: active,
                schema_browser: active,
                row_editing: active,
                saved_queries: false,
                explain_plan: false,
            },
            message: if active {
                "SQL adapters are registered and ready for query execution.".into()
            } else {
                "SQL service scaffolded. Add a database adapter to enable query execution and schema browsing."
                    .into()
            },
        }
    }

    pub fn execute_query(&self, request: &SqlQueryRequest) -> SqlQueryResult {
        match self.adapter_for(&request.connection_id) {
            Some(adapter) => adapter.execute_query(request),
            None => SqlQueryResult {
                connection_id: request.connection_id.clone(),
                error: Some(
                    "SQL query runner is not configured yet. The Phase 2 SQLite adapter will plug in here."
                        .into(),
                ),
                message: Some("No SQL adapters are registered yet.".into()),
                ..SqlQueryResult::default()
            },
        }
    }

    pub fn get_schema(&self, connection_id: &str) -> SqlSchemaSummary {
        match self.adapter_for(connection_id) {
            Some(adapter) => adapter.get_schema(connection_id),
            None => SqlSchemaSummary {
                connection_id: connection_id.to_string(),
                error: Some(
                    "SQL schema browser is not configured yet. The Phase 3 data browser will plug in here."
                        .into(),
                ),
                ..SqlSchemaSummary::default()
            },
        }
    }

    pub fn get_table_rows(&self, request: &SqlBrowseTableRequest) -> SqlTableRowsResult {
        match self.adapter_for(&request.connection_id) {
            Some(adapter) => adapter.get_table_rows(request),
            None => SqlTableRowsResult {
                connection_id: request.connection_id.clone(),
                table_id: request.table_id.clone(),
                error: Some("SQL table browser is not configured yet.".into()),
                ..SqlTableRowsResult::default()
            },
        }
    }

    pub fn update_row(&self, request: &SqlUpdateRowRequest) -> SqlMutationResult {
        match self.adapter_for(&request.connection_id) {
            Some(adapter) => adapter.update_row(request),
            None => unconfigured(&request.connection_id, &request.table_id, "SQL row editing is not configured yet."),
        }
    }

    pub fn insert_row(&self, request: &SqlInsertRowRequest) -> SqlMutationResult {
        match self.adapter_for(&request.connection_id) {
            Some(adapter) => adapter.insert_row(request),
            None => unconfigured(&request.connection_id, &request.table_id, "SQL row inserts are not configured yet."),
        }
    }

    pub fn delete_row(&self, request: &SqlDeleteRowRequest) -> SqlMutationResult {
        match self.adapter_for(&request.connection_id) {
            Some(adapter) => adapter.delete_row(request),
            None => unconfigured(&request.connection_id, &request.table_id, "SQL row deletes are not configured yet."),
        }
    }

    pub fn dispose(&self) {
        for adapter in self.adapters.values() {
            adapter.dispose();
        }
    }
}

fn unconfigured(connection_id: &str, table_id: &str, message: &str) -> SqlMutationResult {
    SqlMutationResult {
        connection_id: connection_id.to_string(),
        table_id: table_id.to_string(),
        error: Some(message.to_string()),
        ..SqlMutationResult::default()
    }
}
