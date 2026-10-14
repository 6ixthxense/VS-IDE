use regex::{Regex, RegexBuilder};

use crate::options::SearchOptions;

/// Port of `normalizePathForMatch`.
pub fn normalize_path_for_match(value: &str) -> String {
    value.replace('\\', "/")
}

/// Port of `splitPatterns`: split on newlines, commas and semicolons, trim, drop empties.
pub fn split_patterns(value: &str) -> Vec<String> {
    value
        .split(['\n', ',', ';'])
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .map(str::to_string)
        .collect()
}

/// Port of `globToRegExp`: `*` matches any run of characters (including `/`), `?` matches
/// one character, everything else is literal. Anchored and case-insensitive.
fn glob_to_regex(pattern: &str) -> Option<Regex> {
    let mut source = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => source.push_str(".*"),
            '?' => source.push('.'),
            other => {
                let mut buf = [0u8; 4];
                source.push_str(&regex::escape(other.encode_utf8(&mut buf)));
            }
        }
    }
    source.push('$');
    RegexBuilder::new(&source)
        .case_insensitive(true)
        .dot_matches_new_line(true)
        .build()
        .ok()
}

fn compile_patterns(text: &str) -> Vec<Regex> {
    split_patterns(text)
        .iter()
        .filter_map(|pattern| glob_to_regex(&normalize_path_for_match(pattern)))
        .collect()
}

/// Pre-compiled include/exclude filter (compile once, test many paths).
#[derive(Clone, Debug)]
pub struct PathFilter {
    has_include: bool,
    include: Vec<Regex>,
    exclude: Vec<Regex>,
}

impl PathFilter {
    pub fn new(options: &SearchOptions) -> Self {
        let options = options.normalized();
        Self {
            has_include: !split_patterns(&options.include_pattern).is_empty(),
            include: compile_patterns(&options.include_pattern),
            exclude: compile_patterns(&options.exclude_pattern),
        }
    }

    /// `relative_path` is relative to the workspace root; separators are normalized here.
    pub fn should_search(&self, relative_path: &str) -> bool {
        let path = normalize_path_for_match(relative_path);
        if self.has_include && !self.include.iter().any(|re| re.is_match(&path)) {
            return false;
        }
        !self.exclude.iter().any(|re| re.is_match(&path))
    }
}

/// Port of `shouldSearchPath`.
pub fn should_search_path(relative_path: &str, options: &SearchOptions) -> bool {
    PathFilter::new(options).should_search(relative_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn include(pattern: &str) -> SearchOptions {
        SearchOptions {
            include_pattern: pattern.into(),
            ..Default::default()
        }
    }

    fn exclude(pattern: &str) -> SearchOptions {
        SearchOptions {
            exclude_pattern: pattern.into(),
            ..Default::default()
        }
    }

    #[test]
    fn evaluates_include_and_exclude_patterns_against_normalized_relative_paths() {
        assert!(should_search_path("src/app.tsx", &include("src/*")));
        assert!(!should_search_path("docs/readme.md", &include("src/*")));
        assert!(!should_search_path("dist/bundle.js", &exclude("dist/*")));
        assert!(should_search_path("src\\app.tsx", &include("src/*")));
    }

    #[test]
    fn splits_patterns_on_commas_semicolons_and_newlines() {
        assert_eq!(
            split_patterns(" a, b;c\n d ,, "),
            vec!["a", "b", "c", "d"]
        );
        assert!(!should_search_path("docs/a.md", &exclude("dist/*, docs/*")));
        assert!(!should_search_path("dist/a.js", &exclude("dist/*, docs/*")));
        assert!(should_search_path("src/a.js", &exclude("dist/*, docs/*")));
    }

    #[test]
    fn globs_are_case_insensitive_and_star_crosses_directories() {
        assert!(should_search_path("SRC/Deep/er/File.TS", &include("src/*.ts")));
        assert!(should_search_path("a/b.c", &include("a/?.c")));
        assert!(!should_search_path("a/bb.c", &include("a/?.c")));
        // Regex metacharacters are literal.
        assert!(should_search_path("a+b.txt", &include("a+b.txt")));
        assert!(!should_search_path("aab.txt", &include("a+b.txt")));
    }
}
