//! Span based text edits (port of `src/renderer/utils/textEdits.ts`).

/// A replacement of `length` units starting at `start` with `new_text`.
///
/// Offsets are expressed in **UTF-16 code units**, matching the TypeScript
/// language service and LSP. Use [`apply_text_span_edits_utf8`] for byte
/// offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSpanEdit {
    /// Start offset.
    pub start: usize,
    /// Number of units to replace.
    pub length: usize,
    /// Replacement text.
    pub new_text: String,
}

impl TextSpanEdit {
    /// Convenience constructor.
    pub fn new(start: usize, length: usize, new_text: impl Into<String>) -> Self {
        Self {
            start,
            length,
            new_text: new_text.into(),
        }
    }
}

/// Order edits so they can be applied back-to-front without invalidating
/// earlier offsets: by `start` descending, then `length` descending (stable).
fn back_to_front(edits: &[TextSpanEdit]) -> Vec<&TextSpanEdit> {
    let mut sorted: Vec<&TextSpanEdit> = edits.iter().collect();
    sorted.sort_by(|l, r| r.start.cmp(&l.start).then(r.length.cmp(&l.length)));
    sorted
}

/// Apply `edits` to `content`, interpreting offsets as UTF-16 code units.
///
/// Edits are applied from the end of the document backwards. Offsets past the
/// end are clamped, like `String.prototype.slice`. An edit that splits a
/// surrogate pair yields U+FFFD for the orphaned half.
pub fn apply_text_span_edits(content: &str, edits: &[TextSpanEdit]) -> String {
    let mut units: Vec<u16> = content.encode_utf16().collect();
    for edit in back_to_front(edits) {
        let start = edit.start.min(units.len());
        let end = start.saturating_add(edit.length).min(units.len());
        units.splice(start..end, edit.new_text.encode_utf16());
    }
    String::from_utf16_lossy(&units)
}

/// Like [`apply_text_span_edits`] but with UTF-8 **byte** offsets, as used by
/// Rust text buffers. Offsets are clamped to the string length and rounded
/// down to the nearest char boundary.
pub fn apply_text_span_edits_utf8(content: &str, edits: &[TextSpanEdit]) -> String {
    let mut text = content.to_owned();
    for edit in back_to_front(edits) {
        let start = floor_boundary(&text, edit.start);
        let end = floor_boundary(&text, edit.start.saturating_add(edit.length)).max(start);
        text.replace_range(start..end, &edit.new_text);
    }
    text
}

fn floor_boundary(text: &str, index: usize) -> usize {
    let mut i = index.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_multiple_edits_from_the_end_of_the_document_backwards() {
        let out = apply_text_span_edits(
            "alpha beta gamma",
            &[
                TextSpanEdit::new(0, 5, "omega"),
                TextSpanEdit::new(11, 5, "delta"),
            ],
        );
        assert_eq!(out, "omega beta delta");
    }

    #[test]
    fn supports_repeated_replacements_in_the_same_document() {
        let out = apply_text_span_edits(
            "foo + foo + foo",
            &[
                TextSpanEdit::new(0, 3, "bar"),
                TextSpanEdit::new(6, 3, "bar"),
                TextSpanEdit::new(12, 3, "bar"),
            ],
        );
        assert_eq!(out, "bar + bar + bar");
    }

    #[test]
    fn offsets_are_utf16_units_and_clamped() {
        // U+1F600 is two UTF-16 units.
        let out = apply_text_span_edits("a\u{1F600}b", &[TextSpanEdit::new(3, 1, "X")]);
        assert_eq!(out, "a\u{1F600}X");
        assert_eq!(
            apply_text_span_edits("abc", &[TextSpanEdit::new(10, 5, "!")]),
            "abc!"
        );
    }

    #[test]
    fn utf8_variant_uses_byte_offsets() {
        let out = apply_text_span_edits_utf8("a\u{1F600}b", &[TextSpanEdit::new(5, 1, "X")]);
        assert_eq!(out, "a\u{1F600}X");
    }
}
