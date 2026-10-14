//! Port of `src/shared/types/sql.ts`, with a typed cell value.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A SQLite cell value.
///
/// Query/browse results never contain [`SqlValue::Blob`] data in full: like the TypeScript
/// `normalizeCellValue`, blobs are rendered as `0x<hex>` (truncated after 48 hex digits) and
/// returned as [`SqlValue::Text`].  `Blob` exists for row locators / inputs.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

/// JSON key used to serialise raw blobs losslessly: `{"$blob": "<base64>"}`.
const BLOB_KEY: &str = "$blob";

/// Maximum number of hex digits shown for a blob preview (24 bytes).
const BLOB_PREVIEW_HEX_DIGITS: usize = 48;

pub(crate) fn blob_preview(bytes: &[u8]) -> String {
    use fmt::Write;
    let mut hex = String::with_capacity(bytes.len().min(BLOB_PREVIEW_HEX_DIGITS / 2 + 1) * 2 + 5);
    for byte in bytes.iter().take(BLOB_PREVIEW_HEX_DIGITS / 2) {
        let _ = write!(hex, "{byte:02x}");
    }
    if bytes.len() * 2 <= BLOB_PREVIEW_HEX_DIGITS {
        format!("0x{hex}")
    } else {
        format!("0x{hex}...")
    }
}

impl SqlValue {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(v) => Some(v),
            _ => None,
        }
    }

    /// Text used by exports (`stringifyExportValue`): null -> "", everything else `String(v)`.
    pub fn to_export_string(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Integer(v) => v.to_string(),
            Self::Real(v) => v.to_string(),
            Self::Text(v) => v.clone(),
            Self::Blob(v) => blob_preview(v),
        }
    }
}

impl fmt::Display for SqlValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_export_string())
    }
}

impl From<i64> for SqlValue {
    fn from(v: i64) -> Self {
        Self::Integer(v)
    }
}
impl From<f64> for SqlValue {
    fn from(v: f64) -> Self {
        Self::Real(v)
    }
}
impl From<&str> for SqlValue {
    fn from(v: &str) -> Self {
        Self::Text(v.to_string())
    }
}
impl From<String> for SqlValue {
    fn from(v: String) -> Self {
        Self::Text(v)
    }
}
impl<T: Into<SqlValue>> From<Option<T>> for SqlValue {
    fn from(v: Option<T>) -> Self {
        v.map_or(Self::Null, Into::into)
    }
}

impl Serialize for SqlValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Integer(v) => serializer.serialize_i64(*v),
            Self::Real(v) => serializer.serialize_f64(*v),
            Self::Text(v) => serializer.serialize_str(v),
            // Lossless: {"$blob": "<base64>"}.  The `0x…` preview is display-only
            // (see `blob_preview` / `to_export_string`).
            Self::Blob(v) => {
                use base64::Engine;
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(
                    BLOB_KEY,
                    &base64::engine::general_purpose::STANDARD.encode(v),
                )?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for SqlValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Integer(i64::from(b)),
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(i) => Self::Integer(i),
                None => Self::Real(n.as_f64().unwrap_or(f64::NAN)),
            },
            serde_json::Value::String(s) => Self::Text(s),
            serde_json::Value::Object(map) if map.len() == 1 && map.contains_key(BLOB_KEY) => {
                use base64::Engine;
                let encoded = map[BLOB_KEY].as_str().ok_or_else(|| {
                    serde::de::Error::custom("`$blob` must be a base64 string")
                })?;
                Self::Blob(
                    base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .map_err(serde::de::Error::custom)?,
                )
            }
            other => Self::Text(other.to_string()),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SqlDialect {
    Sqlite,
    Postgres,
    Mysql,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionStatus {
    Disconnected,
    Ready,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlConnectionProfile {
    pub id: String,
    pub name: String,
    pub dialect: SqlDialect,
    pub status: ConnectionStatus,
    pub database: Option<String>,
    pub host: Option<String>,
    pub read_only: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlFeatureFlags {
    pub editor: bool,
    pub query_runner: bool,
    pub schema_browser: bool,
    pub row_editing: bool,
    pub saved_queries: bool,
    pub explain_plan: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServicePhase {
    Scaffold,
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlServiceStatus {
    pub phase: ServicePhase,
    pub supported_dialects: Vec<SqlDialect>,
    pub registered_dialects: Vec<SqlDialect>,
    pub connections: Vec<SqlConnectionProfile>,
    pub features: SqlFeatureFlags,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlColumnMetadata {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TableType {
    Table,
    View,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlTableSummary {
    pub id: String,
    pub schema: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub table_type: TableType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlIndexInfo {
    pub name: String,
    pub unique: bool,
    /// `c` = CREATE INDEX, `u` = UNIQUE constraint, `pk` = PRIMARY KEY.
    pub origin: String,
    pub partial: bool,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlSchemaSummary {
    pub connection_id: String,
    pub tables: Vec<SqlTableSummary>,
    pub columns_by_table: BTreeMap<String, Vec<SqlColumnMetadata>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SqlQueryRequest {
    pub connection_id: String,
    pub sql: String,
    /// Row cap (default [`crate::DEFAULT_ROW_LIMIT`], clamped to [`crate::MAX_ROW_LIMIT`]).
    pub limit: Option<usize>,
    /// Rows to skip before collecting (paging).
    pub offset: Option<usize>,
    /// Abort the statement after this many milliseconds.
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlQueryResult {
    pub connection_id: String,
    pub success: bool,
    pub columns: Vec<String>,
    /// Row-major cells aligned with `columns`.
    pub rows: Vec<Vec<SqlValue>>,
    pub row_count: usize,
    pub duration_ms: u64,
    /// True when more rows were available than the cap allowed.
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RowLocatorKind {
    PrimaryKey,
    Rowid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlRowLocator {
    pub kind: RowLocatorKind,
    pub values: BTreeMap<String, SqlValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlTableRow {
    pub locator: Option<SqlRowLocator>,
    /// Cells aligned with [`SqlTableRowsResult::columns`].
    pub values: Vec<SqlValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SqlFilterOperator {
    Contains,
    StartsWith,
    EndsWith,
    Equals,
    NotEquals,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlBrowseFilter {
    pub column: Option<String>,
    pub operator: SqlFilterOperator,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SqlBrowseTableRequest {
    pub connection_id: String,
    pub table_id: String,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub sort_column: Option<String>,
    pub sort_direction: Option<SortDirection>,
    pub filters: Option<Vec<SqlBrowseFilter>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlTableRowsResult {
    pub connection_id: String,
    pub table_id: String,
    pub columns: Vec<SqlColumnMetadata>,
    pub rows: Vec<SqlTableRow>,
    pub writable: bool,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub total_rows: u64,
    pub sort_column: Option<String>,
    pub sort_direction: Option<SortDirection>,
    pub filters: Option<Vec<SqlBrowseFilter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlUpdateRowRequest {
    pub connection_id: String,
    pub table_id: String,
    pub locator: SqlRowLocator,
    pub values: BTreeMap<String, SqlValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlInsertRowRequest {
    pub connection_id: String,
    pub table_id: String,
    pub values: BTreeMap<String, SqlValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlDeleteRowRequest {
    pub connection_id: String,
    pub table_id: String,
    pub locator: SqlRowLocator,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlMutationResult {
    pub connection_id: String,
    pub table_id: String,
    pub success: bool,
    pub row_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_preview_truncates_like_typescript() {
        assert_eq!(blob_preview(&[0xde, 0xad, 0xbe, 0xef]), "0xdeadbeef");
        assert_eq!(blob_preview(&[0u8; 24]), format!("0x{}", "00".repeat(24)));
        assert_eq!(blob_preview(&[0u8; 25]), format!("0x{}...", "00".repeat(24)));
        assert_eq!(blob_preview(&[]), "0x");
    }

    #[test]
    fn value_json_round_trip() {
        let values = vec![
            SqlValue::Null,
            SqlValue::Integer(7),
            SqlValue::Real(1.5),
            SqlValue::Text("x".into()),
        ];
        let json = serde_json::to_string(&values).expect("serialize");
        assert_eq!(json, r#"[null,7,1.5,"x"]"#);
        let back: Vec<SqlValue> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, values);
        let blob = SqlValue::Blob(vec![0, 255, 16, 1]);
        let json = serde_json::to_string(&blob).expect("blob json");
        assert_eq!(json, r#"{"$blob":"AP8QAQ=="}"#);
        assert_eq!(serde_json::from_str::<SqlValue>(&json).expect("blob back"), blob);
        assert!(serde_json::from_str::<SqlValue>(r#"{"$blob":"!!"}"#).is_err());
        let bools: Vec<SqlValue> = serde_json::from_str("[true,false,[1]]").expect("bools");
        assert_eq!(bools, vec![SqlValue::Integer(1), SqlValue::Integer(0), SqlValue::Text("[1]".into())]);
    }
}
