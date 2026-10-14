//! Approximation of `String.prototype.localeCompare` (ICU root collation) for
//! file names: punctuation < digits < letters, letters compared case-insensitively
//! first, lowercase before uppercase as a tie-breaker.

use std::cmp::Ordering;

const PUNCTUATION_ORDER: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

fn primary_key(c: char) -> (u8, u32) {
    if c.is_whitespace() {
        (0, c as u32)
    } else if let Some(pos) = PUNCTUATION_ORDER.find(c) {
        (1, pos as u32)
    } else if c.is_ascii_digit() {
        (2, c as u32)
    } else if c.is_alphabetic() {
        (3, c.to_lowercase().next().unwrap_or(c) as u32)
    } else {
        (1, 1000 + c as u32)
    }
}

/// Compare two strings the way `a.localeCompare(b)` roughly orders them.
pub fn locale_compare(left: &str, right: &str) -> Ordering {
    let primary = left
        .chars()
        .map(primary_key)
        .cmp(right.chars().map(primary_key));
    if primary != Ordering::Equal {
        return primary;
    }

    for (l, r) in left.chars().zip(right.chars()) {
        if l != r {
            // Lowercase sorts before uppercase.
            return match (l.is_lowercase(), r.is_lowercase()) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => l.cmp(&r),
            };
        }
    }
    left.cmp(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_like_icu_for_common_names() {
        assert_eq!(locale_compare("a", "B"), Ordering::Less);
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
        assert_eq!(locale_compare("src/a", "src/b"), Ordering::Less);
        assert_eq!(locale_compare("file2", "file10"), Ordering::Greater);
        assert_eq!(locale_compare("_x", "1"), Ordering::Less);
        assert_eq!(locale_compare("same", "same"), Ordering::Equal);
    }
}
