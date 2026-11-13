//! Read-only filesystem view. Nothing in this module can modify the filesystem;
//! planning/dry-run code receives only `&dyn ReadOnlyFs` (SAFETY_MODEL S5).

use std::fs::Metadata;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::engine::classifier::extension_of;

/// macOS `UF_HIDDEN` file flag (Finder "hidden").
const UF_HIDDEN: u32 = 0x8000;

/// Directory extensions macOS treats as single items (packages / bundles).
pub const PACKAGE_EXTENSIONS: &[&str] = &[
    "app",
    "bundle",
    "framework",
    "plugin",
    "kext",
    "pkg",
    "mpkg",
    "photoslibrary",
    "musiclibrary",
    "tvlibrary",
    "xcodeproj",
    "xcworkspace",
    "playground",
    "rtfd",
    "pages",
    "numbers",
    "key",
    "sparsebundle",
    "fcpbundle",
    "logicx",
    "band",
    "imovielibrary",
    "lrdata",
    "lrlibrary",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    /// A directory macOS presents as one item (e.g. `.app`).
    Package,
    Symlink,
    /// FIFO, socket, device — never opened or processed.
    Special,
}

/// Snapshot of a filesystem entry's metadata (never its contents).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFacts {
    pub path: PathBuf,
    pub name: String,
    pub extension: Option<String>,
    pub kind: EntryKind,
    pub size: u64,
    pub dev: u64,
    pub ino: u64,
    pub created_ns: Option<i64>,
    pub modified_ns: i64,
    pub is_hidden: bool,
}

pub trait ReadOnlyFs: Send + Sync {
    /// Metadata without following a symlink leaf.
    fn stat(&self, path: &Path) -> std::io::Result<FileFacts>;
    /// Up to `max` bytes from the start of a regular file (no symlink follow, non-blocking open).
    fn read_header(&self, path: &Path, max: usize) -> std::io::Result<Vec<u8>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealFs;

pub fn kind_of(meta: &Metadata, name: &str) -> EntryKind {
    let ft = meta.file_type();
    if ft.is_symlink() {
        EntryKind::Symlink
    } else if ft.is_dir() {
        match extension_of(name) {
            Some(ext) if PACKAGE_EXTENSIONS.contains(&ext.as_str()) => EntryKind::Package,
            _ => EntryKind::Dir,
        }
    } else if ft.is_file() {
        EntryKind::File
    } else {
        EntryKind::Special
    }
}

/// Builds facts from metadata. The path and name are NFC-normalised so the
/// same file always has the same key regardless of how macOS spelled it.
pub fn facts_from_metadata(path: &Path, meta: &Metadata) -> FileFacts {
    let path = crate::engine::paths::nfc(path);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    #[cfg(target_os = "macos")]
    let flag_hidden = std::os::darwin::fs::MetadataExt::st_flags(meta) & UF_HIDDEN != 0;
    #[cfg(not(target_os = "macos"))]
    let flag_hidden = false;
    let created_ns = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
    FileFacts {
        extension: extension_of(&name),
        kind: kind_of(meta, &name),
        size: meta.len(),
        dev: meta.dev(),
        ino: meta.ino(),
        created_ns,
        modified_ns: meta
            .mtime()
            .saturating_mul(1_000_000_000)
            .saturating_add(meta.mtime_nsec()),
        is_hidden: name.starts_with('.') || flag_hidden,
        name,
        path,
    }
}

impl ReadOnlyFs for RealFs {
    fn stat(&self, path: &Path) -> std::io::Result<FileFacts> {
        let meta = std::fs::symlink_metadata(path)?;
        Ok(facts_from_metadata(path, &meta))
    }

    fn read_header(&self, path: &Path, max: usize) -> std::io::Result<Vec<u8>> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }
        let mut buf = Vec::with_capacity(max.min(8192));
        file.take(max as u64).read_to_end(&mut buf)?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{mkdir, tempdir, write_file};

    #[test]
    fn stat_reports_kinds_hidden_and_identity_without_following_links() {
        let (_d, root) = tempdir();
        write_file(&root.join("a.txt"), b"hello");
        write_file(&root.join(".secret"), b"x");
        mkdir(&root.join("Slack.app/Contents"));
        mkdir(&root.join("dir"));
        crate::test_support::symlink(root.join("a.txt"), root.join("link.txt"));
        let fs = RealFs;
        let a = fs.stat(&root.join("a.txt")).expect("stat");
        assert_eq!(
            (a.kind, a.size, a.extension.as_deref(), a.is_hidden),
            (EntryKind::File, 5, Some("txt"), false)
        );
        assert!(a.ino > 0 && a.modified_ns > 0);
        assert!(fs.stat(&root.join(".secret")).expect("stat").is_hidden);
        assert_eq!(
            fs.stat(&root.join("Slack.app")).expect("stat").kind,
            EntryKind::Package
        );
        assert_eq!(
            fs.stat(&root.join("dir")).expect("stat").kind,
            EntryKind::Dir
        );
        let l = fs.stat(&root.join("link.txt")).expect("stat");
        assert_eq!(l.kind, EntryKind::Symlink);
        assert_ne!(l.ino, a.ino);
    }

    #[test]
    fn read_header_is_bounded_and_refuses_symlinks_and_fifos() {
        let (_d, root) = tempdir();
        write_file(&root.join("big.bin"), &vec![7u8; 20_000]);
        crate::test_support::symlink(root.join("big.bin"), root.join("link"));
        let fs = RealFs;
        assert_eq!(
            fs.read_header(&root.join("big.bin"), 8192)
                .expect("read")
                .len(),
            8192
        );
        assert!(
            fs.read_header(&root.join("link"), 8192).is_err(),
            "O_NOFOLLOW"
        );
        let fifo = root.join("pipe");
        let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).expect("cstr");
        #[allow(unsafe_code)]
        // SAFETY: test-only FFI call with a valid NUL-terminated path.
        let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
        assert_eq!(rc, 0);
        assert!(
            fs.read_header(&fifo, 8192).is_err(),
            "must not block or read a FIFO"
        );
    }
}
