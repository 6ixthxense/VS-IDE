//! vsm-search: workspace text search and Quick Open fuzzy matching.
//!
//! * [`search_workspace`] / [`search_streaming`] / [`spawn_search`]: text search built
//!   on `ignore` (parallel directory walk) and `grep-searcher` (no whole-file strings),
//!   with a result cap and cancellation.
//! * [`replace_in_workspace`]: search-and-replace across the same candidate files.
//! * [`build_quick_open_items`]: fuzzy file ranking for Quick Open (`nucleo-matcher`).
//!
//! The crate has no dependency on other workspace crates; callers pass a root path
//! or a file list.

mod engine;
mod error;
mod glob;
mod locale;
mod options;
mod quick_open;
mod replace;
mod scope;
mod utils;

pub use engine::{
    search_streaming, search_workspace, search_workspace_cancellable, spawn_search, SearchHandle,
    SearchMatch, SearchSummary,
};
pub use error::{Result, SearchError};
pub use glob::{normalize_path_for_match, should_search_path, split_patterns, PathFilter};
pub use locale::locale_compare;
pub use options::{build_pattern_source, build_search_regex, SearchOptions, MAX_RESULTS};
pub use quick_open::{
    build_quick_open_items, find_fuzzy_match, find_fuzzy_match_greedy, get_relative_quick_open_path,
    parse_quick_open_query, FuzzyMatch, FuzzyMatcher, ParsedQuickOpenQuery, QuickOpenItem,
    QuickOpenResult, DEFAULT_LIMIT as QUICK_OPEN_DEFAULT_LIMIT,
};
pub use replace::{replace_in_workspace, ReplaceFailure, ReplaceReport};
pub use scope::{normalize_lexical, should_skip_directory, SearchRequest};
pub use utils::{
    build_replace_preview, group_search_results, touch_search_history, SearchResultGroup,
    MAX_SEARCH_HISTORY,
};
