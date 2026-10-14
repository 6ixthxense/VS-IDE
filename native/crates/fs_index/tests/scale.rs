//! Performance guards for large indexes (generous bounds so they only catch
//! algorithmic regressions, not machine noise).

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use vsm_fs_index::{FileIndex, IndexConfig};

fn big_tree(dirs: usize, files_per_dir: usize) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for d in 0..dirs {
        let dir = root.join(format!("pkg{:03}/src", d));
        fs::create_dir_all(&dir).unwrap();
        for f in 0..files_per_dir {
            fs::write(dir.join(format!("File{f}.ts")), "").unwrap();
        }
    }
    (tmp, root)
}

#[test]
fn building_and_updating_a_large_index_stays_fast() {
    let (_tmp, root) = big_tree(200, 100); // 20,000 files
    let index = FileIndex::with_config(IndexConfig {
        watch: false,
        ..IndexConfig::default()
    });

    let started = Instant::now();
    let files = index.get_files(&root).unwrap();
    let build = started.elapsed();
    assert_eq!(files.len(), 20_000);
    assert!(build < Duration::from_secs(10), "build took {build:?}");

    // Renaming a directory of 100 files re-sorts nothing and stays cheap.
    let started = Instant::now();
    index.rename_path(&root.join("pkg007"), &root.join("pkg007-renamed"));
    let rename = started.elapsed();
    assert!(rename < Duration::from_secs(2), "rename took {rename:?}");
    let files = index.get_files(&root).unwrap();
    assert_eq!(files.len(), 20_000);
    assert!(files.contains(&root.join("pkg007-renamed/src/File3.ts")));
    assert!(files.windows(2).all(|w| vsm_fs_index::paths::compare_paths(&w[0], &w[1]).is_lt()));

    // Single-file operations are cheap too.
    let started = Instant::now();
    for i in 0..200 {
        let path = root.join(format!("pkg001/src/Extra{i}.ts"));
        index.add_file(&path);
        index.remove_path(&path);
    }
    let single = started.elapsed();
    assert!(single < Duration::from_secs(3), "200 add/remove pairs took {single:?}");
}
