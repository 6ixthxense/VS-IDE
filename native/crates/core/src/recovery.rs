//! Unsaved-edit recovery and external-change conflict detection
//! (port of `src/renderer/utils/editorRecovery.ts`).

use crate::types::{Tab, TabRecoveryState};
use crate::workbench::PersistedEditorTab;

/// Message shown for a tab restored with unsaved changes.
pub const RECOVERED_CHANGES_MESSAGE: &str = "Recovered unsaved changes from your previous session.";
/// Message shown when the file changed on disk while the app was closed.
pub const CONFLICT_ON_RESTORE_MESSAGE: &str = "This file changed on disk while the app was closed.";
/// Message shown when the file changed on disk underneath unsaved edits.
pub const CONFLICT_ON_SYNC_MESSAGE: &str =
    "This file changed on disk while you still have unsaved edits.";
/// Message shown when the file disappeared from disk.
pub const FILE_MISSING_MESSAGE: &str = "This file is no longer available on disk.";

/// Problem found while restoring a tab from a session snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoredIssue {
    /// Unsaved edits were restored on top of an unchanged file.
    Recovered,
    /// Unsaved edits conflict with a file that changed on disk.
    Conflict,
}

/// What [`sync_tab_with_disk`] did to the tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncEffect {
    /// Disk content equals the saved content; nothing to do.
    Unchanged,
    /// Disk content equals the buffer (saved elsewhere); the tab is now clean.
    Saved,
    /// A clean tab was reloaded from disk.
    Reloaded,
    /// A dirty tab's file changed on disk.
    Conflict,
    /// The file no longer exists.
    Missing,
}

/// Result of [`resolve_restored_tab`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredTabResolution {
    /// Issue to surface to the user, if any.
    pub issue: Option<RestoredIssue>,
    /// The tab to open.
    pub tab: Tab,
}

/// Result of [`sync_tab_with_disk`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedTabResolution {
    /// What happened.
    pub effect: SyncEffect,
    /// The updated tab.
    pub tab: Tab,
}

/// Reconcile a persisted tab with the current `disk_content` of its file.
///
/// * No unsaved changes (or buffer already equals disk): clean tab from disk.
/// * Disk equals the persisted saved content: unsaved edits are restored
///   (`Recovered`).
/// * Otherwise the file changed independently: edits are kept and flagged
///   (`Conflict`) with the new disk content as the saved baseline.
pub fn resolve_restored_tab(
    snapshot: &PersistedEditorTab,
    disk_content: &str,
) -> RestoredTabResolution {
    let persisted_content = snapshot.content.as_deref().unwrap_or(disk_content);
    let persisted_saved = snapshot.saved_content.as_deref().unwrap_or(disk_content);
    let has_unsaved_changes = persisted_content != persisted_saved;

    let base = |content: &str, saved: &str| Tab {
        id: snapshot.id.clone(),
        path: snapshot.path.clone(),
        name: snapshot.name.clone(),
        language: snapshot.language.clone(),
        content: content.to_owned(),
        saved_content: saved.to_owned(),
        recovery_state: None,
        recovery_message: None,
    };

    if !has_unsaved_changes || disk_content == persisted_content {
        return RestoredTabResolution {
            issue: None,
            tab: base(disk_content, disk_content),
        };
    }

    if disk_content == persisted_saved {
        return RestoredTabResolution {
            issue: Some(RestoredIssue::Recovered),
            tab: Tab {
                recovery_state: Some(TabRecoveryState::Recovered),
                recovery_message: Some(RECOVERED_CHANGES_MESSAGE.to_owned()),
                ..base(persisted_content, disk_content)
            },
        };
    }

    RestoredTabResolution {
        issue: Some(RestoredIssue::Conflict),
        tab: Tab {
            recovery_state: Some(TabRecoveryState::Conflict),
            recovery_message: Some(CONFLICT_ON_RESTORE_MESSAGE.to_owned()),
            ..base(persisted_content, disk_content)
        },
    }
}

/// Reconcile an open `tab` with the file's current state on disk.
pub fn sync_tab_with_disk(tab: &Tab, disk_exists: bool, disk_content: &str) -> SyncedTabResolution {
    if !disk_exists {
        return SyncedTabResolution {
            effect: SyncEffect::Missing,
            tab: Tab {
                recovery_state: Some(TabRecoveryState::Conflict),
                recovery_message: Some(FILE_MISSING_MESSAGE.to_owned()),
                ..tab.clone()
            },
        };
    }

    if disk_content == tab.saved_content {
        // The file is back after being reported missing: drop that flag, but
        // leave any other recovery/conflict state alone.
        let was_missing = tab.recovery_message.as_deref() == Some(FILE_MISSING_MESSAGE);
        return SyncedTabResolution {
            effect: SyncEffect::Unchanged,
            tab: if was_missing {
                Tab {
                    recovery_state: None,
                    recovery_message: None,
                    ..tab.clone()
                }
            } else {
                tab.clone()
            },
        };
    }

    if disk_content == tab.content {
        return SyncedTabResolution {
            effect: SyncEffect::Saved,
            tab: Tab {
                saved_content: disk_content.to_owned(),
                recovery_state: None,
                recovery_message: None,
                ..tab.clone()
            },
        };
    }

    if tab.content == tab.saved_content {
        return SyncedTabResolution {
            effect: SyncEffect::Reloaded,
            tab: Tab {
                content: disk_content.to_owned(),
                saved_content: disk_content.to_owned(),
                recovery_state: None,
                recovery_message: None,
                ..tab.clone()
            },
        };
    }

    SyncedTabResolution {
        effect: SyncEffect::Conflict,
        tab: Tab {
            saved_content: disk_content.to_owned(),
            recovery_state: Some(TabRecoveryState::Conflict),
            recovery_message: Some(CONFLICT_ON_SYNC_MESSAGE.to_owned()),
            ..tab.clone()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab_with(content: &str, saved: &str) -> Tab {
        Tab {
            id: "app.ts".into(),
            path: "C:\\workspace\\app.ts".into(),
            name: "app.ts".into(),
            language: "typescript".into(),
            content: content.into(),
            saved_content: saved.into(),
            recovery_state: None,
            recovery_message: None,
        }
    }

    fn snapshot(content: Option<&str>, saved: Option<&str>) -> PersistedEditorTab {
        PersistedEditorTab {
            id: "app.ts".into(),
            path: "C:\\workspace\\app.ts".into(),
            name: "app.ts".into(),
            language: "typescript".into(),
            content: content.map(Into::into),
            saved_content: saved.map(Into::into),
        }
    }

    #[test]
    fn marks_unsaved_session_content_as_recovered_when_disk_content_is_unchanged() {
        let r = resolve_restored_tab(
            &snapshot(Some("const value = 2\n"), Some("const value = 1\n")),
            "const value = 1\n",
        );
        assert_eq!(r.issue, Some(RestoredIssue::Recovered));
        assert_eq!(r.tab.content, "const value = 2\n");
        assert_eq!(r.tab.saved_content, "const value = 1\n");
        assert_eq!(r.tab.recovery_state, Some(TabRecoveryState::Recovered));
        assert_eq!(r.tab.recovery_message.as_deref(), Some(RECOVERED_CHANGES_MESSAGE));
    }

    #[test]
    fn marks_restored_tabs_as_conflicted_when_disk_content_changed_separately() {
        let r = resolve_restored_tab(
            &snapshot(Some("const value = 2\n"), Some("const value = 1\n")),
            "const value = 3\n",
        );
        assert_eq!(r.issue, Some(RestoredIssue::Conflict));
        assert_eq!(r.tab.content, "const value = 2\n");
        assert_eq!(r.tab.saved_content, "const value = 3\n");
        assert_eq!(r.tab.recovery_state, Some(TabRecoveryState::Conflict));
        assert_eq!(r.tab.recovery_message.as_deref(), Some(CONFLICT_ON_RESTORE_MESSAGE));
    }

    #[test]
    fn restores_clean_tab_from_disk_when_snapshot_has_no_content_or_matches_disk() {
        let r = resolve_restored_tab(&snapshot(None, None), "disk");
        assert_eq!(r.issue, None);
        assert_eq!((r.tab.content.as_str(), r.tab.saved_content.as_str()), ("disk", "disk"));

        // Edits that were already saved to disk become a clean tab.
        let r = resolve_restored_tab(&snapshot(Some("new"), Some("old")), "new");
        assert_eq!(r.issue, None);
        assert!(!r.tab.is_dirty());
        assert_eq!(r.tab.recovery_state, None);
    }

    #[test]
    fn reloads_clean_tabs_when_the_disk_version_changes_externally() {
        let r = sync_tab_with_disk(
            &tab_with("const value = 1\n", "const value = 1\n"),
            true,
            "const value = 4\n",
        );
        assert_eq!(r.effect, SyncEffect::Reloaded);
        assert_eq!(r.tab.content, "const value = 4\n");
        assert_eq!(r.tab.saved_content, "const value = 4\n");
    }

    #[test]
    fn marks_dirty_tabs_as_conflicted_when_the_disk_version_changes_underneath_them() {
        let r = sync_tab_with_disk(
            &tab_with("const value = 2\n", "const value = 1\n"),
            true,
            "const value = 5\n",
        );
        assert_eq!(r.effect, SyncEffect::Conflict);
        assert_eq!(r.tab.content, "const value = 2\n");
        assert_eq!(r.tab.saved_content, "const value = 5\n");
        assert_eq!(r.tab.recovery_state, Some(TabRecoveryState::Conflict));
        assert_eq!(r.tab.recovery_message.as_deref(), Some(CONFLICT_ON_SYNC_MESSAGE));
    }

    #[test]
    fn handles_unchanged_saved_and_missing_files() {
        let dirty = tab_with("b", "a");
        assert_eq!(sync_tab_with_disk(&dirty, true, "a").effect, SyncEffect::Unchanged);

        let mut flagged = dirty.clone();
        flagged.recovery_state = Some(TabRecoveryState::Conflict);
        flagged.recovery_message = Some("x".into());
        let saved = sync_tab_with_disk(&flagged, true, "b");
        assert_eq!(saved.effect, SyncEffect::Saved);
        assert_eq!(saved.tab.saved_content, "b");
        assert_eq!(saved.tab.recovery_state, None);
        assert_eq!(saved.tab.recovery_message, None);

        let missing = sync_tab_with_disk(&dirty, false, "");
        assert_eq!(missing.effect, SyncEffect::Missing);
        assert_eq!(missing.tab.content, "b");
        assert_eq!(missing.tab.recovery_message.as_deref(), Some(FILE_MISSING_MESSAGE));
    }

    #[test]
    fn the_missing_flag_is_cleared_when_the_file_comes_back_unchanged() {
        let missing = sync_tab_with_disk(&tab_with("b", "a"), false, "").tab;
        let back = sync_tab_with_disk(&missing, true, "a");
        assert_eq!(back.effect, SyncEffect::Unchanged);
        assert_eq!(back.tab.recovery_state, None);
        assert_eq!(back.tab.recovery_message, None);
        assert_eq!(back.tab.content, "b");

        // Other conflict states are preserved.
        let mut conflicted = tab_with("b", "a");
        conflicted.recovery_state = Some(TabRecoveryState::Conflict);
        conflicted.recovery_message = Some(CONFLICT_ON_SYNC_MESSAGE.into());
        let same = sync_tab_with_disk(&conflicted, true, "a");
        assert_eq!(same.tab, conflicted);
    }
}
