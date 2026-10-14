//! URI and position helpers (LSP positions are UTF-16 code unit based by default).

use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::{Position, Uri};

use crate::error::LspError;

/// `file://` URI for an absolute path.
pub fn path_to_uri(path: &Path) -> Result<Uri, LspError> {
    let url = url::Url::from_file_path(path)
        .map_err(|_| LspError::InvalidUri(path.display().to_string()))?;
    Uri::from_str(url.as_str()).map_err(|e| LspError::InvalidUri(format!("{}: {e}", url)))
}

/// Path for a `file://` URI.
pub fn uri_to_path(uri: &Uri) -> Result<PathBuf, LspError> {
    let url = url::Url::parse(uri.as_str()).map_err(|e| LspError::InvalidUri(format!("{}: {e}", uri.as_str())))?;
    url.to_file_path().map_err(|_| LspError::InvalidUri(uri.as_str().to_string()))
}

/// LSP position of a byte offset in `text` (columns counted in UTF-16 units).
/// Offsets past the end clamp to the end; offsets inside a char round down.
pub fn position_at(text: &str, byte_offset: usize) -> Position {
    let mut end = byte_offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let col: usize = before[line_start..].chars().map(char::len_utf16).sum();
    Position::new(line as u32, col as u32)
}

/// Byte offset of an LSP position. Columns past the line end clamp to the line end (before the
/// line break); lines past the end clamp to the text length.
pub fn offset_at(text: &str, pos: Position) -> usize {
    let mut start = 0usize;
    for _ in 0..pos.line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return text.len(),
        }
    }
    let line_end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let line = text[start..line_end].trim_end_matches('\r');
    let mut units = 0u32;
    for (i, ch) in line.char_indices() {
        if units >= pos.character {
            return start + i;
        }
        units += ch.len_utf16() as u32;
    }
    start + line.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_utf16() {
        let t = "ab\n𝄞x\nlast";
        assert_eq!(position_at(t, 0), Position::new(0, 0));
        assert_eq!(position_at(t, 3), Position::new(1, 0));
        // '𝄞' is 4 bytes / 2 UTF-16 units
        assert_eq!(position_at(t, 7), Position::new(1, 2));
        assert_eq!(position_at(t, 8), Position::new(1, 3));
        assert_eq!(position_at(t, 1000), Position::new(2, 4));
        assert_eq!(offset_at(t, Position::new(1, 2)), 7);
        assert_eq!(offset_at(t, Position::new(1, 99)), 8);
        assert_eq!(offset_at(t, Position::new(9, 0)), t.len());
        for off in [0, 1, 3, 7, 8, 9, 13] {
            assert_eq!(offset_at(t, position_at(t, off)), off);
        }
    }

    #[test]
    fn uri_roundtrip() {
        let p = std::env::temp_dir().join("a b").join("c#d.ts");
        let u = path_to_uri(&p).unwrap();
        assert!(u.as_str().starts_with("file://"));
        assert!(!u.as_str().contains(' '));
        assert_eq!(uri_to_path(&u).unwrap(), p);
        assert!(path_to_uri(Path::new("relative/x")).is_err());
    }
}
