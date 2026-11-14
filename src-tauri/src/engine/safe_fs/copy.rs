//! Exclusive streaming copy (`O_CREAT | O_EXCL`, 64 KiB buffer, BLAKE3, `fsync`).
#![allow(clippy::disallowed_methods)]

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::{Blake3Hash, CopyOutput, Fs, SafeFs, STREAM_BUF_BYTES};

/// Streams `from` into a newly created `to` (`O_CREAT | O_EXCL`), computing a BLAKE3
/// digest on the fly and calling `sync_all` on both the destination file and its parent directory.
pub(crate) fn copy_excl_impl(fs: &SafeFs, from: &Path, to: &Path) -> io::Result<CopyOutput> {
    let mut reader = open_regular_no_follow(from)?;
    let mut writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(to)?;

    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; STREAM_BUF_BYTES];
    let mut bytes_copied: u64 = 0;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        bytes_copied = bytes_copied.saturating_add(n as u64);
    }

    writer.flush()?;
    writer.sync_all()?;
    fs.record_fsync();
    drop(writer);

    if let Some(parent) = to.parent() {
        fs.fsync_dir(parent)?;
    }

    let digest = hasher.finalize();
    Ok(CopyOutput {
        bytes_copied,
        blake3: Blake3Hash(*digest.as_bytes()),
    })
}

/// Streams `path` in 64 KiB chunks (`O_NOFOLLOW | O_NONBLOCK`) and computes `(size, Blake3Hash)`.
pub(crate) fn hash_file_impl(path: &Path) -> io::Result<(u64, Blake3Hash)> {
    let mut reader = open_regular_no_follow(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; STREAM_BUF_BYTES];
    let mut total: u64 = 0;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total = total.saturating_add(n as u64);
    }

    Ok((total, Blake3Hash(*hasher.finalize().as_bytes())))
}

/// Writes up to `max_bytes` from `from` into a newly created `to` (`create_new(true)`)
/// to simulate a mid-copy crash or `ENOSPC` fault (`FaultKind::PartialCopy`).
pub(crate) fn write_partial_copy(from: &Path, to: &Path, max_bytes: usize) -> io::Result<()> {
    let mut reader = open_regular_no_follow(from)?;
    let mut writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(to)?;
    let mut buf = vec![0u8; max_bytes.min(STREAM_BUF_BYTES)];
    let mut remaining = max_bytes;
    while remaining > 0 {
        let chunk = remaining.min(buf.len());
        let n = reader.read(&mut buf[..chunk])?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        remaining -= n;
    }
    writer.flush()?;
    Ok(())
}

fn open_regular_no_follow(path: &Path) -> io::Result<std::fs::File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(file)
}
