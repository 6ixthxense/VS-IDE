//! Quick Open fuzzy file matching (port of `renderer/features/quick-open/utils.ts`).
//!
//! Matching positions come from `nucleo-matcher`; the *ranking* keeps the TS scoring
//! formula (filename match beats path match, recent files get a boost, shorter paths win)
//! computed from the matched positions. The original greedy subsequence matcher is kept
//! as [`find_fuzzy_match_greedy`] for reference and parity tests.

use std::collections::{HashMap, HashSet};

use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::glob::strip_root_prefix;
use crate::locale::locale_compare;

/// Default number of results (`limit = 12` in the TS function).
pub const DEFAULT_LIMIT: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedQuickOpenQuery {
    pub file_query: String,
    pub line_number: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickOpenItem {
    pub path: String,
    pub relative_path: String,
    pub file_name: String,
    pub is_recent: bool,
    pub score: i64,
    /// Character (not UTF-16) indexes of the matched characters in `file_name`.
    pub file_name_matches: Vec<usize>,
    /// Character indexes of the matched characters in `relative_path`.
    pub path_matches: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickOpenResult {
    pub parsed: ParsedQuickOpenQuery,
    pub items: Vec<QuickOpenItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub score: i64,
    pub indexes: Vec<usize>,
}

/// Port of `getRelativeQuickOpenPath`.
///
/// The root only counts as a prefix at a path-separator boundary, so `C:\workspace-old\a.ts`
/// is not treated as being under `C:\workspace`.
pub fn get_relative_quick_open_path(file_path: &str, root_path: Option<&str>) -> String {
    match root_path.and_then(|root| strip_root_prefix(file_path, root)) {
        Some(rest) => rest.replace('\\', "/"),
        None => file_path.replace('\\', "/"),
    }
}

/// Port of `parseQuickOpenQuery`: an optional `:<line>` suffix.
pub fn parse_quick_open_query(raw_query: &str) -> ParsedQuickOpenQuery {
    let trimmed = raw_query.trim();
    if trimmed.is_empty() {
        return ParsedQuickOpenQuery {
            file_query: String::new(),
            line_number: None,
        };
    }

    if let Some((file_part, digits)) = trimmed.rsplit_once(':') {
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            return ParsedQuickOpenQuery {
                file_query: file_part.trim().to_string(),
                line_number: digits.parse::<u64>().ok().filter(|n| *n > 0),
            };
        }
    }
    ParsedQuickOpenQuery {
        file_query: trimmed.to_string(),
        line_number: None,
    }
}

/// The TS score formula, computed from the lowercase text and the matched positions.
fn score_match(text: &str, query: &str, indexes: &[usize]) -> i64 {
    let text_len = text.chars().count() as i64;
    let query_len = query.chars().count() as i64;
    let gaps: i64 = indexes
        .windows(2)
        .map(|pair| (pair[1] as i64 - pair[0] as i64 - 1).max(0))
        .sum();
    let first = indexes.first().copied().unwrap_or(0) as i64;

    let starts_with = text.starts_with(query);
    let includes = text.contains(query);
    (if starts_with { 140 } else { 0 })
        + (if includes { 60 } else { 0 })
        + (80 - first * 2).max(0)
        + (100 - gaps * 8).max(0)
        + (40 - (text_len - query_len).max(0)).max(0)
}

/// Exact port of the TS greedy subsequence matcher (`findFuzzyMatch`).
pub fn find_fuzzy_match_greedy(text: &str, query: &str) -> Option<FuzzyMatch> {
    let text = text.to_lowercase();
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            indexes: Vec::new(),
        });
    }

    let query_chars: Vec<char> = query.chars().collect();
    let mut indexes = Vec::with_capacity(query_chars.len());
    let mut query_index = 0;
    for (text_index, ch) in text.chars().enumerate() {
        if ch == query_chars[query_index] {
            indexes.push(text_index);
            query_index += 1;
            if query_index == query_chars.len() {
                break;
            }
        }
    }
    if query_index != query_chars.len() {
        return None;
    }
    let score = score_match(&text, &query, &indexes);
    Some(FuzzyMatch { score, indexes })
}

/// Reusable fuzzy matcher (keeps nucleo's scratch buffers between calls).
pub struct FuzzyMatcher {
    matcher: Matcher,
    haystack_buf: Vec<char>,
    needle_buf: Vec<char>,
    indices: Vec<u32>,
}

impl Default for FuzzyMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl FuzzyMatcher {
    pub fn new() -> Self {
        // Latin normalization is off: nucleo only normalizes the haystack when matching
        // through `Utf32Str`, so "café" would never match a "café.ts" file name.
        let mut config = Config::DEFAULT.match_paths();
        config.normalize = false;
        Self {
            matcher: Matcher::new(config),
            haystack_buf: Vec::new(),
            needle_buf: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Fuzzy-match `query` against `text` (case-insensitive subsequence match, with the
    /// best alignment chosen by nucleo). The query is trimmed; an empty query matches
    /// everything with score 0, like the TS implementation.
    pub fn find(&mut self, text: &str, query: &str) -> Option<FuzzyMatch> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Some(FuzzyMatch {
                score: 0,
                indexes: Vec::new(),
            });
        }

        self.indices.clear();
        let haystack = Utf32Str::new(text, &mut self.haystack_buf);
        let needle = Utf32Str::new(&query, &mut self.needle_buf);
        self.matcher
            .fuzzy_indices(haystack, needle, &mut self.indices)?;

        let mut indexes: Vec<usize> = self.indices.iter().map(|i| *i as usize).collect();
        indexes.sort_unstable();
        indexes.dedup();
        let score = score_match(&text.to_lowercase(), &query, &indexes);
        Some(FuzzyMatch { score, indexes })
    }
}

/// Port of `findFuzzyMatch` using nucleo for the match positions.
pub fn find_fuzzy_match(text: &str, query: &str) -> Option<FuzzyMatch> {
    FuzzyMatcher::new().find(text, query)
}

/// Port of `buildQuickOpenItems`.
///
/// `files` are absolute paths (as strings, any separator style), `recent_files` is
/// ordered most recent first. Sorting is score descending, then recent files, then
/// relative path.
pub fn build_quick_open_items<S: AsRef<str>, R: AsRef<str>>(
    files: &[S],
    root_path: Option<&str>,
    raw_query: &str,
    recent_files: &[R],
    limit: usize,
) -> QuickOpenResult {
    let parsed = parse_quick_open_query(raw_query);
    let recent_weights: HashMap<&str, i64> = recent_files
        .iter()
        .enumerate()
        .map(|(index, path)| (path.as_ref(), (recent_files.len() - index) as i64))
        .collect();

    let mut matcher = FuzzyMatcher::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut items: Vec<QuickOpenItem> = Vec::new();

    for file in files {
        let file_path = file.as_ref();
        if !seen.insert(file_path) {
            continue;
        }

        let relative_path = get_relative_quick_open_path(file_path, root_path);
        let file_name = match relative_path.rsplit('/').next() {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => relative_path.clone(),
        };
        let file_name_match = matcher.find(&file_name, &parsed.file_query);
        let path_match = if file_name_match.is_some() {
            None
        } else {
            matcher.find(&relative_path, &parsed.file_query)
        };

        if !parsed.file_query.is_empty() && file_name_match.is_none() && path_match.is_none() {
            continue;
        }

        let recent_boost = recent_weights.get(file_path).copied().unwrap_or(0) * 12;
        let match_score = match (&file_name_match, &path_match) {
            (Some(m), _) => m.score + 220,
            (None, Some(m)) => m.score + 80,
            (None, None) => 40,
        };
        let length_penalty = relative_path.chars().count().min(120) as i64;

        items.push(QuickOpenItem {
            path: file_path.to_string(),
            relative_path,
            file_name,
            is_recent: recent_weights.contains_key(file_path),
            score: match_score + recent_boost - length_penalty,
            file_name_matches: file_name_match.map(|m| m.indexes).unwrap_or_default(),
            path_matches: path_match.map(|m| m.indexes).unwrap_or_default(),
        });
    }

    items.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.is_recent.cmp(&left.is_recent))
            .then_with(|| locale_compare(&left.relative_path, &right.relative_path))
    });
    items.truncate(limit);

    QuickOpenResult { parsed, items }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "C:\\workspace";

    fn files() -> Vec<&'static str> {
        vec![
            "C:\\workspace\\src\\AppShell.tsx",
            "C:\\workspace\\src\\api\\accountService.ts",
            "C:\\workspace\\docs\\architecture.md",
        ]
    }

    #[test]
    fn parses_optional_line_jump_suffixes_from_the_query() {
        assert_eq!(
            parse_quick_open_query("AppShell:120"),
            ParsedQuickOpenQuery {
                file_query: "AppShell".into(),
                line_number: Some(120)
            }
        );
        assert_eq!(
            parse_quick_open_query(":42"),
            ParsedQuickOpenQuery {
                file_query: String::new(),
                line_number: Some(42)
            }
        );
        assert_eq!(
            parse_quick_open_query("service"),
            ParsedQuickOpenQuery {
                file_query: "service".into(),
                line_number: None
            }
        );
    }

    #[test]
    fn parse_handles_edge_cases() {
        assert_eq!(parse_quick_open_query("   ").file_query, "");
        // A zero line number is dropped but the suffix is still stripped.
        let zero = parse_quick_open_query("a.ts:0");
        assert_eq!((zero.file_query.as_str(), zero.line_number), ("a.ts", None));
        // Not purely digits: part of the file query.
        assert_eq!(parse_quick_open_query("a:b").file_query, "a:b");
        assert_eq!(parse_quick_open_query("a: 12").file_query, "a: 12");
        assert_eq!(parse_quick_open_query(" a.ts :7 ").line_number, Some(7));
    }

    #[test]
    fn finds_fuzzy_subsequence_matches_and_tracks_their_indexes() {
        let found = find_fuzzy_match("AppShell.tsx", "ash").expect("match");
        assert_eq!(found.indexes, vec![0, 3, 4]);
        assert!(find_fuzzy_match("AppShell.tsx", "xyz").is_none());

        let greedy = find_fuzzy_match_greedy("AppShell.tsx", "ash").expect("match");
        assert_eq!(greedy.indexes, vec![0, 3, 4]);
        assert!(find_fuzzy_match_greedy("AppShell.tsx", "xyz").is_none());
    }

    #[test]
    fn greedy_scoring_matches_the_ts_formula() {
        // startsWith(140) + includes(60) + (80-0) + (100-0) + max(0, 40-(12-3))
        let found = find_fuzzy_match_greedy("AppShell.tsx", "app").expect("match");
        assert_eq!(found.indexes, vec![0, 1, 2]);
        assert_eq!(found.score, 140 + 60 + 80 + 100 + 31);
        // "ash": no startsWith/includes; 80 + (100 - 1*... gaps: 2 + 0) + 31
        let found = find_fuzzy_match_greedy("AppShell.tsx", "ash").expect("match");
        assert_eq!(found.score, 80 + (100 - 2 * 8) + (40 - 9));
    }

    #[test]
    fn empty_query_matches_with_zero_score() {
        let found = find_fuzzy_match("anything", "  ").expect("match");
        assert_eq!(found.score, 0);
        assert!(found.indexes.is_empty());
    }

    #[test]
    fn nucleo_and_greedy_accept_the_same_candidates() {
        let texts = [
            "AppShell.tsx",
            "src/api/accountService.ts",
            "docs/architecture.md",
            "README",
            "a",
        ];
        for text in texts {
            for query in ["a", "as", "acc", "arch", "zz", "tsx", "srv", "ARCH", "md"] {
                assert_eq!(
                    find_fuzzy_match(text, query).is_some(),
                    find_fuzzy_match_greedy(text, query).is_some(),
                    "{text} / {query}"
                );
            }
        }
    }

    #[test]
    fn nucleo_indexes_are_a_valid_subsequence() {
        let text = "src/components/FileExplorerPanel.tsx";
        let found = find_fuzzy_match(text, "fexp").expect("match");
        let chars: Vec<char> = text.to_lowercase().chars().collect();
        let query: Vec<char> = "fexp".chars().collect();
        assert_eq!(found.indexes.len(), query.len());
        for (position, expected) in found.indexes.iter().zip(&query) {
            assert_eq!(chars[*position], *expected);
        }
        assert!(found.indexes.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn boosts_recent_files_and_filename_matches_when_building_results() {
        let recent = ["C:\\workspace\\src\\api\\accountService.ts"];
        let result = build_quick_open_items(&files(), Some(ROOT), "acc", &recent, DEFAULT_LIMIT);

        assert_eq!(result.items[0].file_name, "accountService.ts");
        assert!(result.items[0].is_recent);
        assert_eq!(result.items[0].relative_path, "src/api/accountService.ts");
        assert_eq!(result.items[0].file_name_matches, vec![0, 1, 2]);
    }

    #[test]
    fn filename_matches_outrank_path_only_matches() {
        let list = [
            "C:\\workspace\\docs\\notes\\util.md",
            "C:\\workspace\\util\\readme.md",
        ];
        let none: [&str; 0] = [];
        let result = build_quick_open_items(&list, Some(ROOT), "util", &none, DEFAULT_LIMIT);
        assert_eq!(result.items[0].relative_path, "docs/notes/util.md");
        assert!(!result.items[0].file_name_matches.is_empty());
        assert!(result.items[1].file_name_matches.is_empty());
        assert!(!result.items[1].path_matches.is_empty());
    }

    #[test]
    fn shows_recent_files_first_when_the_query_is_empty() {
        let recent = [
            "C:\\workspace\\docs\\architecture.md",
            "C:\\workspace\\src\\AppShell.tsx",
        ];
        let result = build_quick_open_items(&files(), Some(ROOT), "", &recent, DEFAULT_LIMIT);

        let names: Vec<&str> = result
            .items
            .iter()
            .take(2)
            .map(|item| item.file_name.as_str())
            .collect();
        assert_eq!(names, vec!["architecture.md", "AppShell.tsx"]);
        assert_eq!(result.items.len(), 3);
    }

    #[test]
    fn dedupes_files_limits_results_and_filters_non_matches() {
        let mut list = files();
        list.push("C:\\workspace\\src\\AppShell.tsx");
        let none: [&str; 0] = [];
        let all = build_quick_open_items(&list, Some(ROOT), "", &none, DEFAULT_LIMIT);
        assert_eq!(all.items.len(), 3);
        let limited = build_quick_open_items(&list, Some(ROOT), "", &none, 2);
        assert_eq!(limited.items.len(), 2);
        let filtered = build_quick_open_items(&list, Some(ROOT), "zzz", &none, DEFAULT_LIMIT);
        assert!(filtered.items.is_empty());
    }

    #[test]
    fn query_with_line_suffix_still_matches_the_file() {
        let none: [&str; 0] = [];
        let result = build_quick_open_items(&files(), Some(ROOT), "shell:120", &none, 5);
        assert_eq!(result.parsed.line_number, Some(120));
        assert_eq!(result.items[0].file_name, "AppShell.tsx");
    }

    #[test]
    fn matches_non_ascii_file_names() {
        let none: [&str; 0] = [];
        let list = [
            "C:\\workspace\\café.ts",
            "C:\\workspace\\Ärger.ts",
            "C:\\workspace\\ทดสอบ.ts",
            "C:\\workspace\\plain.ts",
        ];
        for (query, expected) in [("café", "café.ts"), ("ärger", "Ärger.ts"), ("ÄRGER", "Ärger.ts"), ("ทดสอบ", "ทดสอบ.ts")] {
            let result = build_quick_open_items(&list, Some(ROOT), query, &none, DEFAULT_LIMIT);
            assert_eq!(result.items.len(), 1, "{query}");
            assert_eq!(result.items[0].file_name, expected);
            assert_eq!(result.items[0].file_name_matches.len(), query.chars().count());
            // Parity with the greedy matcher.
            assert!(find_fuzzy_match_greedy(expected, query).is_some());
            assert_eq!(
                find_fuzzy_match(expected, query).map(|m| m.indexes),
                find_fuzzy_match_greedy(expected, query).map(|m| m.indexes)
            );
        }
        // No accent folding: "cafe" does not match "café".
        assert!(find_fuzzy_match("café.ts", "cafe").is_none());
    }

    #[test]
    fn root_prefix_must_end_at_a_separator() {
        assert_eq!(
            get_relative_quick_open_path("C:\\workspace-old\\a.ts", Some(ROOT)),
            "C:/workspace-old/a.ts"
        );
        assert_eq!(get_relative_quick_open_path("C:\\workspace\\a.ts", Some("C:\\workspace\\")), "a.ts");
    }

    #[test]
    fn relative_paths_are_normalized() {
        assert_eq!(
            get_relative_quick_open_path("C:\\workspace\\src\\a.ts", Some(ROOT)),
            "src/a.ts"
        );
        assert_eq!(
            get_relative_quick_open_path("D:\\other\\a.ts", Some(ROOT)),
            "D:/other/a.ts"
        );
        assert_eq!(get_relative_quick_open_path("/x/y.ts", None), "/x/y.ts");
    }
}
