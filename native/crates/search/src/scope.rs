//! Candidate file discovery. Mirrors the rules of `workspaceFileIndex.buildIndex`:
//! directories named `node_modules`, `dist`, `dist-electron`, `dist-renderer`, `.git`
//! or starting with `.` are skipped; symlinks are not followed; `.gitignore` is not used.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use ignore::{WalkBuilder, WalkState};

use crate::error::{Result, SearchError};
use crate::glob::PathFilter;
use crate::options::{SearchOptions, MAX_RESULTS};

/// Everything needed to describe one workspace search or replace.
#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub root: PathBuf,
    pub query: String,
    pub options: SearchOptions,
    /// Optional file or directory (inside `root`) restricting the search.
    pub target: Option<PathBuf>,
    /// Stop after this many matches (defaults to [`MAX_RESULTS`]).
    pub max_results: usize,
}

impl SearchRequest {
    pub fn new(root: impl Into<PathBuf>, query: impl Into<String>, options: SearchOptions) -> Self {
        Self {
            root: root.into(),
            query: query.into(),
            options,
            target: None,
            max_results: MAX_RESULTS,
        }
    }

    pub fn with_target(mut self, target: impl Into<PathBuf>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.max_results = max_results;
        self
    }
}

/// Port of `shouldSkipDirectory` from the file index.
pub fn should_skip_directory(name: &str) -> bool {
    matches!(
        name,
        "node_modules" | "dist" | "dist-electron" | "dist-renderer" | ".git"
    ) || name.starts_with('.')
}

/// `path.strip_prefix(root)`, comparing components case-insensitively when `case_insensitive`
/// (Windows). The remainder keeps the original spelling of `path`.
pub(crate) fn strip_prefix_with(path: &Path, root: &Path, case_insensitive: bool) -> Option<PathBuf> {
    if !case_insensitive {
        return path.strip_prefix(root).ok().map(Path::to_path_buf);
    }
    let fold = |c: Component<'_>| c.as_os_str().to_string_lossy().to_lowercase();
    let mut path_components = path.components();
    for root_component in root.components() {
        let component = path_components.next()?;
        if fold(component) != fold(root_component) {
            return None;
        }
    }
    Some(path_components.as_path().to_path_buf())
}

fn strip_prefix_native(path: &Path, root: &Path) -> Option<PathBuf> {
    strip_prefix_with(path, root, cfg!(windows))
}

/// Lexically normalize a path (`.` and `..` resolved, no filesystem access).
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub(crate) enum Target {
    /// Walk a directory (the root itself or a sub directory).
    Dir(PathBuf),
    /// Search exactly one file.
    File(PathBuf),
    /// The target is inside an always-skipped directory: nothing to search.
    Empty,
}

pub(crate) struct Scope {
    pub root: PathBuf,
    pub target: Target,
    pub filter: PathFilter,
}

impl Scope {
    pub fn new(request: &SearchRequest) -> Result<Scope> {
        let root = normalize_lexical(&request.root);
        if !root.is_absolute() {
            return Err(SearchError::RootNotAbsolute(root));
        }
        let filter = PathFilter::new(&request.options);

        let target = match &request.target {
            None => Target::Dir(root.clone()),
            Some(target) => {
                let target = normalize_lexical(target);
                let relative = strip_prefix_native(&target, &root).ok_or_else(|| {
                    SearchError::OutsideRoot {
                        root: root.clone(),
                        target: target.clone(),
                    }
                })?;
                let metadata = std::fs::metadata(&target).map_err(|source| SearchError::Io {
                    path: target.clone(),
                    source,
                })?;
                if metadata.is_file() {
                    Target::File(target)
                } else if relative
                    .components()
                    .any(|c| should_skip_directory(&c.as_os_str().to_string_lossy()))
                {
                    Target::Empty
                } else {
                    Target::Dir(target)
                }
            }
        };

        Ok(Scope {
            root,
            target,
            filter,
        })
    }

    /// Path relative to the root with `/` separators, or `None` if outside the root.
    pub fn relative(&self, path: &Path) -> Option<String> {
        let relative = strip_prefix_native(path, &self.root)?;
        let parts: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        Some(parts.join("/"))
    }

    pub fn accepts(&self, path: &Path) -> bool {
        self.relative(path)
            .is_some_and(|relative| self.filter.should_search(&relative))
    }

    /// Visit every candidate file with a parallel walk. `visit` may be called from
    /// several threads at once; returning `false` stops the whole walk.
    pub fn walk_parallel<F>(&self, cancel: &AtomicBool, visit: F)
    where
        F: Fn(&Path) -> bool + Sync,
    {
        match &self.target {
            Target::Empty => {}
            Target::File(path) => {
                if !cancel.load(Ordering::Relaxed) && self.accepts(path) {
                    visit(path);
                }
            }
            Target::Dir(dir) => {
                let mut builder = WalkBuilder::new(dir);
                builder
                    .standard_filters(false)
                    .hidden(false)
                    .follow_links(false)
                    .filter_entry(|entry| {
                        entry.depth() == 0
                            || !entry.file_type().is_some_and(|t| t.is_dir())
                            || !should_skip_directory(&entry.file_name().to_string_lossy())
                    });
                builder.build_parallel().run(|| {
                    let visit = &visit;
                    Box::new(move |entry| {
                        if cancel.load(Ordering::Relaxed) {
                            return WalkState::Quit;
                        }
                        let Ok(entry) = entry else {
                            return WalkState::Continue;
                        };
                        if !entry.file_type().is_some_and(|t| t.is_file()) {
                            return WalkState::Continue;
                        }
                        if !self.accepts(entry.path()) {
                            return WalkState::Continue;
                        }
                        if visit(entry.path()) {
                            WalkState::Continue
                        } else {
                            WalkState::Quit
                        }
                    })
                });
            }
        }
    }

    /// All candidate files, sorted like the TS file index (`localeCompare`).
    pub fn collect_sorted(&self, cancel: &AtomicBool) -> Result<Vec<PathBuf>> {
        let files = Mutex::new(Vec::new());
        self.walk_parallel(cancel, |path| {
            if let Ok(mut files) = files.lock() {
                files.push(path.to_path_buf());
            }
            true
        });
        if cancel.load(Ordering::Relaxed) {
            return Err(SearchError::Cancelled);
        }
        let mut files = files.into_inner().unwrap_or_else(|e| e.into_inner());
        files.sort_by(|a, b| {
            crate::locale::locale_compare(&a.to_string_lossy(), &b.to_string_lossy())
        });
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_comparison_can_ignore_case() {
        let path = Path::new("/Work/Space/Src/a.ts");
        let root = Path::new("/work/space");
        assert_eq!(strip_prefix_with(path, root, false), None);
        assert_eq!(
            strip_prefix_with(path, root, true),
            Some(PathBuf::from("Src/a.ts"))
        );
        assert_eq!(strip_prefix_with(Path::new("/work/spaceship"), root, true), None);
        assert_eq!(strip_prefix_with(Path::new("/work"), root, true), None);
    }

    #[test]
    fn relative_roots_are_rejected() {
        let request = SearchRequest::new("relative/root", "x", Default::default());
        assert!(matches!(
            Scope::new(&request),
            Err(SearchError::RootNotAbsolute(_))
        ));
    }
}
