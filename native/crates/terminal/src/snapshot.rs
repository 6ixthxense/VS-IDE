//! Renderer-facing snapshot types and color resolution.

use alacritty_terminal::vte::ansi::{Color, CursorShape as VteCursorShape, Rgb as VteRgb};

/// 24-bit color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }
    const fn hex(v: u32) -> Self {
        Rgb { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8 }
    }
    pub(crate) fn dim(self) -> Self {
        // 2/3 brightness, like most terminals' SGR 2 handling.
        Rgb { r: (self.r as u16 * 2 / 3) as u8, g: (self.g as u16 * 2 / 3) as u8, b: (self.b as u16 * 2 / 3) as u8 }
    }
}

impl From<VteRgb> for Rgb {
    fn from(c: VteRgb) -> Self {
        Rgb { r: c.r, g: c.g, b: c.b }
    }
}

/// Theme palette used to resolve named/indexed colors into RGB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    /// ANSI colors 0..16 (normal then bright).
    pub ansi: [Rgb; 16],
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
}

impl Default for Palette {
    /// VS Code "Dark+" terminal colors.
    fn default() -> Self {
        Palette {
            ansi: [
                Rgb::hex(0x000000),
                Rgb::hex(0xcd3131),
                Rgb::hex(0x0dbc79),
                Rgb::hex(0xe5e510),
                Rgb::hex(0x2472c8),
                Rgb::hex(0xbc3fbc),
                Rgb::hex(0x11a8cd),
                Rgb::hex(0xe5e5e5),
                Rgb::hex(0x666666),
                Rgb::hex(0xf14c4c),
                Rgb::hex(0x23d18b),
                Rgb::hex(0xf5f543),
                Rgb::hex(0x3b8eea),
                Rgb::hex(0xd670d6),
                Rgb::hex(0x29b8db),
                Rgb::hex(0xe5e5e5),
            ],
            foreground: Rgb::hex(0xcccccc),
            background: Rgb::hex(0x1e1e1e),
            cursor: Rgb::hex(0xffffff),
        }
    }
}

impl Palette {
    /// Resolve an xterm palette index (0..=268 following alacritty's layout).
    pub fn indexed(&self, idx: usize) -> Rgb {
        match idx {
            0..=15 => self.ansi[idx],
            16..=231 => {
                let i = idx - 16;
                let level = |n: usize| if n == 0 { 0 } else { (55 + 40 * n) as u8 };
                Rgb::new(level(i / 36), level((i / 6) % 6), level(i % 6))
            }
            232..=255 => {
                let v = (8 + 10 * (idx - 232)) as u8;
                Rgb::new(v, v, v)
            }
            256 | 267 => self.foreground,
            257 => self.background,
            258 => self.cursor,
            259..=266 => self.ansi[idx - 259].dim(),
            268 => self.foreground.dim(),
            _ => self.foreground,
        }
    }

    pub(crate) fn resolve(&self, color: Color, overrides: &dyn Fn(usize) -> Option<VteRgb>) -> Rgb {
        match color {
            Color::Spec(c) => c.into(),
            Color::Indexed(i) => overrides(i as usize).map(Rgb::from).unwrap_or_else(|| self.indexed(i as usize)),
            Color::Named(n) => {
                let i = n as usize;
                overrides(i).map(Rgb::from).unwrap_or_else(|| self.indexed(i))
            }
        }
    }
}

/// Per-cell attribute bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CellFlags(pub u16);

impl CellFlags {
    pub const BOLD: u16 = 1 << 0;
    pub const ITALIC: u16 = 1 << 1;
    /// Single underline.
    pub const UNDERLINE: u16 = 1 << 2;
    pub const STRIKEOUT: u16 = 1 << 3;
    pub const DIM: u16 = 1 << 4;
    /// SGR 8: draw nothing.
    pub const HIDDEN: u16 = 1 << 5;
    /// First half of a double-width glyph.
    pub const WIDE: u16 = 1 << 6;
    /// Second half of a double-width glyph (skip drawing).
    pub const WIDE_SPACER: u16 = 1 << 7;
    /// Cell is inside the active selection.
    pub const SELECTED: u16 = 1 << 8;
    pub const DOUBLE_UNDERLINE: u16 = 1 << 9;
    pub const UNDERCURL: u16 = 1 << 10;
    pub const DOTTED_UNDERLINE: u16 = 1 << 11;
    pub const DASHED_UNDERLINE: u16 = 1 << 12;
    /// Background is the theme default (the renderer may skip painting it). Never set for
    /// inverse-video cells.
    pub const DEFAULT_BG: u16 = 1 << 13;
    /// Inverse video was applied (fg/bg are already swapped).
    pub const INVERSE: u16 = 1 << 14;

    pub fn contains(self, bits: u16) -> bool {
        self.0 & bits == bits
    }
    pub fn bold(self) -> bool {
        self.contains(Self::BOLD)
    }
    pub fn italic(self) -> bool {
        self.contains(Self::ITALIC)
    }
    pub fn underline(self) -> bool {
        self.0 & (Self::UNDERLINE | Self::DOUBLE_UNDERLINE | Self::UNDERCURL | Self::DOTTED_UNDERLINE | Self::DASHED_UNDERLINE) != 0
    }
    pub fn selected(self) -> bool {
        self.contains(Self::SELECTED)
    }
}

/// One visible grid cell with colors already resolved (inverse applied, dim applied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    /// Combining characters attached to `ch`.
    pub zerowidth: Option<Box<[char]>>,
    pub fg: Rgb,
    pub bg: Rgb,
    pub flags: CellFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Underline,
    Beam,
    HollowBlock,
    Hidden,
}

impl From<VteCursorShape> for CursorShape {
    fn from(s: VteCursorShape) -> Self {
        match s {
            VteCursorShape::Block => CursorShape::Block,
            VteCursorShape::Underline => CursorShape::Underline,
            VteCursorShape::Beam => CursorShape::Beam,
            VteCursorShape::HollowBlock => CursorShape::HollowBlock,
            VteCursorShape::Hidden => CursorShape::Hidden,
        }
    }
}

/// Cursor position in viewport coordinates. `visible` is false when the app hid the cursor or the
/// view is scrolled so the cursor row is off screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorSnapshot {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    pub visible: bool,
}

/// Terminal modes relevant to input encoding by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModeFlags {
    pub alt_screen: bool,
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub bracketed_paste: bool,
    /// Any mouse reporting mode (click, drag or motion) is active.
    pub mouse_reporting: bool,
    pub mouse_drag: bool,
    pub mouse_motion: bool,
    pub sgr_mouse: bool,
    pub focus_reporting: bool,
    /// Mouse wheel should be translated to arrow keys on the alternate screen.
    pub alternate_scroll: bool,
}

/// Everything a renderer needs to draw the visible viewport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridSnapshot {
    pub cols: usize,
    pub rows: usize,
    /// Row-major, `rows * cols` cells.
    pub cells: Vec<Cell>,
    pub cursor: CursorSnapshot,
    /// Lines scrolled back from the bottom (0 = following live output).
    pub display_offset: usize,
    /// Number of scrollback lines available above the viewport.
    pub history_size: usize,
    pub title: Option<String>,
    pub mode: ModeFlags,
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    pub cursor_color: Rgb,
}

impl GridSnapshot {
    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        if row < self.rows && col < self.cols {
            self.cells.get(row * self.cols + col)
        } else {
            None
        }
    }

    pub fn row(&self, row: usize) -> &[Cell] {
        if row < self.rows {
            &self.cells[row * self.cols..(row + 1) * self.cols]
        } else {
            &[]
        }
    }

    /// Text of one row with trailing blanks trimmed.
    pub fn line_text(&self, row: usize) -> String {
        let mut s = String::new();
        for c in self.row(row) {
            if c.flags.contains(CellFlags::WIDE_SPACER) {
                continue;
            }
            s.push(c.ch);
            if let Some(z) = &c.zerowidth {
                s.extend(z.iter());
            }
        }
        s.truncate(s.trim_end().len());
        s
    }

    /// Visible text, rows joined with `\n`, trailing blank rows removed.
    pub fn to_text(&self) -> String {
        let mut lines: Vec<String> = (0..self.rows).map(|r| self.line_text(r)).collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }
}
