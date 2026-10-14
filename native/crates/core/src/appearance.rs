//! Appearance settings (port of `src/renderer/config/appearance.ts`).
//!
//! The DOM-specific `applyAppearanceConfig` has no equivalent here; the UI
//! crate maps [`AppearanceConfig`] onto its own theme.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Colour theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    /// "Night Shift" (default).
    #[default]
    Dark,
    /// "Daylight".
    Light,
    /// "OLED Black".
    Amoled,
}

impl ThemeName {
    /// Lowercase identifier (`dark`, `light`, `amoled`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Amoled => "amoled",
        }
    }

    /// Parse a lowercase identifier.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "amoled" => Some(Self::Amoled),
            _ => None,
        }
    }
}

/// Preview swatches for a theme picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ThemePreview {
    /// Window frame colour.
    pub frame: &'static str,
    /// Panel colour.
    pub panel: &'static str,
    /// Editor surface colour.
    pub surface: &'static str,
    /// Accent colour.
    pub accent: &'static str,
    /// Text colour.
    pub text: &'static str,
}

/// A selectable theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ThemeOption {
    /// Theme identifier.
    pub id: ThemeName,
    /// Display label.
    pub label: &'static str,
    /// Short description.
    pub description: &'static str,
    /// Preview swatches.
    pub preview: ThemePreview,
}

/// A selectable accent colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AccentOption {
    /// Identifier.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Short description.
    pub description: &'static str,
    /// Hex colour, also the persisted value.
    pub value: &'static str,
    /// CSS-style gradient string.
    pub gradient: &'static str,
}

/// A monospace font preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FontPreset {
    /// Display label.
    pub label: &'static str,
    /// CSS-style font stack.
    pub value: &'static str,
}

/// Available themes.
pub const THEME_OPTIONS: [ThemeOption; 3] = [
    ThemeOption {
        id: ThemeName::Dark,
        label: "Night Shift",
        description: "Balanced contrast for long coding sessions.",
        preview: ThemePreview {
            frame: "#1e1e1f",
            panel: "#252526",
            surface: "#121314",
            accent: "#0a84ff",
            text: "#f3f4f6",
        },
    },
    ThemeOption {
        id: ThemeName::Light,
        label: "Daylight",
        description: "Bright panels with softer borders and cleaner text.",
        preview: ThemePreview {
            frame: "#e7edf3",
            panel: "#f7fafc",
            surface: "#ffffff",
            accent: "#2563eb",
            text: "#111827",
        },
    },
    ThemeOption {
        id: ThemeName::Amoled,
        label: "OLED Black",
        description: "Pure black chrome for high-contrast setups.",
        preview: ThemePreview {
            frame: "#020202",
            panel: "#080808",
            surface: "#000000",
            accent: "#22c55e",
            text: "#fafafa",
        },
    },
];

/// Available accent colours. The first entry is the default.
pub const ACCENT_OPTIONS: [AccentOption; 5] = [
    AccentOption {
        id: "ocean",
        name: "Ocean Blue",
        description: "Clean and familiar with a VS-style feel.",
        value: "#007acc",
        gradient: "linear-gradient(135deg, #007acc, #005a9e)",
    },
    AccentOption {
        id: "jade",
        name: "Jade",
        description: "Fresh and calm with clearer success states.",
        value: "#12b981",
        gradient: "linear-gradient(135deg, #12b981, #0f766e)",
    },
    AccentOption {
        id: "sunset",
        name: "Sunset",
        description: "Warm highlight for tabs, buttons, and focus rings.",
        value: "#f97316",
        gradient: "linear-gradient(135deg, #f97316, #ea580c)",
    },
    AccentOption {
        id: "rose",
        name: "Rose",
        description: "A punchier accent that still reads well on dark UIs.",
        value: "#e11d48",
        gradient: "linear-gradient(135deg, #e11d48, #be123c)",
    },
    AccentOption {
        id: "indigo",
        name: "Indigo",
        description: "Cool and deeper than the default blue.",
        value: "#6366f1",
        gradient: "linear-gradient(135deg, #6366f1, #4338ca)",
    },
];

/// Editor font presets. The first entry is the default.
pub const FONT_PRESETS: [FontPreset; 4] = [
    FontPreset {
        label: "Cascadia Code",
        value: "'Cascadia Code', 'Fira Code', Consolas, monospace",
    },
    FontPreset {
        label: "JetBrains Mono",
        value: "'JetBrains Mono', 'Cascadia Code', Consolas, monospace",
    },
    FontPreset {
        label: "IBM Plex Mono",
        value: "'IBM Plex Mono', 'Cascadia Code', Consolas, monospace",
    },
    FontPreset {
        label: "Fira Code",
        value: "'Fira Code', 'Cascadia Code', Consolas, monospace",
    },
];

/// Default editor font size.
pub const DEFAULT_FONT_SIZE: u32 = 14;
/// Default terminal font size.
pub const DEFAULT_TERMINAL_FONT_SIZE: u32 = 12;
/// Default tab width.
pub const DEFAULT_TAB_SIZE: u32 = 4;

/// All user-tunable appearance and editor settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceConfig {
    /// Colour theme.
    pub theme: ThemeName,
    /// Accent colour (one of [`ACCENT_OPTIONS`] values).
    pub accent: String,
    /// Gradient matching [`AppearanceConfig::accent`].
    pub accent_gradient: String,
    /// Editor font size (12..=24).
    pub font_size: u32,
    /// Terminal font size (10..=22).
    pub terminal_font_size: u32,
    /// Editor font stack.
    pub font_family: String,
    /// Soft-wrap long lines.
    pub word_wrap: bool,
    /// Show line numbers.
    pub line_numbers: bool,
    /// Save automatically.
    pub auto_save: bool,
    /// Tab width (2..=8).
    pub tab_size: u32,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: ThemeName::Dark,
            accent: ACCENT_OPTIONS[0].value.to_owned(),
            accent_gradient: ACCENT_OPTIONS[0].gradient.to_owned(),
            font_size: DEFAULT_FONT_SIZE,
            terminal_font_size: DEFAULT_TERMINAL_FONT_SIZE,
            font_family: FONT_PRESETS[0].value.to_owned(),
            word_wrap: true,
            line_numbers: true,
            auto_save: false,
            tab_size: DEFAULT_TAB_SIZE,
        }
    }
}

/// Round to nearest integer and clamp into `min..=max`; non-finite values
/// fall back to `fallback`.
fn clamp_number(value: f64, fallback: u32, min: u32, max: u32) -> u32 {
    if !value.is_finite() {
        return fallback;
    }
    // JS `Math.round` rounds half towards +infinity.
    let rounded = (value + 0.5).floor();
    rounded.clamp(f64::from(min), f64::from(max)) as u32
}

/// Clamp the editor font size into 12..=24 (non-finite input gives the default).
pub fn sanitize_editor_font_size(value: impl Into<f64>) -> u32 {
    clamp_number(value.into(), DEFAULT_FONT_SIZE, 12, 24)
}

/// Clamp the terminal font size into 10..=22 (non-finite input gives the default).
pub fn sanitize_terminal_font_size(value: impl Into<f64>) -> u32 {
    clamp_number(value.into(), DEFAULT_TERMINAL_FONT_SIZE, 10, 22)
}

/// Trim the font family; a blank value yields the default font stack.
pub fn sanitize_font_family(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        FONT_PRESETS[0].value.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Clamp the tab size into 2..=8 (non-finite input gives the default).
pub fn sanitize_tab_size(value: impl Into<f64>) -> u32 {
    clamp_number(value.into(), DEFAULT_TAB_SIZE, 2, 8)
}

/// Find the accent option whose value equals `value`, falling back to the
/// default accent for unknown or missing values.
pub fn resolve_accent_option(value: Option<&str>) -> &'static AccentOption {
    value
        .and_then(|v| ACCENT_OPTIONS.iter().find(|a| a.value == v))
        .unwrap_or(&ACCENT_OPTIONS[0])
}

impl AppearanceConfig {
    /// Set the accent (and its gradient); unknown values fall back to the default accent.
    pub fn set_accent(&mut self, accent: &str) {
        let option = resolve_accent_option(Some(accent));
        self.accent = option.value.to_owned();
        self.accent_gradient = option.gradient.to_owned();
    }

    /// Set the editor font size, sanitized.
    pub fn set_font_size(&mut self, size: impl Into<f64>) {
        self.font_size = sanitize_editor_font_size(size);
    }

    /// Set the terminal font size, sanitized.
    pub fn set_terminal_font_size(&mut self, size: impl Into<f64>) {
        self.terminal_font_size = sanitize_terminal_font_size(size);
    }

    /// Set the font family, sanitized.
    pub fn set_font_family(&mut self, family: &str) {
        self.font_family = sanitize_font_family(family);
    }

    /// Set the tab size, sanitized.
    pub fn set_tab_size(&mut self, size: impl Into<f64>) {
        self.tab_size = sanitize_tab_size(size);
    }

    /// Build a config from arbitrary JSON, mirroring `getInitialAppearanceConfig`:
    /// every field is validated and sanitized, anything missing or of the wrong
    /// type falls back to its default. Accepts the legacy `vs-monitor-accent` key.
    pub fn from_json_lenient(value: &Value) -> Self {
        let defaults = Self::default();
        let get = |key: &str| value.get(key);
        let get_str = |key: &str| get(key).and_then(Value::as_str);
        let get_num = |key: &str| get(key).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let get_bool = |key: &str, fallback: bool| {
            get(key).and_then(Value::as_bool).unwrap_or(fallback)
        };

        let stored_accent = get_str("accent")
            .filter(|s| !s.is_empty())
            .or_else(|| get_str("vs-monitor-accent"));
        let accent = resolve_accent_option(stored_accent);

        Self {
            theme: get_str("theme")
                .and_then(ThemeName::parse)
                .unwrap_or(defaults.theme),
            accent: accent.value.to_owned(),
            accent_gradient: accent.gradient.to_owned(),
            font_size: sanitize_editor_font_size(get_num("fontSize")),
            terminal_font_size: sanitize_terminal_font_size(get_num("terminalFontSize")),
            font_family: sanitize_font_family(
                get_str("fontFamily").unwrap_or(FONT_PRESETS[0].value),
            ),
            word_wrap: get_bool("wordWrap", defaults.word_wrap),
            line_numbers: get_bool("lineNumbers", defaults.line_numbers),
            auto_save: get_bool("autoSave", defaults.auto_save),
            tab_size: sanitize_tab_size(get_num("tabSize")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn falls_back_to_the_default_accent_when_the_value_is_unknown() {
        assert_eq!(
            resolve_accent_option(Some("#not-found")).value,
            AppearanceConfig::default().accent
        );
        assert_eq!(resolve_accent_option(None).value, "#007acc");
        assert_eq!(resolve_accent_option(Some("#12b981")).id, "jade");
    }

    #[test]
    fn clamps_font_sizes_into_supported_ranges() {
        assert_eq!(sanitize_editor_font_size(8), 12);
        assert_eq!(sanitize_editor_font_size(40), 24);
        assert_eq!(sanitize_terminal_font_size(4), 10);
        assert_eq!(sanitize_terminal_font_size(30), 22);
        assert_eq!(sanitize_tab_size(1), 2);
        assert_eq!(sanitize_tab_size(9), 8);
    }

    #[test]
    fn rounds_and_handles_non_finite_numbers() {
        assert_eq!(sanitize_editor_font_size(14.5), 15);
        assert_eq!(sanitize_editor_font_size(14.4), 14);
        assert_eq!(sanitize_editor_font_size(f64::NAN), 14);
        assert_eq!(sanitize_terminal_font_size(f64::INFINITY), 12);
        assert_eq!(sanitize_tab_size(f64::NAN), 4);
    }

    #[test]
    fn keeps_the_default_font_stack_when_the_custom_value_is_blank() {
        assert_eq!(
            sanitize_font_family("   "),
            AppearanceConfig::default().font_family
        );
        assert_eq!(sanitize_font_family("  Menlo  "), "Menlo");
    }

    #[test]
    fn lenient_parsing_sanitizes_every_field() {
        let cfg = AppearanceConfig::from_json_lenient(&json!({
            "theme": "neon",
            "accent": "#f97316",
            "fontSize": 99,
            "terminalFontSize": "oops",
            "fontFamily": "   ",
            "wordWrap": false,
            "lineNumbers": "yes",
            "tabSize": 3,
        }));
        assert_eq!(cfg.theme, ThemeName::Dark);
        assert_eq!(cfg.accent, "#f97316");
        assert_eq!(cfg.accent_gradient, ACCENT_OPTIONS[2].gradient);
        assert_eq!(cfg.font_size, 24);
        assert_eq!(cfg.terminal_font_size, 12);
        assert_eq!(cfg.font_family, FONT_PRESETS[0].value);
        assert!(!cfg.word_wrap);
        assert!(cfg.line_numbers);
        assert_eq!(cfg.tab_size, 3);
        assert_eq!(
            AppearanceConfig::from_json_lenient(&json!({})),
            AppearanceConfig::default()
        );
        assert_eq!(
            AppearanceConfig::from_json_lenient(&json!({ "vs-monitor-accent": "#6366f1" })).accent,
            "#6366f1"
        );
    }
}
