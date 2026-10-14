//! The search engine: `grep-searcher` over files discovered by `ignore`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

use grep_regex::RegexMatcher;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkFinish, SinkMatch};

use crate::error::{Result, SearchError};
use crate::options::build_line_matcher;
use crate::scope::{Scope, SearchRequest};

/// One matching line (port of `SearchResult`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    /// Absolute path of the file.
    pub path: PathBuf,
    /// File name (`path.basename`).
    pub name: String,
    /// 1-based line number.
    pub line: u64,
    /// The matching line, trimmed.
    pub text: String,
}

/// Outcome of a streaming search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchSummary {
    /// Number of matches delivered to the sink.
    pub matches: usize,
    /// `true` only when at least one more match existed than the `max_results` cap allows.
    pub truncated: bool,
    /// `true` when the search stopped because of the cancellation flag.
    pub cancelled: bool,
}

fn build_searcher() -> Searcher {
    SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(b'\0'))
        .build()
}

/// Collects the matches of one file. If binary data (NUL) is detected anywhere in the
/// file, every match of that file is dropped, so "binary files are skipped" holds even
/// when the NUL byte shows up late in a big file.
struct FileSink<'a> {
    path: &'a Path,
    name: &'a str,
    limit: usize,
    cancel: &'a AtomicBool,
    matches: Vec<SearchMatch>,
    binary: bool,
}

impl Sink for FileSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> io::Result<bool> {
        if self.cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        // Past the limit the rest of the file is still scanned (without keeping matches)
        // so that a NUL byte further down can still disqualify the whole file.
        if self.matches.len() < self.limit {
            self.matches.push(SearchMatch {
                path: self.path.to_path_buf(),
                name: self.name.to_string(),
                line: mat.line_number().unwrap_or(0),
                text: String::from_utf8_lossy(mat.bytes()).trim().to_string(),
            });
        }
        Ok(true)
    }

    fn binary_data(&mut self, _: &Searcher, _: u64) -> io::Result<bool> {
        self.binary = true;
        Ok(false)
    }

    fn finish(&mut self, _: &Searcher, finish: &SinkFinish) -> io::Result<()> {
        if finish.binary_byte_offset().is_some() {
            self.binary = true;
        }
        Ok(())
    }
}

/// Search one file and return its matching lines (at most `limit`). Binary files yield
/// no matches at all.
fn search_file(
    searcher: &mut Searcher,
    matcher: &RegexMatcher,
    path: &Path,
    limit: usize,
    cancel: &AtomicBool,
) -> io::Result<Vec<SearchMatch>> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut sink = FileSink {
        path,
        name: &name,
        limit,
        cancel,
        matches: Vec::new(),
        binary: false,
    };
    searcher.search_path(matcher, path, &mut sink)?;
    if sink.binary {
        sink.matches.clear();
    }
    Ok(sink.matches)
}

/// Port of `searchWorkspace`: deterministic results (files in sorted order, lines in
/// order), capped at `request.max_results`. Files are searched in parallel but the
/// result is identical to a sequential, sorted scan.
pub fn search_workspace(request: &SearchRequest) -> Result<Vec<SearchMatch>> {
    search_workspace_cancellable(request, &AtomicBool::new(false))
}

/// Same as [`search_workspace`] but stops with [`SearchError::Cancelled`] when `cancel` is set.
pub fn search_workspace_cancellable(
    request: &SearchRequest,
    cancel: &AtomicBool,
) -> Result<Vec<SearchMatch>> {
    if request.query.trim().is_empty() || request.max_results == 0 {
        return Ok(Vec::new());
    }
    let options = request.options.normalized();
    let matcher = build_line_matcher(&request.query, &options)?;
    let scope = Scope::new(request)?;
    let files = scope.collect_sorted(cancel)?;
    let cap = request.max_results;

    let slots: Vec<Mutex<Vec<SearchMatch>>> = files.iter().map(|_| Mutex::new(Vec::new())).collect();
    let next = AtomicUsize::new(0);
    let total = AtomicUsize::new(0);
    let workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(files.len().max(1));

    thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                let mut searcher = build_searcher();
                loop {
                    // Files are claimed in order and always searched to completion (up to
                    // the per-file cap), so the first `cap` matches in file order are exact.
                    if cancel.load(Ordering::Relaxed) || total.load(Ordering::Relaxed) >= cap {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(index) else { break };
                    let found =
                        search_file(&mut searcher, &matcher, path, cap, cancel).unwrap_or_default();
                    total.fetch_add(found.len(), Ordering::Relaxed);
                    if let Ok(mut slot) = slots[index].lock() {
                        *slot = found;
                    }
                }
            });
        }
    });

    if cancel.load(Ordering::Relaxed) {
        return Err(SearchError::Cancelled);
    }

    let mut results = Vec::new();
    for slot in slots {
        results.extend(slot.into_inner().unwrap_or_else(|e| e.into_inner()));
        if results.len() >= cap {
            results.truncate(cap);
            break;
        }
    }
    Ok(results)
}

/// Streaming search: matches are passed to `sink` as soon as they are found, from
/// several threads, in no particular order. Stops at `request.max_results` matches or
/// when `cancel` is set. The call blocks until the search is over.
pub fn search_streaming<F>(
    request: &SearchRequest,
    cancel: &AtomicBool,
    sink: F,
) -> Result<SearchSummary>
where
    F: Fn(SearchMatch) + Sync,
{
    if request.query.trim().is_empty() || request.max_results == 0 {
        return Ok(SearchSummary::default());
    }
    let options = request.options.normalized();
    let matcher = build_line_matcher(&request.query, &options)?;
    let scope = Scope::new(request)?;
    let cap = request.max_results;
    let emitted = AtomicUsize::new(0);
    let truncated = AtomicBool::new(false);

    scope.walk_parallel(cancel, |path| {
        if truncated.load(Ordering::Relaxed) {
            return false;
        }
        // Once the cap is reached keep looking until a file has one more match: only a
        // refused reservation proves that results were cut off.
        let limit = cap
            .saturating_sub(emitted.load(Ordering::Relaxed))
            .saturating_add(1);
        let mut searcher = build_searcher();
        let Ok(found) = search_file(&mut searcher, &matcher, path, limit, cancel) else {
            return true;
        };
        for m in found {
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            // Reserve a slot first so the cap is never exceeded under contention.
            if emitted.fetch_add(1, Ordering::Relaxed) >= cap {
                emitted.fetch_sub(1, Ordering::Relaxed);
                truncated.store(true, Ordering::Relaxed);
                return false;
            }
            sink(m);
        }
        true
    });

    Ok(SearchSummary {
        matches: emitted.load(Ordering::Relaxed),
        truncated: truncated.load(Ordering::Relaxed),
        cancelled: cancel.load(Ordering::Relaxed),
    })
}

/// A search running on a background thread, delivering results through a channel.
pub struct SearchHandle {
    /// Matches arrive here; the channel closes when the search is finished.
    pub receiver: mpsc::Receiver<SearchMatch>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<SearchSummary>>>,
}

impl SearchHandle {
    /// Ask the search to stop as soon as possible.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// A clonable cancellation flag.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    /// Wait for the search to finish and return its summary.
    pub fn join(mut self) -> Result<SearchSummary> {
        match self.thread.take() {
            Some(thread) => thread.join().map_err(|_| SearchError::WorkerPanicked)?,
            None => Ok(SearchSummary::default()),
        }
    }
}

impl Drop for SearchHandle {
    fn drop(&mut self) {
        // Dropping the handle abandons the search.
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Run [`search_streaming`] on a background thread.
pub fn spawn_search(request: SearchRequest) -> SearchHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = Arc::clone(&cancel);
    let thread = thread::spawn(move || {
        search_streaming(&request, &thread_cancel, |m| {
            if tx.send(m).is_err() {
                // Receiver gone: nobody is interested any more.
                thread_cancel.store(true, Ordering::Relaxed);
            }
        })
    });
    SearchHandle {
        receiver: rx,
        cancel,
        thread: Some(thread),
    }
}
