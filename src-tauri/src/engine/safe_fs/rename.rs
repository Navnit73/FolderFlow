//! Exclusive atomic rename (`renamex_np(RENAME_EXCL)` on macOS, `ADR-005` / `SAFETY_MODEL.md` S1).

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::{Fs, SafeFs};

/// Performs an exclusive rename (`RENAME_EXCL`) that fails with `AlreadyExists` (`EEXIST`)
/// if `to` already exists (including a dangling symlink), and `EXDEV` across volumes.
pub(crate) fn rename_excl_impl(fs: &SafeFs, from: &Path, to: &Path) -> io::Result<()> {
    let c_from = CString::new(from.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "source path contains interior NUL",
        )
    })?;
    let c_to = CString::new(to.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination path contains interior NUL",
        )
    })?;

    #[cfg(target_os = "macos")]
    let rc = {
        #[allow(unsafe_code)]
        // SAFETY: `c_from` and `c_to` are valid NUL-terminated C strings live for the duration
        // of the FFI call. `RENAME_EXCL` is the documented macOS flag preventing overwrite.
        unsafe {
            libc::renamex_np(c_from.as_ptr(), c_to.as_ptr(), libc::RENAME_EXCL)
        }
    };

    #[cfg(not(target_os = "macos"))]
    let rc = {
        let _ = (&c_from, &c_to);
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "renamex_np requires macOS",
        ));
    };

    if rc != 0 {
        return Err(io::Error::last_os_error());
    }

    if let Some(parent) = to.parent() {
        fs.fsync_dir(parent)?;
    }
    Ok(())
}
