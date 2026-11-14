//! Test-only helpers. Fixture creation needs file writes, which clippy bans in
//! production code (SAFETY_MODEL S1); they are allowed only here and only in tests.
#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};

/// Writes a fixture file, creating parent directories. Panics outside the temp dir.
pub fn write_file(path: &Path, bytes: &[u8]) {
    assert_test_path(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, bytes).expect("write fixture");
}

pub fn mkdir(path: &Path) {
    assert_test_path(path);
    std::fs::create_dir_all(path).expect("mkdir");
}

pub fn remove_file(path: &Path) {
    assert_test_path(path);
    std::fs::remove_file(path).expect("remove fixture");
}

pub fn rename(from: &Path, to: &Path) {
    assert_test_path(from);
    assert_test_path(to);
    std::fs::rename(from, to).expect("rename fixture");
}

pub fn remove_dir_all(path: &Path) {
    assert_test_path(path);
    std::fs::remove_dir_all(path).expect("remove fixture dir");
}

/// chmod for permission tests.
pub fn set_mode(path: impl AsRef<Path>, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let path = path.as_ref();
    assert_test_path(path);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// Creates a symlink at `link` (inside the temp dir) pointing to `target`.
pub fn symlink(target: impl AsRef<Path>, link: impl AsRef<Path>) {
    let (target, link) = (target.as_ref(), link.as_ref());
    assert_test_path(link);
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

/// Canonical temp dir (macOS /var → /private/var), so paths compare exactly.
pub fn tempdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let canon = dir.path().canonicalize().expect("canonicalize tempdir");
    (dir, canon)
}

/// Captures a deterministic snapshot of every entry under `root` (`(rel_path, size, mtime_ns, bytes)`)
/// so tests (`r_plan_01`, `r_onb_01`) can assert zero filesystem mutations occurred.
pub fn snapshot_tree(root: &Path) -> Vec<(String, u64, i64, Vec<u8>)> {
    use std::os::unix::fs::MetadataExt;
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .flatten()
    {
        let p = entry.path();
        let rel = p
            .strip_prefix(root)
            .unwrap_or(p)
            .to_string_lossy()
            .into_owned();
        if let Ok(md) = std::fs::symlink_metadata(p) {
            let bytes = if md.is_file() {
                std::fs::read(p).unwrap_or_default()
            } else {
                Vec::new()
            };
            out.push((rel, md.len(), md.mtime_nsec(), bytes));
        }
    }
    out
}

/// Guard: tests must never touch real user folders (TESTING_STRATEGY §4).
pub fn assert_test_path(path: &Path) {
    let tmp = std::env::temp_dir().canonicalize().expect("temp dir");
    let p = path.to_string_lossy();
    assert!(
        p.starts_with(&*tmp.to_string_lossy())
            || p.starts_with(&*std::env::temp_dir().to_string_lossy()),
        "test tried to touch a non-temp path: {p}"
    );
}

/// Valid file headers for classification tests.
pub mod magic {
    pub const PDF: &[u8] = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n";
    pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0";
    pub const JPEG: &[u8] = b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01\x01\0\0\x01\0\x01\0\0";
    pub const ZIP: &[u8] = b"PK\x03\x04\x14\0\0\0\x08\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
    pub const MACHO_64: &[u8] = b"\xcf\xfa\xed\xfe\x07\0\0\x01\x03\0\0\0\x02\0\0\0";
    pub const MP4: &[u8] = b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom";
    pub const GZIP: &[u8] = b"\x1f\x8b\x08\0\0\0\0\0\0\x03";
}
