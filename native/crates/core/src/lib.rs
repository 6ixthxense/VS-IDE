//! `vsm-core` - shared, UI-free logic for the native VS-Monitor IDE.
//!
//! This crate is the Rust port of the pure-logic parts of the Electron
//! renderer:
//!
//! * [`appearance`] / [`config`] - appearance settings (`appearance.ts`,
//!   `configStore.ts`) persisted as JSON in the OS config directory.
//! * [`workbench`] - per-workspace editor/terminal sessions and global UI
//!   state (`workbenchPersistence.ts`).
//! * [`recovery`] - unsaved-edit recovery and external-change conflict
//!   detection (`editorRecovery.ts`).
//! * [`text_edits`] - span based text edits (`textEdits.ts`).
//! * [`types`] - shared data types (`shared/types/file.ts`).
//! * [`fsutil`] - atomic JSON/file writes used by the persistence layers.
//!
//! Everything here is synchronous and free of UI dependencies. All fallible
//! operations return [`CoreError`].

pub mod appearance;
pub mod config;
pub mod error;
pub mod fsutil;
pub mod recovery;
pub mod text_edits;
pub mod types;
pub mod workbench;

pub use appearance::{AppearanceConfig, ThemeName};
pub use config::{AppConfig, ConfigStore};
pub use error::{CoreError, CoreResult};
pub use recovery::{
    resolve_restored_tab, sync_tab_with_disk, RestoredIssue, RestoredTabResolution, SyncEffect,
    SyncedTabResolution,
};
pub use text_edits::{apply_text_span_edits, TextSpanEdit};
pub use types::{FileEntry, Tab, TabRecoveryState};
pub use workbench::WorkbenchStore;
