//! Folder scanner (FR-006, FR-011, FR-017, FR-033). Read-only.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::Path;

use walkdir::WalkDir;

use crate::engine::classifier::{Classifier, Confidence, HEADER_BYTES};
use crate::engine::fs_view::{facts_from_metadata, EntryKind, FileFacts, ReadOnlyFs, RealFs};
use crate::engine::ignore::IgnoreRules;

pub const BATCH_SIZE: usize = 500;
/// Upper bound on entries walked inside one package when sizing it.
pub const MAX_PACKAGE_ENTRIES: usize = 50_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub facts: FileFacts,
    pub category: String,
    pub mime: Option<String>,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CategoryTotal {
    pub category: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanSummary {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub skipped: u64,
    pub errors: u64,
    /// Sorted by bytes descending.
    pub by_category: Vec<CategoryTotal>,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub recursive: bool,
    pub rules: IgnoreRules,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("folder not found")]
    NotFound,
    #[error("permission denied")]
    PermissionDenied,
    #[error("io error: {0}")]
    Io(String),
    #[error("scan cancelled: {0}")]
    Sink(String),
}

/// Walks `root`, classifying each processable item and handing batches of at
/// most `BATCH_SIZE` to `sink`. Never follows symlinks; never descends into
/// packages or pruned folders. Memory use is bounded by the batch size.
pub fn scan<F>(
    root: &Path,
    opts: &ScanOptions,
    classifier: &Classifier,
    mut sink: F,
) -> Result<ScanSummary, ScanError>
where
    F: FnMut(Vec<ScannedFile>) -> Result<(), String>,
{
    let io_err = |e: std::io::Error| match e.kind() {
        ErrorKind::NotFound => ScanError::NotFound,
        ErrorKind::PermissionDenied => ScanError::PermissionDenied,
        _ => ScanError::Io(e.kind().to_string()),
    };
    if !std::fs::metadata(root).map_err(io_err)?.is_dir() {
        return Err(ScanError::NotFound);
    }
    // Probe readability up front: TCC denial surfaces here, not as N per-entry errors.
    std::fs::read_dir(root).map_err(io_err)?;

    let fs = RealFs;
    let mut summary = ScanSummary::default();
    let mut totals: HashMap<String, (u64, u64)> = HashMap::new();
    let mut batch: Vec<ScannedFile> = Vec::with_capacity(BATCH_SIZE);

    let mut walker = WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .max_depth(if opts.recursive { usize::MAX } else { 1 })
        .sort_by_file_name()
        .into_iter();

    while let Some(next) = walker.next() {
        let entry = match next {
            Ok(e) => e,
            Err(err) => {
                summary.errors += 1;
                tracing::debug!(target: "folderflow::scan", error = %err, "entry unreadable");
                continue;
            }
        };
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                summary.errors += 1;
                continue;
            }
        };
        let mut facts = facts_from_metadata(entry.path(), &meta);
        match facts.kind {
            EntryKind::Symlink | EntryKind::Special => {
                summary.skipped += 1;
                continue;
            }
            EntryKind::Dir => {
                if opts
                    .rules
                    .check_dir(&facts.path, &facts.name, facts.is_hidden)
                    .is_some()
                {
                    summary.skipped += 1;
                    walker.skip_current_dir();
                } else {
                    summary.folders += 1;
                }
                continue;
            }
            EntryKind::Package => {
                walker.skip_current_dir();
                if opts
                    .rules
                    .check_dir(&facts.path, &facts.name, facts.is_hidden)
                    .is_some()
                    || opts
                        .rules
                        .check_file(
                            &facts.path,
                            &facts.name,
                            facts.extension.as_deref(),
                            facts.is_hidden,
                        )
                        .is_some()
                {
                    summary.skipped += 1;
                    continue;
                }
                facts.size = package_size(&facts.path);
            }
            EntryKind::File => {
                if opts
                    .rules
                    .check_file(
                        &facts.path,
                        &facts.name,
                        facts.extension.as_deref(),
                        facts.is_hidden,
                    )
                    .is_some()
                {
                    summary.skipped += 1;
                    continue;
                }
            }
        }

        let is_package = facts.kind == EntryKind::Package;
        let header = if is_package {
            None
        } else if facts.size == 0 {
            Some(Vec::new())
        } else {
            fs.read_header(&facts.path, HEADER_BYTES).ok()
        };
        let c = classifier.classify(&facts.name, header.as_deref(), is_package);

        summary.files += 1;
        summary.bytes += facts.size;
        let t = totals.entry(c.category.clone()).or_default();
        t.0 += 1;
        t.1 += facts.size;

        batch.push(ScannedFile {
            facts,
            category: c.category,
            mime: c.mime,
            confidence: c.confidence,
        });
        if batch.len() == BATCH_SIZE {
            sink(std::mem::take(&mut batch)).map_err(ScanError::Sink)?;
        }
    }
    if !batch.is_empty() {
        sink(batch).map_err(ScanError::Sink)?;
    }

    summary.by_category = totals
        .into_iter()
        .map(|(category, (files, bytes))| CategoryTotal {
            category,
            files,
            bytes,
        })
        .collect();
    summary.by_category.sort_by(|a, b| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| a.category.cmp(&b.category))
    });
    Ok(summary)
}

/// Total size of regular files inside a package (bounded walk, no symlinks).
pub(crate) fn package_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .take(MAX_PACKAGE_ENTRIES)
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::builtin_categories;
    use crate::test_support::{magic, tempdir, write_file};
    use std::path::PathBuf;

    fn opts(recursive: bool) -> ScanOptions {
        ScanOptions {
            recursive,
            rules: IgnoreRules {
                ignore_hidden: true,
                ignore_temporary: true,
                ..Default::default()
            },
        }
    }

    fn run(root: &Path, o: &ScanOptions) -> (ScanSummary, Vec<ScannedFile>, Vec<usize>) {
        let c = Classifier::new(builtin_categories());
        let mut all = Vec::new();
        let mut batches = Vec::new();
        let s = scan(root, o, &c, |b| {
            batches.push(b.len());
            all.extend(b);
            Ok(())
        })
        .expect("scan");
        all.sort_by(|a, b| a.facts.path.cmp(&b.facts.path));
        (s, all, batches)
    }

    /// Sorted file names (order-independent comparison).
    fn names(files: &[ScannedFile]) -> Vec<String> {
        let mut n: Vec<String> = files.iter().map(|f| f.facts.name.clone()).collect();
        n.sort();
        n
    }

    fn tree(root: &Path) {
        write_file(&root.join("invoice.pdf"), magic::PDF);
        write_file(&root.join("photo.png"), magic::PNG);
        write_file(&root.join("Projects/notes.txt"), b"hello world");
        write_file(&root.join("Projects/deep/song.mp3"), b"ID3\x03\0\0\0\0\0\0");
    }

    #[test]
    fn r_scan_01_recursive_counts_bytes_and_categories() {
        let (_d, root) = tempdir();
        tree(&root);
        let (s, files, _) = run(&root, &opts(true));
        assert_eq!(
            names(&files),
            vec!["invoice.pdf", "notes.txt", "photo.png", "song.mp3"]
        );
        assert_eq!((s.files, s.folders, s.errors), (4, 2, 0));
        let expected_bytes = (magic::PDF.len() + magic::PNG.len() + 11 + 10) as u64;
        assert_eq!(s.bytes, expected_bytes);
        let docs = s
            .by_category
            .iter()
            .find(|c| c.category == "documents")
            .expect("documents");
        assert_eq!(docs.files, 2);
        assert_eq!(s.by_category.iter().map(|c| c.files).sum::<u64>(), 4);
        assert!(s.by_category.windows(2).all(|w| w[0].bytes >= w[1].bytes));
        let pdf = files
            .iter()
            .find(|f| f.facts.name == "invoice.pdf")
            .expect("pdf");
        assert_eq!(
            (pdf.category.as_str(), pdf.mime.as_deref()),
            ("documents", Some("application/pdf"))
        );
    }

    #[test]
    fn r_scan_02_non_recursive_lists_top_level_only() {
        let (_d, root) = tempdir();
        tree(&root);
        let (s, files, _) = run(&root, &opts(false));
        assert_eq!(names(&files), vec!["invoice.pdf", "photo.png"]);
        assert_eq!((s.files, s.folders), (2, 1));
    }

    #[test]
    fn r_scan_03_excluded_folders_and_extensions() {
        let (_d, root) = tempdir();
        tree(&root);
        let mut o = opts(true);
        o.rules.excluded_dirs = vec![root.join("Projects/deep")];
        o.rules.exclude_extensions = vec!["png".into()];
        let (s, files, _) = run(&root, &o);
        assert_eq!(names(&files), vec!["invoice.pdf", "notes.txt"]);
        assert_eq!(s.skipped, 2, "excluded dir + excluded extension");
    }

    #[test]
    fn r_scan_04_ignore_list_and_cache_dirs() {
        let (_d, root) = tempdir();
        write_file(&root.join("keep.pdf"), magic::PDF);
        for junk in [
            ".DS_Store",
            "._keep.pdf",
            "movie.mp4.crdownload",
            ".hidden.txt",
            "x.tmp",
            ".folderflow-1.partial",
        ] {
            write_file(&root.join(junk), b"x");
        }
        write_file(&root.join("big.zip.download/big.zip"), b"PK");
        write_file(&root.join("node_modules/pkg/index.js"), b"x");
        let (s, files, _) = run(&root, &opts(true));
        assert_eq!(names(&files), vec!["keep.pdf"]);
        assert_eq!(s.skipped, 8);
    }

    #[test]
    fn r_scan_05_symlinks_are_not_followed_and_loops_terminate() {
        let (_d, root) = tempdir();
        let (_o, outside) = tempdir();
        write_file(&outside.join("secret.pdf"), magic::PDF);
        write_file(&root.join("a/real.pdf"), magic::PDF);
        crate::test_support::symlink(&root, root.join("a/loop"));
        crate::test_support::symlink(&outside, root.join("out"));
        crate::test_support::symlink(root.join("a/real.pdf"), root.join("alias.pdf"));
        let (s, files, _) = run(&root, &opts(true));
        assert_eq!(names(&files), vec!["real.pdf"]);
        assert_eq!(s.skipped, 3, "three symlinks skipped");
    }

    #[test]
    fn r_scan_06_packages_are_single_items() {
        let (_d, root) = tempdir();
        write_file(&root.join("Slack.app/Contents/MacOS/Slack"), &[0u8; 100]);
        write_file(&root.join("Slack.app/Contents/Info.plist"), &[0u8; 20]);
        let (s, files, _) = run(&root, &opts(true));
        assert_eq!(names(&files), vec!["Slack.app"]);
        assert_eq!(files[0].category, "installers");
        assert_eq!(files[0].facts.size, 120, "package size = sum of contents");
        assert_eq!((s.files, s.bytes), (1, 120));
    }

    #[test]
    fn r_scan_07_batches_are_bounded() {
        let (_d, root) = tempdir();
        for i in 0..1203 {
            write_file(&root.join(format!("f{i:04}.txt")), b"x");
        }
        let (s, _, batches) = run(&root, &opts(false));
        assert_eq!(s.files, 1203);
        assert_eq!(batches, vec![500, 500, 203]);
    }

    #[test]
    fn r_scan_08_unreadable_subfolder_is_counted_and_scan_continues() {
        let (_d, root) = tempdir();
        write_file(&root.join("ok.pdf"), magic::PDF);
        write_file(&root.join("locked/x.pdf"), magic::PDF);
        let locked = root.join("locked");
        set_mode(&locked, 0o000);
        let (s, files, _) = run(&root, &opts(true));
        set_mode(&locked, 0o755);
        assert_eq!(names(&files), vec!["ok.pdf"]);
        assert_eq!(s.errors, 1);
    }

    #[test]
    fn r_scan_09_missing_root_is_an_error() {
        let (_d, root) = tempdir();
        let c = Classifier::new(builtin_categories());
        let r = scan(&root.join("nope"), &opts(true), &c, |_| Ok(()));
        assert!(matches!(r, Err(ScanError::NotFound)));
        let _ = PathBuf::new();
    }

    fn set_mode(p: &Path, mode: u32) {
        crate::test_support::set_mode(p, mode);
    }
}
