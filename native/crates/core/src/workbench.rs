//! Workbench layout persistence (port of `src/renderer/utils/workbenchPersistence.ts`).
//!
//! Editor and terminal sessions are stored per workspace, UI preferences
//! globally. The TypeScript version used `localStorage` keys of the form
//! `vs-monitor-ide:workspace:<encoded path>:<section>`; here each section is a
//! JSON file below the store directory:
//!
//! ```text
//! <dir>/global/ui.json
//! <dir>/workspaces/<slug>-<hash>/editor.json
//! <dir>/workspaces/<slug>-<hash>/terminal.json
//! ```
//!
//! Workspace identity is the same normalised key as before (backslashes become
//! `/`, lower-cased), so `C:\Proj` and `c:/proj` share state.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::config::default_config_dir;
use crate::error::{CoreError, CoreResult};
use crate::fsutil;
use crate::types::Tab;

/// Maximum combined `content + savedContent` length (UTF-16 units) persisted per tab.
pub const MAX_PERSISTED_TAB_SIZE: usize = 200_000;
/// Maximum combined persisted tab payload across a whole session.
pub const MAX_PERSISTED_TAB_BUDGET: usize = 1_500_000;

/// Deserialize a list, silently skipping elements that do not parse so one
/// malformed or outdated entry does not discard the whole snapshot.
fn lenient_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: DeserializeOwned,
{
    let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect())
}

// Snapshots are read leniently: unknown fields are ignored, missing fields
// take defaults, unknown enum variants fall back to the default variant, and
// malformed list elements are dropped.

/// Sidebar panel identifiers (unknown values fall back to `Explorer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PersistedSidebarView {
    /// Search.
    Search,
    /// Source control.
    Git,
    /// Database browser.
    Database,
    /// Problems list.
    Problems,
    /// Tasks.
    Tasks,
    /// File explorer (also the fallback for unknown values; must stay last).
    #[default]
    #[serde(other)]
    Explorer,
}

/// Split direction of editor groups (unknown values fall back to `Horizontal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PersistedEditorLayoutDirection {
    /// Groups stacked.
    Vertical,
    /// Groups side by side (also the fallback for unknown values; must stay last).
    #[default]
    #[serde(other)]
    Horizontal,
}

/// One editor group.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersistedEditorGroup {
    /// Group id.
    pub id: String,
    /// Tabs in display order.
    #[serde(deserialize_with = "lenient_vec")]
    pub tab_ids: Vec<String>,
    /// Active tab in this group.
    pub active_tab_id: Option<String>,
}

/// A tab inside a persisted session. Content is omitted when too large.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedEditorTab {
    /// Tab id.
    pub id: String,
    /// File path.
    pub path: PathBuf,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Language id.
    #[serde(default)]
    pub language: String,
    /// Buffer content (omitted when over budget).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Saved content (omitted when over budget).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_content: Option<String>,
}

/// Editor session of one workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersistedEditorSession {
    /// Open tabs.
    #[serde(deserialize_with = "lenient_vec")]
    pub tabs: Vec<PersistedEditorTab>,
    /// Editor groups.
    #[serde(deserialize_with = "lenient_vec")]
    pub groups: Vec<PersistedEditorGroup>,
    /// Focused group.
    pub active_group_id: String,
    /// Group layout direction.
    pub layout_direction: PersistedEditorLayoutDirection,
}

/// Global UI state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersistedWorkbenchUi {
    /// Terminal panel visible.
    pub show_terminal: bool,
    /// System monitor visible.
    pub show_sys_monitor: bool,
    /// Active sidebar view.
    pub active_sidebar_view: PersistedSidebarView,
    /// Sidebar visible.
    pub show_sidebar: bool,
}

impl Default for PersistedWorkbenchUi {
    fn default() -> Self {
        Self {
            show_terminal: false,
            show_sys_monitor: false,
            active_sidebar_view: PersistedSidebarView::Explorer,
            show_sidebar: true,
        }
    }
}

/// One persisted terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedTerminalEntry {
    /// Terminal id.
    pub id: String,
    /// Working directory.
    #[serde(default)]
    pub cwd: PathBuf,
}

/// Terminal session of one workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersistedTerminalSession {
    /// Terminals.
    #[serde(deserialize_with = "lenient_vec")]
    pub terminals: Vec<PersistedTerminalEntry>,
    /// Active terminal id.
    pub active_id: String,
}

/// Length of `s` in UTF-16 code units (JS `String.length`).
fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Convert open tabs to their persisted form, dropping `content` and
/// `savedContent` for tabs that exceed [`MAX_PERSISTED_TAB_SIZE`] or would push
/// the running total over [`MAX_PERSISTED_TAB_BUDGET`]. The tab metadata is
/// always kept so the tab can still be reopened from disk.
pub fn serialize_tabs_for_session(tabs: &[Tab]) -> Vec<PersistedEditorTab> {
    let mut budget = 0usize;
    tabs.iter()
        .map(|tab| {
            let payload = utf16_len(&tab.content) + utf16_len(&tab.saved_content);
            let can_persist =
                payload <= MAX_PERSISTED_TAB_SIZE && budget + payload <= MAX_PERSISTED_TAB_BUDGET;
            if can_persist {
                budget += payload;
            }
            PersistedEditorTab {
                id: tab.id.clone(),
                path: tab.path.clone(),
                name: tab.name.clone(),
                language: tab.language.clone(),
                content: can_persist.then(|| tab.content.clone()),
                saved_content: can_persist.then(|| tab.saved_content.clone()),
            }
        })
        .collect()
}

/// `encodeURIComponent` (unreserved set: `A-Za-z0-9 - _ . ! ~ * ' ( )`).
fn encode_uri_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Normalised workspace key: backslashes to `/`, lower-cased, URI-encoded.
pub fn normalize_workspace_key(root_path: &Path) -> String {
    let text = root_path.to_string_lossy().replace('\\', "/").to_lowercase();
    encode_uri_component(&text)
}

/// 64-bit FNV-1a (stable across runs and platforms).
fn fnv1a64(data: &str) -> u64 {
    data.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Directory name for a workspace: readable slug plus a hash of the key.
fn workspace_dir_name(root_path: &Path) -> String {
    let key = normalize_workspace_key(root_path);
    let normalized = root_path.to_string_lossy().replace('\\', "/").to_lowercase();
    let slug: String = normalized
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(32)
        .collect();
    let slug = if slug.is_empty() { "workspace".to_owned() } else { slug };
    format!("{slug}-{:016x}", fnv1a64(&key))
}

/// File-backed store for workbench snapshots.
///
/// All reads return `Ok(None)` when no snapshot exists *or* the stored JSON is
/// unreadable/outdated (matching the TypeScript `readStorage`, which returned
/// `null` on any parse failure). Genuine I/O errors are reported.
#[derive(Debug, Clone)]
pub struct WorkbenchStore {
    dir: PathBuf,
}

impl WorkbenchStore {
    /// Store rooted at `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Store in `<OS config dir>/workbench`.
    pub fn open_default() -> CoreResult<Self> {
        Ok(Self::new(default_config_dir()?.join("workbench")))
    }

    /// Directory backing the store.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn workspace_file(&self, root_path: &Path, section: &str) -> PathBuf {
        self.dir
            .join("workspaces")
            .join(workspace_dir_name(root_path))
            .join(format!("{section}.json"))
    }

    fn global_file(&self, section: &str) -> PathBuf {
        self.dir.join("global").join(format!("{section}.json"))
    }

    fn read<T: DeserializeOwned>(path: &Path) -> CoreResult<Option<T>> {
        Ok(fsutil::read_optional(path)?.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
    }

    fn remove(path: &Path) -> CoreResult<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CoreError::io(path, e)),
        }
    }

    /// Read the editor session of `root_path`.
    pub fn read_editor_session(
        &self,
        root_path: impl AsRef<Path>,
    ) -> CoreResult<Option<PersistedEditorSession>> {
        Self::read(&self.workspace_file(root_path.as_ref(), "editor"))
    }

    /// Write the editor session of `root_path`.
    pub fn write_editor_session(
        &self,
        root_path: impl AsRef<Path>,
        snapshot: &PersistedEditorSession,
    ) -> CoreResult<()> {
        fsutil::atomic_write_json(&self.workspace_file(root_path.as_ref(), "editor"), snapshot)
    }

    /// Delete the editor session of `root_path`.
    pub fn clear_editor_session(&self, root_path: impl AsRef<Path>) -> CoreResult<()> {
        Self::remove(&self.workspace_file(root_path.as_ref(), "editor"))
    }

    /// Read the terminal session of `root_path`.
    pub fn read_terminal_session(
        &self,
        root_path: impl AsRef<Path>,
    ) -> CoreResult<Option<PersistedTerminalSession>> {
        Self::read(&self.workspace_file(root_path.as_ref(), "terminal"))
    }

    /// Write the terminal session of `root_path`.
    pub fn write_terminal_session(
        &self,
        root_path: impl AsRef<Path>,
        snapshot: &PersistedTerminalSession,
    ) -> CoreResult<()> {
        fsutil::atomic_write_json(&self.workspace_file(root_path.as_ref(), "terminal"), snapshot)
    }

    /// Delete the terminal session of `root_path`.
    pub fn clear_terminal_session(&self, root_path: impl AsRef<Path>) -> CoreResult<()> {
        Self::remove(&self.workspace_file(root_path.as_ref(), "terminal"))
    }

    /// Read the global UI state.
    pub fn read_workbench_ui(&self) -> CoreResult<Option<PersistedWorkbenchUi>> {
        Self::read(&self.global_file("ui"))
    }

    /// Write the global UI state.
    pub fn write_workbench_ui(&self, snapshot: &PersistedWorkbenchUi) -> CoreResult<()> {
        fsutil::atomic_write_json(&self.global_file("ui"), snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_tab(id: &str, content: &str, saved: &str) -> Tab {
        Tab {
            id: id.into(),
            path: format!("C:\\workspace\\{id}.ts").into(),
            name: format!("{id}.ts"),
            language: "typescript".into(),
            content: content.into(),
            saved_content: saved.into(),
            recovery_state: None,
            recovery_message: None,
        }
    }

    fn session(id: &str, root: &str) -> PersistedEditorSession {
        PersistedEditorSession {
            tabs: vec![PersistedEditorTab {
                id: id.into(),
                path: format!("{root}\\{id}.ts").into(),
                name: format!("{id}.ts"),
                language: "typescript".into(),
                content: None,
                saved_content: None,
            }],
            groups: vec![PersistedEditorGroup {
                id: "main".into(),
                tab_ids: vec![id.into()],
                active_tab_id: Some(id.into()),
            }],
            active_group_id: "main".into(),
            layout_direction: PersistedEditorLayoutDirection::Horizontal,
        }
    }

    #[test]
    fn keeps_editor_snapshots_scoped_to_the_workspace_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkbenchStore::new(dir.path());

        store
            .write_editor_session("C:\\workspace-a", &session("a", "C:\\workspace-a"))
            .unwrap();
        store
            .write_editor_session("C:\\workspace-b", &session("b", "C:\\workspace-b"))
            .unwrap();

        let a = store.read_editor_session("C:\\workspace-a").unwrap().unwrap();
        let b = store.read_editor_session("C:\\workspace-b").unwrap().unwrap();
        assert_eq!(a.tabs[0].id, "a");
        assert_eq!(b.tabs[0].id, "b");
        assert!(store.read_editor_session("C:\\workspace-c").unwrap().is_none());
    }

    #[test]
    fn workspace_keys_are_case_and_separator_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkbenchStore::new(dir.path());
        store
            .write_editor_session("C:\\Proj", &session("a", "C:\\Proj"))
            .unwrap();
        assert!(store.read_editor_session("c:/proj").unwrap().is_some());
        assert_eq!(
            normalize_workspace_key(Path::new("C:\\Proj")),
            "c%3A%2Fproj"
        );
    }

    #[test]
    fn stores_global_ui_preferences_independently_from_workspace_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkbenchStore::new(dir.path());
        assert!(store.read_workbench_ui().unwrap().is_none());

        let ui = PersistedWorkbenchUi {
            show_terminal: false,
            show_sys_monitor: true,
            active_sidebar_view: PersistedSidebarView::Git,
            show_sidebar: true,
        };
        store.write_workbench_ui(&ui).unwrap();
        store
            .write_editor_session("/ws", &session("a", "/ws"))
            .unwrap();
        assert_eq!(store.read_workbench_ui().unwrap(), Some(ui));
    }

    #[test]
    fn terminal_sessions_round_trip_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkbenchStore::new(dir.path());
        let snap = PersistedTerminalSession {
            terminals: vec![PersistedTerminalEntry {
                id: "t1".into(),
                cwd: "/ws".into(),
            }],
            active_id: "t1".into(),
        };
        store.write_terminal_session("/ws", &snap).unwrap();
        assert_eq!(store.read_terminal_session("/ws").unwrap(), Some(snap));
        store.clear_terminal_session("/ws").unwrap();
        store.clear_terminal_session("/ws").unwrap();
        assert!(store.read_terminal_session("/ws").unwrap().is_none());
    }

    #[test]
    fn snapshots_tolerate_unknown_fields_variants_and_bad_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkbenchStore::new(dir.path());
        store
            .write_editor_session("/ws", &session("a", "/ws"))
            .unwrap();
        let file = store.workspace_file(Path::new("/ws"), "editor");
        std::fs::write(
            &file,
            r#"{
                "futureField": {"x": 1},
                "layoutDirection": "diagonal",
                "tabs": [
                    {"id": "good", "path": "/ws/good.ts", "name": "good.ts", "extra": true},
                    {"id": 42},
                    "garbage",
                    {"id": "nopath"}
                ],
                "groups": [{"id": "main", "tabIds": ["good"], "activeTabId": "good", "newer": 1}]
            }"#,
        )
        .unwrap();
        let loaded = store.read_editor_session("/ws").unwrap().unwrap();
        assert_eq!(loaded.layout_direction, PersistedEditorLayoutDirection::Horizontal);
        assert_eq!(loaded.tabs.len(), 1);
        assert_eq!(loaded.tabs[0].id, "good");
        assert_eq!(loaded.tabs[0].language, "");
        assert_eq!(loaded.groups[0].active_tab_id.as_deref(), Some("good"));
        assert_eq!(loaded.active_group_id, "");

        // UI state: unknown sidebar view and missing fields.
        let ui = store.global_file("ui");
        std::fs::create_dir_all(ui.parent().unwrap()).unwrap();
        std::fs::write(&ui, r#"{"activeSidebarView":"timeline","showTerminal":true,"x":1}"#).unwrap();
        let loaded = store.read_workbench_ui().unwrap().unwrap();
        assert_eq!(loaded.active_sidebar_view, PersistedSidebarView::Explorer);
        assert!(loaded.show_terminal && loaded.show_sidebar);

        // Terminal list: bad entries are skipped.
        let term = store.workspace_file(Path::new("/ws"), "terminal");
        std::fs::write(&term, r#"{"terminals":[{"id":"t1","cwd":"/ws"},{"cwd":5}],"activeId":"t1"}"#)
            .unwrap();
        let loaded = store.read_terminal_session("/ws").unwrap().unwrap();
        assert_eq!(loaded.terminals.len(), 1);
    }

    #[test]
    fn corrupt_snapshots_read_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkbenchStore::new(dir.path());
        store
            .write_editor_session("/ws", &session("a", "/ws"))
            .unwrap();
        let file = store.workspace_file(Path::new("/ws"), "editor");
        std::fs::write(&file, "{ broken").unwrap();
        assert!(store.read_editor_session("/ws").unwrap().is_none());
    }

    #[test]
    fn drops_oversized_tab_content_while_keeping_the_tab_metadata_restorable() {
        let small = create_tab("small", "console.log(\"ok\")\n", "console.log(\"ok\")\n");
        let large_text = "x".repeat(240_000);
        let large = create_tab("large", &large_text, &large_text);

        let snapshot = serialize_tabs_for_session(&[small, large]);

        assert_eq!(snapshot[0].id, "small");
        assert_eq!(snapshot[0].content.as_deref(), Some("console.log(\"ok\")\n"));
        assert_eq!(snapshot[1].id, "large");
        assert_eq!(snapshot[1].content, None);
        assert_eq!(snapshot[1].saved_content, None);
        assert_eq!(snapshot[1].path, PathBuf::from("C:\\workspace\\large.ts"));
    }

    #[test]
    fn total_budget_limits_persisted_content() {
        // Each tab: 2 * 100_000 = 200_000 (at the per-tab limit); 7 fit in 1.5M, the 8th does not.
        let text = "y".repeat(100_000);
        let tabs: Vec<Tab> = (0..8)
            .map(|i| create_tab(&format!("t{i}"), &text, &text))
            .collect();
        let snapshot = serialize_tabs_for_session(&tabs);
        assert_eq!(snapshot.iter().filter(|t| t.content.is_some()).count(), 7);
        assert!(snapshot[7].content.is_none());
    }

    #[test]
    fn omitted_content_is_not_written_to_json() {
        let tab = serialize_tabs_for_session(&[create_tab("x", &"z".repeat(300_000), "")]);
        let json = serde_json::to_string(&tab[0]).unwrap();
        assert!(!json.contains("content"));
        assert!(json.contains("\"language\""));
    }
}
