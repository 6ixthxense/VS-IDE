//! Ports of `renderer/features/search/utils.ts` (the parts that are not browser storage).

use std::collections::HashMap;

use regex::Captures;
use vsm_core::paths::strip_root_prefix;

use crate::engine::SearchMatch;
use crate::glob::normalize_path_for_match;
use crate::options::{build_search_regex, SearchOptions};

pub const MAX_SEARCH_HISTORY: usize = 10;

/// Matches of one file (port of `SearchResultGroup`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResultGroup {
    pub path: String,
    pub name: String,
    pub relative_path: String,
    pub matches: Vec<SearchMatch>,
}

/// Port of `touchSearchHistory`: most recent query first, de-duplicated, capped at 10.
pub fn touch_search_history(items: &[String], query: &str) -> Vec<String> {
    let query = query.trim();
    if query.is_empty() {
        return items.to_vec();
    }
    std::iter::once(query.to_string())
        .chain(items.iter().filter(|item| item.as_str() != query).cloned())
        .take(MAX_SEARCH_HISTORY)
        .collect()
}

/// Port of `groupSearchResults`: group by file, preserving first-seen order.
pub fn group_search_results(
    results: &[SearchMatch],
    root_path: Option<&str>,
) -> Vec<SearchResultGroup> {
    let mut groups: Vec<SearchResultGroup> = Vec::new();
    let mut index_by_path: HashMap<String, usize> = HashMap::new();

    for result in results {
        let path = result.path.to_string_lossy().into_owned();
        let index = match index_by_path.get(&path) {
            Some(index) => *index,
            None => {
                let relative = match root_path.and_then(|root| strip_root_prefix(&path, root)) {
                    Some(rest) => normalize_path_for_match(rest),
                    None => normalize_path_for_match(&path),
                };
                groups.push(SearchResultGroup {
                    path: path.clone(),
                    name: result.name.clone(),
                    relative_path: relative,
                    matches: Vec::new(),
                });
                index_by_path.insert(path, groups.len() - 1);
                groups.len() - 1
            }
        };
        groups[index].matches.push(result.clone());
    }
    groups
}

/// Expand a JavaScript `String.prototype.replace` replacement template
/// (`$$`, `$&`, `` $` ``, `$'`, `$1`..`$99`, `$<name>`).
fn expand_js_template(
    template: &str,
    caps: &Captures<'_>,
    haystack: &str,
    has_named_groups: bool,
    out: &mut String,
) {
    let whole = caps.get(0).map(|m| (m.start(), m.end())).unwrap_or((0, 0));
    let group_count = caps.len() - 1;
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;

    let push_group = |n: usize, out: &mut String| {
        if let Some(m) = caps.get(n) {
            out.push_str(m.as_str());
        }
    };

    while i < chars.len() {
        let c = chars[i];
        if c != '$' || i + 1 >= chars.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let next = chars[i + 1];
        match next {
            '$' => {
                out.push('$');
                i += 2;
            }
            '&' => {
                out.push_str(&haystack[whole.0..whole.1]);
                i += 2;
            }
            '`' => {
                out.push_str(&haystack[..whole.0]);
                i += 2;
            }
            '\'' => {
                out.push_str(&haystack[whole.1..]);
                i += 2;
            }
            d if d.is_ascii_digit() => {
                let one = d.to_digit(10).unwrap_or(0) as usize;
                let two = chars
                    .get(i + 2)
                    .filter(|c| c.is_ascii_digit())
                    .map(|c| one * 10 + c.to_digit(10).unwrap_or(0) as usize);
                match two {
                    Some(n) if (1..=group_count).contains(&n) => {
                        push_group(n, out);
                        i += 3;
                    }
                    _ if (1..=group_count).contains(&one) => {
                        push_group(one, out);
                        i += 2;
                    }
                    _ => {
                        out.push('$');
                        i += 1;
                    }
                }
            }
            '<' if has_named_groups => {
                if let Some(close) = chars[i + 2..].iter().position(|c| *c == '>') {
                    let name: String = chars[i + 2..i + 2 + close].iter().collect();
                    if let Some(m) = caps.name(&name) {
                        out.push_str(m.as_str());
                    }
                    i += close + 3;
                } else {
                    out.push('$');
                    i += 1;
                }
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
}

/// Port of `buildReplacePreview`: the line with every match replaced, or an empty
/// string when there is nothing to preview.
pub fn build_replace_preview(
    match_line: &str,
    query: &str,
    replacement: &str,
    options: &SearchOptions,
) -> String {
    if query.trim().is_empty() || replacement.is_empty() {
        return String::new();
    }
    let Ok(regex) = build_search_regex(query, options) else {
        return String::new();
    };
    let preview = apply_js_replacement(&regex, match_line, replacement);
    if preview == match_line {
        String::new()
    } else {
        preview
    }
}

/// Replace every match of `regex` in `text` using a JavaScript replacement template
/// (`$&`, `$1`, `$<name>`, ...). Shared by the preview and by the actual replace so the
/// two always agree.
pub(crate) fn apply_js_replacement(regex: &regex::Regex, text: &str, replacement: &str) -> String {
    let has_named_groups = regex.capture_names().flatten().next().is_some();
    regex
        .replace_all(text, |caps: &Captures<'_>| {
            let mut out = String::new();
            expand_js_template(replacement, caps, text, has_named_groups, &mut out);
            out
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn hit(path: &str, name: &str, line: u64, text: &str) -> SearchMatch {
        SearchMatch {
            path: PathBuf::from(path),
            name: name.into(),
            line,
            text: text.into(),
        }
    }

    #[test]
    fn groups_search_results_by_file_path_while_preserving_match_order() {
        let groups = group_search_results(
            &[
                hit("C:/workspace/src/app.ts", "app.ts", 3, "hello world"),
                hit("C:/workspace/src/app.ts", "app.ts", 8, "hello again"),
                hit("C:/workspace/docs/readme.md", "readme.md", 2, "hello docs"),
            ],
            Some("C:/workspace"),
        );

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].path, "C:/workspace/src/app.ts");
        assert_eq!(groups[0].relative_path, "src/app.ts");
        assert_eq!(
            groups[0].matches.iter().map(|m| m.line).collect::<Vec<_>>(),
            vec![3, 8]
        );
        assert_eq!(groups[1].relative_path, "docs/readme.md");
    }

    #[test]
    fn group_without_root_keeps_the_normalized_full_path() {
        let groups = group_search_results(&[hit("a\\b.ts", "b.ts", 1, "x")], None);
        assert_eq!(groups[0].relative_path, "a/b.ts");
    }

    #[test]
    fn builds_a_replace_preview_from_the_active_search_options() {
        let preview = build_replace_preview(
            "const helloWorld = \"hello\";",
            "hello",
            "updated",
            &SearchOptions::default(),
        );
        assert_eq!(preview, "const updatedWorld = \"updated\";");
    }

    #[test]
    fn replace_preview_is_empty_without_change_or_replacement() {
        let options = SearchOptions::default();
        assert_eq!(build_replace_preview("abc", "zzz", "x", &options), "");
        assert_eq!(build_replace_preview("abc", "abc", "", &options), "");
        assert_eq!(build_replace_preview("abc", "  ", "x", &options), "");
        let bad = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        assert_eq!(build_replace_preview("abc", "(", "x", &bad), "");
    }

    #[test]
    fn replace_preview_expands_js_style_templates() {
        let options = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        assert_eq!(
            build_replace_preview("foo=bar", "(\\w+)=(\\w+)", "$2=$1", &options),
            "bar=foo"
        );
        assert_eq!(
            build_replace_preview("foo", "foo", "[$&]$$", &options),
            "[foo]$"
        );
        assert_eq!(build_replace_preview("foo", "foo", "$9", &options), "$9");
    }

    #[test]
    fn group_prefix_requires_a_separator_boundary() {
        let groups = group_search_results(
            &[hit("C:/workspace-old/a.ts", "a.ts", 1, "x")],
            Some("C:/workspace"),
        );
        assert_eq!(groups[0].relative_path, "C:/workspace-old/a.ts");
    }

    #[test]
    fn deduplicates_search_history_while_keeping_the_most_recent_items_first() {
        let history = touch_search_history(
            &["status".into(), "hello".into(), "replace".into()],
            "hello",
        );
        assert_eq!(history, vec!["hello", "status", "replace"]);
    }

    #[test]
    fn history_ignores_blank_queries_and_is_capped() {
        let items: Vec<String> = (0..10).map(|i| format!("q{i}")).collect();
        assert_eq!(touch_search_history(&items, "   "), items);
        let next = touch_search_history(&items, " new ");
        assert_eq!(next.len(), MAX_SEARCH_HISTORY);
        assert_eq!(next[0], "new");
        assert!(!next.contains(&"q9".to_string()));
    }
}
