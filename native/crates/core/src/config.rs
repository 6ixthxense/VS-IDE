//! Persistent app configuration (port of `configStore.ts`).
//!
//! The store keeps the config in memory and writes it atomically as JSON to
//! `<config dir>/config.json` after every change, like the zustand store that
//! persisted to `localStorage`.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::appearance::{AppearanceConfig, ThemeName};
use crate::error::{CoreError, CoreResult};
use crate::fsutil;

const CONFIG_FILE: &str = "config.json";
const CONFIG_VERSION: u32 = 1;

/// Per-user configuration directory for the app (e.g. `~/.config/vs-monitor-ide`).
pub fn default_config_dir() -> CoreResult<PathBuf> {
    ProjectDirs::from("com", "vs-monitor", "vs-monitor-ide")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .ok_or(CoreError::NoConfigDir)
}

/// Root of the persisted config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// File format version.
    pub version: u32,
    /// Appearance and editor settings.
    pub appearance: AppearanceConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            appearance: AppearanceConfig::default(),
        }
    }
}

impl AppConfig {
    /// Lenient parse: any invalid or missing part falls back to its default.
    pub fn from_json_lenient(value: &Value) -> Self {
        Self {
            version: CONFIG_VERSION,
            appearance: value
                .get("appearance")
                .map(AppearanceConfig::from_json_lenient)
                .unwrap_or_default(),
        }
    }
}

/// Config held in memory and mirrored to a JSON file.
#[derive(Debug)]
pub struct ConfigStore {
    path: PathBuf,
    config: AppConfig,
}

impl ConfigStore {
    /// Open the store at the default OS location.
    pub fn open_default() -> CoreResult<Self> {
        Self::open(default_config_dir()?.join(CONFIG_FILE))
    }

    /// Open the store backed by `path`. A missing or unparsable file yields
    /// the default configuration (the file is only written on the first change).
    pub fn open(path: impl Into<PathBuf>) -> CoreResult<Self> {
        let path = path.into();
        let config = match fsutil::read_optional(&path)? {
            Some(bytes) => serde_json::from_slice::<Value>(&bytes)
                .map(|v| AppConfig::from_json_lenient(&v))
                .unwrap_or_default(),
            None => AppConfig::default(),
        };
        Ok(Self { path, config })
    }

    /// Path of the backing file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Current config.
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Current appearance settings.
    pub fn appearance(&self) -> &AppearanceConfig {
        &self.config.appearance
    }

    /// Mutate the appearance settings and persist. If persisting fails the
    /// in-memory value is still updated and the error is returned.
    pub fn update_appearance(&mut self, f: impl FnOnce(&mut AppearanceConfig)) -> CoreResult<()> {
        f(&mut self.config.appearance);
        self.save()
    }

    /// Write the current config to disk atomically.
    pub fn save(&self) -> CoreResult<()> {
        fsutil::atomic_write_json(&self.path, &self.config)
    }

    /// Set the theme.
    pub fn set_theme(&mut self, theme: ThemeName) -> CoreResult<()> {
        self.update_appearance(|a| a.theme = theme)
    }

    /// Set the accent colour (unknown values fall back to the default accent).
    pub fn set_accent(&mut self, accent: &str) -> CoreResult<()> {
        self.update_appearance(|a| a.set_accent(accent))
    }

    /// Set the editor font size (clamped to 12..=24).
    pub fn set_font_size(&mut self, size: impl Into<f64>) -> CoreResult<()> {
        self.update_appearance(|a| a.set_font_size(size))
    }

    /// Set the terminal font size (clamped to 10..=22).
    pub fn set_terminal_font_size(&mut self, size: impl Into<f64>) -> CoreResult<()> {
        self.update_appearance(|a| a.set_terminal_font_size(size))
    }

    /// Set the editor font family (blank resets to the default stack).
    pub fn set_font_family(&mut self, family: &str) -> CoreResult<()> {
        self.update_appearance(|a| a.set_font_family(family))
    }

    /// Toggle word wrap.
    pub fn set_word_wrap(&mut self, enabled: bool) -> CoreResult<()> {
        self.update_appearance(|a| a.word_wrap = enabled)
    }

    /// Toggle line numbers.
    pub fn set_line_numbers(&mut self, enabled: bool) -> CoreResult<()> {
        self.update_appearance(|a| a.line_numbers = enabled)
    }

    /// Toggle auto save.
    pub fn set_auto_save(&mut self, enabled: bool) -> CoreResult<()> {
        self.update_appearance(|a| a.auto_save = enabled)
    }

    /// Set the tab size (clamped to 2..=8).
    pub fn set_tab_size(&mut self, size: impl Into<f64>) -> CoreResult<()> {
        self.update_appearance(|a| a.set_tab_size(size))
    }

    /// Reset all appearance settings to their defaults.
    pub fn reset_appearance(&mut self) -> CoreResult<()> {
        self.update_appearance(|a| *a = AppearanceConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults_and_changes_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg").join("config.json");

        let mut store = ConfigStore::open(&path).unwrap();
        assert_eq!(store.appearance(), &AppearanceConfig::default());
        assert!(!path.exists());

        store.set_theme(ThemeName::Amoled).unwrap();
        store.set_accent("#e11d48").unwrap();
        store.set_font_size(100.0).unwrap();
        store.set_tab_size(1.0).unwrap();
        store.set_font_family("  JetBrains Mono ").unwrap();
        store.set_auto_save(true).unwrap();

        let reopened = ConfigStore::open(&path).unwrap();
        let a = reopened.appearance();
        assert_eq!(a.theme, ThemeName::Amoled);
        assert_eq!(a.accent, "#e11d48");
        assert_eq!(a.accent_gradient, "linear-gradient(135deg, #e11d48, #be123c)");
        assert_eq!(a.font_size, 24);
        assert_eq!(a.tab_size, 2);
        assert_eq!(a.font_family, "JetBrains Mono");
        assert!(a.auto_save);
    }

    #[test]
    fn reset_restores_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut store = ConfigStore::open(&path).unwrap();
        store.set_theme(ThemeName::Light).unwrap();
        store.reset_appearance().unwrap();
        assert_eq!(
            ConfigStore::open(&path).unwrap().appearance(),
            &AppearanceConfig::default()
        );
    }

    #[test]
    fn corrupt_or_partial_files_fall_back_per_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(
            ConfigStore::open(&path).unwrap().appearance(),
            &AppearanceConfig::default()
        );

        std::fs::write(
            &path,
            r##"{"appearance":{"theme":"light","fontSize":"x","accent":"#nope"}}"##,
        )
        .unwrap();
        let store = ConfigStore::open(&path).unwrap();
        assert_eq!(store.appearance().theme, ThemeName::Light);
        assert_eq!(store.appearance().font_size, 14);
        assert_eq!(store.appearance().accent, "#007acc");
    }
}
