use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use regex::{Regex, RegexBuilder};

use crate::error::{Result, SearchError};

/// Default cap on the number of reported matches (`MAX_RESULTS` in the TS service).
pub const MAX_RESULTS: usize = 200;

/// Search options, mirroring `SearchOptions` from `shared/types/ipc.ts`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    /// Glob list separated by newlines, commas or semicolons. Empty means "everything".
    pub include_pattern: String,
    /// Glob list separated by newlines, commas or semicolons.
    pub exclude_pattern: String,
}

impl SearchOptions {
    /// Port of `normalizeSearchOptions`: glob patterns are trimmed.
    pub fn normalized(&self) -> SearchOptions {
        SearchOptions {
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            use_regex: self.use_regex,
            include_pattern: self.include_pattern.trim().to_string(),
            exclude_pattern: self.exclude_pattern.trim().to_string(),
        }
    }
}

/// Regex source used for a query: the escaped (or raw) query, wrapped in `\b(?:..)\b`
/// when whole-word matching is requested. Same construction as `buildSearchRegExp`.
pub fn build_pattern_source(query: &str, options: &SearchOptions) -> String {
    let source = if options.use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    if options.whole_word {
        format!(r"\b(?:{source})\b")
    } else {
        source
    }
}

/// Port of `buildSearchRegExp` (without the JS `g` flag: use `replace_all`/`find_iter`).
pub fn build_search_regex(query: &str, options: &SearchOptions) -> Result<Regex> {
    let source = build_pattern_source(query, options);
    RegexBuilder::new(&source)
        .case_insensitive(!options.case_sensitive)
        .build()
        .map_err(|error| SearchError::InvalidPattern(error.to_string()))
}

/// Line oriented matcher for `grep-searcher`. The line terminator is declared so
/// that patterns can never match across lines (the TS version splits on `\n`).
pub(crate) fn build_line_matcher(query: &str, options: &SearchOptions) -> Result<RegexMatcher> {
    let source = build_pattern_source(query, options);
    RegexMatcherBuilder::new()
        .case_insensitive(!options.case_sensitive)
        .line_terminator(Some(b'\n'))
        .build(&source)
        .map_err(|error| SearchError::InvalidPattern(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_queries_are_escaped() {
        let options = SearchOptions::default();
        let regex = build_search_regex("a.b(c)", &options).expect("regex");
        assert!(regex.is_match("xa.b(c)x"));
        assert!(!regex.is_match("aXb(c)"));
    }

    #[test]
    fn whole_word_wraps_the_pattern() {
        let options = SearchOptions {
            whole_word: true,
            ..Default::default()
        };
        assert_eq!(build_pattern_source("hello", &options), r"\b(?:hello)\b");
        let regex = build_search_regex("hello", &options).expect("regex");
        assert!(regex.is_match("say Hello!"));
        assert!(!regex.is_match("helloWorld"));
    }

    #[test]
    fn invalid_regex_is_reported() {
        let options = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        assert!(matches!(
            build_search_regex("(", &options),
            Err(SearchError::InvalidPattern(_))
        ));
    }

    #[test]
    fn case_sensitivity_is_honoured() {
        let options = SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        let regex = build_search_regex("Hello", &options).expect("regex");
        assert!(regex.is_match("Hello"));
        assert!(!regex.is_match("hello"));
    }
}
