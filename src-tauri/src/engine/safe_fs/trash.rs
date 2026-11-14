//! Move-to-Trash without permanent deletion (`SAFETY_MODEL.md` S3).

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use objc2_foundation::{NSFileManager, NSString, NSURL};

use super::{Fs, SafeFs};

/// Relocates `path` to Trash without permanently deleting it.
///
/// - When `trash_dir_override` is `Some(dir)` (all automated tests), moves `path` into `dir`
///   using exclusive atomic rename (`rename_excl`) with collision-free suffixing (`name (N).ext`),
///   so tests never touch the user's real `~/.Trash`.
/// - When `trash_dir_override` is `None` (production), calls
///   `NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error`.
pub(crate) fn move_to_trash_impl(
    fs: &SafeFs,
    path: &Path,
    trash_dir_override: Option<&Path>,
) -> io::Result<PathBuf> {
    if let Some(trash_dir) = trash_dir_override {
        return move_into_trash_dir(fs, path, trash_dir);
    }
    trash_via_ns_file_manager(path)
}

fn move_into_trash_dir(fs: &SafeFs, path: &Path, trash_dir: &Path) -> io::Result<PathBuf> {
    // Verify source exists before creating trash_dir.
    let pre = fs.lstat(path)?;
    fs.mkdir_p(trash_dir)?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "invalid file name for trash"))?;

    let (stem, ext) = split_name_for_collision(file_name);

    for attempt in 0..=10_000u32 {
        let candidate_name = if attempt == 0 {
            file_name.to_owned()
        } else {
            match ext {
                Some(e) => format!("{stem} ({attempt}).{e}"),
                None => format!("{stem} ({attempt})"),
            }
        };
        let target = trash_dir.join(candidate_name);
        match fs.rename_excl(path, &target) {
            Ok(()) => return Ok(target),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                // Cross-volume fallback to Trash dir: copy_excl -> verify -> remove_verified_source
                match fs.copy_excl(path, &target) {
                    Ok(copied) => {
                        let token = fs.verify_copied(path, &pre, &target, &copied)?;
                        fs.remove_verified_source(path, token)?;
                        return Ok(target);
                    }
                    Err(ce) if ce.kind() == ErrorKind::AlreadyExists => continue,
                    Err(ce) => return Err(ce),
                }
            }
            Err(e) => return Err(e),
        }
    }

    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "exceeded 10,000 collision attempts in Trash directory",
    ))
}

fn trash_via_ns_file_manager(path: &Path) -> io::Result<PathBuf> {
    let path_str = path
        .to_str()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "path is not valid UTF-8"))?;
    let ns_path = NSString::from_str(path_str);
    let url = NSURL::fileURLWithPath(&ns_path);
    let mut resulting = None;
    let manager = NSFileManager::defaultManager();
    manager
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|ns_err| {
            let code = ns_err.code();
            // NSFileNoSuchFileError = 4, NSFileReadNoSuchFileError = 260
            if code == 4 || code == 260 {
                io::Error::new(ErrorKind::NotFound, ns_err.to_string())
            } else if code == 257 || code == 513 {
                // NSFileReadNoPermissionError = 257, NSFileWriteNoPermissionError = 513
                io::Error::new(ErrorKind::PermissionDenied, ns_err.to_string())
            } else {
                io::Error::other(ns_err.to_string())
            }
        })?;

    if let Some(res_url) = resulting {
        if let Some(res_path) = res_url.path() {
            return Ok(PathBuf::from(res_path.to_string()));
        }
    }
    Ok(PathBuf::from(path_str))
}

pub(crate) fn split_name_for_collision(file_name: &str) -> (&str, Option<&str>) {
    // Hidden file like `.env` has no extension; `.archive.tar` splits at the last dot.
    let search_target = file_name.strip_prefix('.').unwrap_or(file_name);
    if let Some(dot_idx_in_rest) = search_target.rfind('.') {
        let dot_idx = file_name.len() - search_target.len() + dot_idx_in_rest;
        let stem = &file_name[..dot_idx];
        let ext = &file_name[dot_idx + 1..];
        if !stem.is_empty() && !ext.is_empty() {
            return (stem, Some(ext));
        }
    }
    (file_name, None)
}
