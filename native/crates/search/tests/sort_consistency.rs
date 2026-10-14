//! Cross-crate consistency: the search walker, the file index and `vsm-core` must all order
//! file names identically (they used to carry diverging copies of `locale_compare`).

use std::cmp::Ordering;
use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;
use vsm_fs_index::FileIndex;
use vsm_search::{search_workspace, SearchOptions, SearchRequest};

const TRICKY: &[&str] = &[
    "a_b.ts",
    "a-b.ts",
    "A.ts",
    "a.ts",
    "10.ts",
    "9.ts",
    "ก.ts",
    "ข.ts",
    "สวัสดี.ts",
    ".env",
    "B.ts",
    "b.ts",
    "a b.ts",
    "README.md",
];

#[test]
fn every_crate_exposes_the_same_comparator() {
    for a in TRICKY {
        for b in TRICKY {
            let reference = vsm_core::paths::locale_compare(a, b);
            assert_eq!(vsm_search::locale_compare(a, b), reference, "search {a} vs {b}");
            assert_eq!(vsm_fs_index::paths::locale_compare(a, b), reference, "fs_index {a} vs {b}");
        }
    }
    // The pair from the code review: `_` sorts before `-` (ICU), in every crate.
    assert_eq!(vsm_search::locale_compare("a_b.ts", "a-b.ts"), Ordering::Less);
    assert_eq!(vsm_fs_index::paths::locale_compare("a_b.ts", "a-b.ts"), Ordering::Less);
}

#[test]
fn search_results_and_file_index_list_files_in_the_same_order() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path().to_path_buf();
    fs::create_dir(root.join("sub")).expect("mkdir");
    for name in TRICKY {
        fs::write(root.join(name), "needle\n").expect("write");
        fs::write(root.join("sub").join(name), "needle\n").expect("write");
    }

    let index = FileIndex::new();
    let indexed: Vec<PathBuf> = index.get_files(&root).expect("index").iter().cloned().collect();

    let request = SearchRequest::new(&root, "needle", SearchOptions::default());
    let mut searched: Vec<PathBuf> = Vec::new();
    for hit in search_workspace(&request).expect("search") {
        if searched.last() != Some(&hit.path) {
            searched.push(hit.path);
        }
    }

    let mut expected = indexed.clone();
    expected.sort_by(|a, b| vsm_core::paths::compare_paths(a, b));
    assert_eq!(indexed, expected, "file index order");
    assert_eq!(searched, expected, "search order");
    assert_eq!(expected.len(), TRICKY.len() * 2);
}
