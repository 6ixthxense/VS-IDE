//! Dark theme matching the Electron app's palette (see `src/renderer/styles/app.css`
//! in the repo root: `--bg #1e1e1e`, `--bg-sidebar #252526`, `--acc #007acc`, ...).
//!
//! The colours live in `assets/vsm-dark.json` in gpui-component's theme-file
//! format, so they can later be overridden by a user theme file or the `core`
//! crate's config without code changes.

use std::rc::Rc;

use gpui::App;
use gpui_component::{Theme, ThemeSet};

const THEME_JSON: &str = include_str!("../assets/vsm-dark.json");

pub fn apply(cx: &mut App) {
    let set: ThemeSet = serde_json::from_str(THEME_JSON).expect("assets/vsm-dark.json is valid");
    let Some(config) = set.themes.into_iter().next() else {
        return;
    };
    let config = Rc::new(config);
    Theme::update(cx, |theme| theme.apply_config(&config));
}
