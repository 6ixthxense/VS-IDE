//! Tiny subsequence fuzzy matcher used by Quick Open and the command palette.
//!
//! This is intentionally dependency-free; the `vsm-search` crate (nucleo) is the
//! intended replacement once it lands.

/// Score `candidate` against `query`. `None` means no match.
///
/// Rules: case-insensitive subsequence; bonuses for consecutive hits, for hits
/// at word starts (after `/ . _ - space` or a lower->upper transition), and for
/// hits inside the file-name part; small penalty for long candidates.
pub fn score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let c: Vec<char> = candidate.chars().collect();
    let name_start = c.iter().rposition(|&ch| ch == '/').map_or(0, |i| i + 1);

    let mut qi = 0;
    let mut total = 0i32;
    let mut prev_hit: Option<usize> = None;
    for (i, &ch) in c.iter().enumerate() {
        if qi == q.len() {
            break;
        }
        let lc = ch.to_lowercase().next().unwrap_or(ch);
        if lc != q[qi] {
            continue;
        }
        let mut s = 10;
        if prev_hit == Some(i.wrapping_sub(1)) && i > 0 {
            s += 15;
        }
        let word_start = i == 0
            || matches!(c[i - 1], '/' | '.' | '_' | '-' | ' ')
            || (c[i - 1].is_lowercase() && ch.is_uppercase());
        if word_start {
            s += 12;
        }
        if i >= name_start {
            s += 6;
        }
        total += s;
        prev_hit = Some(i);
        qi += 1;
    }
    if qi < q.len() {
        return None;
    }
    // Prefer shorter candidates and shallower paths on ties.
    Some(total * 4 - c.len() as i32)
}

/// Rank `items` (display text) by `query`, best first, at most `limit` results.
/// Returns indices into `items`.
pub fn rank<'a>(
    query: &str,
    items: impl Iterator<Item = &'a str>,
    limit: usize,
) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = items
        .enumerate()
        .filter_map(|(i, s)| score(query, s).map(|sc| (sc, i)))
        .collect();
    // Stable: equal scores keep input order.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.truncate(limit);
    scored.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_matches() {
        assert!(score("mrs", "src/main.rs").is_some());
        assert!(score("xyz", "src/main.rs").is_none());
        assert_eq!(score("", "anything"), Some(0));
    }

    #[test]
    fn filename_hits_beat_directory_hits() {
        let items = ["docs/readme-extra/notes.md", "src/readme.md"];
        let ranked = rank("readme", items.iter().copied(), 10);
        assert_eq!(ranked[0], 1);
    }

    #[test]
    fn consecutive_beats_scattered() {
        let a = score("app", "src/app.rs").unwrap();
        let b = score("app", "a_p_p.rs").unwrap();
        assert!(a > b);
    }
}
