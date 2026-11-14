//! Exclusive, no-overwrite mutating filesystem primitives (`SAFETY_MODEL.md` S1–S5, S8, S10).
//!
//! This module is the **only** production module permitted to mutate the filesystem.
//! Planning, rule evaluation, and dry-run code receive only `&dyn ReadOnlyFs` (`fs_view.rs`).
#![allow(clippy::disallowed_methods)]

pub mod copy;
pub mod fault;
pub mod rename;
pub mod trash;

#[cfg(test)]
mod tests;

use std::ffi::CString;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, ErrorKind};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::engine::fs_view::{kind_of, EntryKind};

/// Streaming buffer size used by `copy_excl` and `hash_file` (`SAFETY_MODEL.md`: 64 KiB).
pub const STREAM_BUF_BYTES: usize = 64 * 1024;

/// 32-byte BLAKE3 digest with hex formatting and parsing.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Blake3Hash(pub [u8; 32]);

impl Blake3Hash {
    pub fn to_hex(self) -> String {
        blake3::Hash::from_bytes(self.0).to_hex().to_string()
    }
}

impl fmt::Debug for Blake3Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Blake3Hash({})", self.to_hex())
    }
}

impl fmt::Display for Blake3Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl FromStr for Blake3Hash {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed = blake3::Hash::from_hex(s).map_err(|e| e.to_string())?;
        Ok(Self(*parsed.as_bytes()))
    }
}

/// Snapshot of an entry's metadata taken via `lstat` (`symlink_metadata`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryIdentity {
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub mtime_ns: i64,
    pub kind: EntryKind,
}

impl EntryIdentity {
    pub fn same_version(&self, other: &Self) -> bool {
        self.dev == other.dev
            && self.ino == other.ino
            && self.size == other.size
            && self.mtime_ns == other.mtime_ns
            && self.kind == other.kind
    }
}

/// Result of a streaming exclusive copy (`copy_excl`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyOutput {
    pub bytes_copied: u64,
    pub blake3: Blake3Hash,
}

/// Unforgeable capability token proving that a cross-volume destination has been
/// written, `fsync`ed, and re-verified (`size` + `blake3`) while the source
/// retained its expected `EntryIdentity` (`SAFETY_MODEL.md` S3).
#[derive(Debug)]
pub struct VerifiedSourceToken {
    pub(crate) source_path: PathBuf,
    pub(crate) source_identity: EntryIdentity,
    pub(crate) blake3: Blake3Hash,
}

impl VerifiedSourceToken {
    pub fn blake3(&self) -> Blake3Hash {
        self.blake3
    }
}

/// Mutating filesystem interface (`P4-T01`, `P4-T02`).
///
/// Only `organizer`, `undo`, `recovery`, and `duplicates::actions` may hold `&dyn Fs`.
pub trait Fs: Send + Sync {
    /// Metadata without following a symlink leaf (`lstat`).
    fn lstat(&self, path: &Path) -> io::Result<EntryIdentity>;

    /// Returns `true` if `path` exists (including a dangling symlink), without following symlinks.
    fn exists_no_follow(&self, path: &Path) -> bool {
        self.lstat(path).is_ok()
    }

    /// Rejects if any ancestor directory of `path` (up to `stop_at_root`) is a symlink (`R-SFS-07`).
    fn ensure_no_symlink_ancestors(&self, path: &Path, stop_at_root: &Path) -> io::Result<()>;

    /// Creates directory tree (`mkdir -p`), refusing symlink ancestors, and `fsync`s parent.
    fn mkdir_p(&self, dir: &Path) -> io::Result<()>;

    /// Removes `dir` only if it is completely empty (`rmdir(2)`). Returns `Ok(true)` if removed,
    /// `Ok(false)` if non-empty or not found. Never deletes files or non-empty directories.
    fn rmdir_if_empty(&self, dir: &Path) -> io::Result<bool>;

    /// Exclusive atomic rename (`renamex_np(RENAME_EXCL)` on macOS).
    /// Fails with `ErrorKind::AlreadyExists` if `to` exists (even a dangling symlink),
    /// and with `EXDEV` across filesystems.
    fn rename_excl(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Exclusive streaming copy (`O_CREAT | O_EXCL`, 64 KiB buffer, BLAKE3, `fsync` file + parent).
    fn copy_excl(&self, from: &Path, to: &Path) -> io::Result<CopyOutput>;

    /// Streams `path` in 64 KiB chunks (`O_NOFOLLOW | O_NONBLOCK`) and computes `(size, Blake3Hash)`.
    fn hash_file(&self, path: &Path) -> io::Result<(u64, Blake3Hash)>;

    /// Verifies that `dst` matches `copied` (`size` and freshly computed `blake3`) and that `from`
    /// still matches `expected_src` (`dev, ino, size, mtime_ns`), returning a [`VerifiedSourceToken`].
    fn verify_copied(
        &self,
        from: &Path,
        expected_src: &EntryIdentity,
        dst: &Path,
        copied: &CopyOutput,
    ) -> io::Result<VerifiedSourceToken>;

    /// Unlinks `from` after a verified cross-volume move. Requires a [`VerifiedSourceToken`]
    /// and re-checks that `from` still matches the token's identity before unlinking.
    fn remove_verified_source(&self, from: &Path, token: VerifiedSourceToken) -> io::Result<()>;

    /// Moves `path` to macOS Trash (or configured test `.Trash`), returning the Trash path.
    /// Never permanently deletes (`SAFETY_MODEL.md` S3).
    fn move_to_trash(&self, path: &Path) -> io::Result<PathBuf>;

    /// Returns available free bytes (`f_bavail * f_frsize`) on the volume containing `path`.
    fn stat_fs_free_bytes(&self, path: &Path) -> io::Result<u64>;

    /// `fsync`s a directory descriptor.
    fn fsync_dir(&self, dir: &Path) -> io::Result<()>;

    /// Number of `fsync` calls recorded by this filesystem (for `R-SFS-04` verification).
    fn fsync_count(&self) -> usize;
}

/// Production implementation of [`Fs`].
#[derive(Debug, Clone)]
pub struct SafeFs {
    trash_dir_override: Option<PathBuf>,
    fsync_counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Default for SafeFs {
    fn default() -> Self {
        Self::production()
    }
}

impl SafeFs {
    /// Production filesystem using `NSFileManager::trashItemAtURL` for Trash.
    pub fn production() -> Self {
        Self {
            trash_dir_override: None,
            fsync_counter: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// Filesystem routing `move_to_trash` into `trash_dir` using exclusive rename so tests
    /// never pollute the user's real `~/.Trash` (`TESTING_STRATEGY.md` §4).
    pub fn with_trash_dir(trash_dir: PathBuf) -> Self {
        Self {
            trash_dir_override: Some(trash_dir),
            fsync_counter: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    pub(crate) fn record_fsync(&self) {
        self.fsync_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Fs for SafeFs {
    fn lstat(&self, path: &Path) -> io::Result<EntryIdentity> {
        let meta = std::fs::symlink_metadata(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(EntryIdentity {
            dev: meta.dev(),
            ino: meta.ino(),
            size: meta.len(),
            mtime_ns: meta
                .mtime()
                .saturating_mul(1_000_000_000)
                .saturating_add(meta.mtime_nsec()),
            kind: kind_of(&meta, &name),
        })
    }

    fn ensure_no_symlink_ancestors(&self, path: &Path, stop_at_root: &Path) -> io::Result<()> {
        let mut cursor = path.parent();
        while let Some(dir) = cursor {
            if let Ok(meta) = std::fs::symlink_metadata(dir) {
                if meta.file_type().is_symlink() {
                    return Err(io::Error::new(
                        ErrorKind::InvalidInput,
                        format!("destination ancestor is a symlink: {}", dir.display()),
                    ));
                }
                if !meta.file_type().is_dir() {
                    return Err(io::Error::new(
                        ErrorKind::InvalidInput,
                        format!("destination ancestor is not a directory: {}", dir.display()),
                    ));
                }
            }
            if dir == stop_at_root {
                break;
            }
            cursor = dir.parent();
        }
        Ok(())
    }

    fn mkdir_p(&self, dir: &Path) -> io::Result<()> {
        // Walk from root to `dir` checking existing components for symlinks before creating.
        let mut built = PathBuf::new();
        for comp in dir.components() {
            built.push(comp);
            match std::fs::symlink_metadata(&built) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        // macOS top-level system links (`/var -> private/var`, `/tmp -> private/tmp`,
                        // `/etc -> private/etc`) are OS-owned root firmlinks/symlinks (`BUG-021`).
                        // Any symlink deeper in the hierarchy is rejected.
                        let is_macos_root_system_link =
                            matches!(built.to_str(), Some("/var" | "/tmp" | "/etc"))
                                && std::fs::metadata(&built)
                                    .map(|m| m.is_dir())
                                    .unwrap_or(false);
                        if !is_macos_root_system_link {
                            return Err(io::Error::new(
                                ErrorKind::InvalidInput,
                                format!(
                                    "refusing to traverse symlink directory: {}",
                                    built.display()
                                ),
                            ));
                        }
                    } else if !meta.file_type().is_dir() {
                        return Err(io::Error::new(
                            ErrorKind::AlreadyExists,
                            format!("path component is not a directory: {}", built.display()),
                        ));
                    }
                }
                Err(e) if e.kind() == ErrorKind::NotFound => match std::fs::create_dir(&built) {
                    Ok(()) => {}
                    Err(ce) if ce.kind() == ErrorKind::AlreadyExists => {
                        let meta = std::fs::symlink_metadata(&built)?;
                        if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
                            return Err(io::Error::new(
                                ErrorKind::InvalidInput,
                                "concurrently created component is not a directory",
                            ));
                        }
                    }
                    Err(ce) => return Err(ce),
                },
                Err(e) => return Err(e),
            }
        }
        if let Some(parent) = dir.parent() {
            let _ = self.fsync_dir(parent);
        }
        Ok(())
    }

    fn rmdir_if_empty(&self, dir: &Path) -> io::Result<bool> {
        match std::fs::remove_dir(dir) {
            Ok(()) => {
                if let Some(parent) = dir.parent() {
                    let _ = self.fsync_dir(parent);
                }
                Ok(true)
            }
            Err(e)
                if e.kind() == ErrorKind::DirectoryNotEmpty
                    || e.kind() == ErrorKind::NotFound
                    || e.raw_os_error() == Some(libc::ENOTEMPTY) =>
            {
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    fn rename_excl(&self, from: &Path, to: &Path) -> io::Result<()> {
        rename::rename_excl_impl(self, from, to)
    }

    fn copy_excl(&self, from: &Path, to: &Path) -> io::Result<CopyOutput> {
        copy::copy_excl_impl(self, from, to)
    }

    fn hash_file(&self, path: &Path) -> io::Result<(u64, Blake3Hash)> {
        copy::hash_file_impl(path)
    }

    fn verify_copied(
        &self,
        from: &Path,
        expected_src: &EntryIdentity,
        dst: &Path,
        copied: &CopyOutput,
    ) -> io::Result<VerifiedSourceToken> {
        let current_src = self.lstat(from)?;
        if !current_src.same_version(expected_src) {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "source file changed during copy verification",
            ));
        }
        let dst_stat = self.lstat(dst)?;
        if dst_stat.size != copied.bytes_copied {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "destination size mismatch during verification",
            ));
        }
        let (rehashed_size, rehashed_digest) = self.hash_file(dst)?;
        if rehashed_size != copied.bytes_copied || rehashed_digest != copied.blake3 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "destination BLAKE3 digest mismatch during verification",
            ));
        }
        Ok(VerifiedSourceToken {
            source_path: from.to_path_buf(),
            source_identity: current_src,
            blake3: copied.blake3,
        })
    }

    fn remove_verified_source(&self, from: &Path, token: VerifiedSourceToken) -> io::Result<()> {
        if from != token.source_path {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "remove_verified_source path does not match VerifiedSourceToken",
            ));
        }
        let current_src = self.lstat(from)?;
        if !current_src.same_version(&token.source_identity) {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "source file changed after verification; refusing to unlink",
            ));
        }
        std::fs::remove_file(from)?;
        if let Some(parent) = from.parent() {
            self.fsync_dir(parent)?;
        }
        Ok(())
    }

    fn move_to_trash(&self, path: &Path) -> io::Result<PathBuf> {
        trash::move_to_trash_impl(self, path, self.trash_dir_override.as_deref())
    }

    fn stat_fs_free_bytes(&self, path: &Path) -> io::Result<u64> {
        let mut cursor = Some(path);
        let existing = loop {
            match cursor {
                Some(p) if p.exists() => break p,
                Some(p) => cursor = p.parent(),
                None => break Path::new("/"),
            }
        };
        let c_path = CString::new(existing.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "path contains interior NUL"))?;

        #[allow(unsafe_code)]
        // SAFETY: `c_path` is a valid NUL-terminated C string and `stat` is zero-initialized.
        let (rc, stat) = unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            let rc = libc::statvfs(c_path.as_ptr(), &mut stat);
            (rc, stat)
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(u64::from(stat.f_bavail).saturating_mul(stat.f_frsize))
    }

    fn fsync_dir(&self, dir: &Path) -> io::Result<()> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY)
            .open(dir)?;
        #[allow(unsafe_code)]
        // SAFETY: `file.as_raw_fd()` is a valid open directory descriptor for the duration of the call.
        let rc = unsafe { libc::fsync(file.as_raw_fd()) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        self.record_fsync();
        Ok(())
    }

    fn fsync_count(&self) -> usize {
        self.fsync_counter.load(std::sync::atomic::Ordering::SeqCst)
    }
}
