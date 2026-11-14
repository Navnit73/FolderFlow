//! Streaming BLAKE3 full and partial file hashers (`P6-T01`, FR-110, NFR-013).
//!
//! Uses a fixed 64 KiB buffer so memory usage stays constant regardless of file size
//! (verified by `r_hash_03` on a 2 GiB sparse file).

use std::io::{ErrorKind, Read, SeekFrom};
use std::path::Path;

use crate::engine::fs_view::{ReadOnlyFs, ReadSeek};

/// Fixed read buffer capacity for all streaming hashes (64 KiB).
pub const HASH_BUFFER_SIZE: usize = 64 * 1024;
/// Chunk length sampled at start, middle, and end for Stage-2 partial hashing (64 KiB).
pub const PARTIAL_CHUNK_SIZE: u64 = 64 * 1024;

/// Computes the full BLAKE3 hex digest from any reader using a single fixed buffer.
pub fn full_hash_reader<R: Read + ?Sized>(reader: &mut R) -> std::io::Result<String> {
    full_hash_reader_with_progress(reader, |_| {})
}

/// Computes the full BLAKE3 hex digest, invoking `on_chunk(bytes_read)` after each buffer read.
pub fn full_hash_reader_with_progress<R: Read + ?Sized, F: FnMut(u64)>(
    reader: &mut R,
    mut on_chunk: F,
) -> std::io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; HASH_BUFFER_SIZE];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                hasher.update(&buf[..n]);
                on_chunk(n as u64);
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Computes the full BLAKE3 hex digest of a regular file via [`ReadOnlyFs`] (`O_NOFOLLOW | O_NONBLOCK`).
pub fn full_hash_file(fs: &dyn ReadOnlyFs, path: &Path) -> std::io::Result<String> {
    let mut stream = fs.open_read(path)?;
    full_hash_reader(&mut *stream)
}

/// Computes the full BLAKE3 hex digest of a regular file while reporting bytes streamed.
pub fn full_hash_file_with_progress<F: FnMut(u64)>(
    fs: &dyn ReadOnlyFs,
    path: &Path,
    on_chunk: F,
) -> std::io::Result<String> {
    let mut stream = fs.open_read(path)?;
    full_hash_reader_with_progress(&mut *stream, on_chunk)
}

/// Computes a fast Stage-2 partial fingerprint sampling start, middle, and end windows.
///
/// Only intended for comparing candidates that already share the same byte `known_size`.
pub fn partial_hash_file(
    fs: &dyn ReadOnlyFs,
    path: &Path,
    known_size: u64,
) -> std::io::Result<String> {
    let mut stream = fs.open_read(path)?;
    partial_hash_reader(&mut *stream, known_size)
}

/// Computes the partial fingerprint over an already opened seekable stream.
pub fn partial_hash_reader(reader: &mut dyn ReadSeek, known_size: u64) -> std::io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"FF-PARTIAL-v1:");
    hasher.update(&known_size.to_le_bytes());

    let mut buf = vec![0u8; HASH_BUFFER_SIZE];
    if known_size <= PARTIAL_CHUNK_SIZE * 3 {
        hash_window(reader, &mut buf, &mut hasher, 0, known_size)?;
    } else {
        let start = 0;
        let mid = (known_size / 2).saturating_sub(PARTIAL_CHUNK_SIZE / 2);
        let end = known_size.saturating_sub(PARTIAL_CHUNK_SIZE);
        for offset in [start, mid, end] {
            hasher.update(&offset.to_le_bytes());
            hash_window(reader, &mut buf, &mut hasher, offset, PARTIAL_CHUNK_SIZE)?;
        }
    }

    Ok(format!("partial:{}", hasher.finalize().to_hex()))
}

fn hash_window(
    reader: &mut dyn ReadSeek,
    buf: &mut [u8],
    hasher: &mut blake3::Hasher,
    offset: u64,
    len: u64,
) -> std::io::Result<()> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut remaining = len;
    while remaining > 0 {
        let want = usize::try_from(remaining)
            .unwrap_or(buf.len())
            .min(buf.len());
        match reader.read(&mut buf[..want]) {
            Ok(0) => break,
            Ok(n) => {
                hasher.update(&buf[..n]);
                remaining = remaining.saturating_sub(n as u64);
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fs_view::RealFs;
    use crate::test_support::{symlink, tempdir, write_file, write_sparse_file};
    use std::io::{Cursor, Seek, SeekFrom};

    #[test]
    fn r_hash_01_known_blake3_vectors_and_streaming_boundaries() {
        // Empty input vector (official BLAKE3 spec)
        let empty = full_hash_reader(&mut Cursor::new(b"")).expect("hash empty");
        assert_eq!(
            empty,
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );

        // Exact multi-chunk input spanning > 64 KiB boundary
        let data: Vec<u8> = (0..(HASH_BUFFER_SIZE * 2 + 123))
            .map(|i| (i % 251) as u8)
            .collect();
        let expected = blake3::hash(&data).to_hex().to_string();

        let (_d, root) = tempdir();
        let p = root.join("vector.bin");
        write_file(&p, &data);
        let got = full_hash_file(&RealFs, &p).expect("hash file");
        assert_eq!(got, expected);

        // Symlinks must be refused by O_NOFOLLOW
        let link = root.join("link.bin");
        symlink(&p, &link);
        assert!(full_hash_file(&RealFs, &link).is_err());
    }

    #[test]
    fn r_hash_02_partial_hash_distinguishes_start_middle_end_and_matches_equal() {
        let (_d, root) = tempdir();
        let len: usize = 512 * 1024; // 512 KiB (> 3 * 64 KiB)
        let base = vec![0xAAu8; len];

        let same = base.clone();
        let mut diff_start = base.clone();
        diff_start[17] ^= 0xFF;
        let mut diff_middle = base.clone();
        diff_middle[len / 2] ^= 0xFF;
        let mut diff_end = base.clone();
        diff_end[len - 9] ^= 0xFF;

        let p_base = root.join("base.bin");
        let p_same = root.join("same.bin");
        let p_start = root.join("start.bin");
        let p_mid = root.join("middle.bin");
        let p_end = root.join("end.bin");
        write_file(&p_base, &base);
        write_file(&p_same, &same);
        write_file(&p_start, &diff_start);
        write_file(&p_mid, &diff_middle);
        write_file(&p_end, &diff_end);

        let h_base = partial_hash_file(&RealFs, &p_base, len as u64).expect("base");
        let h_same = partial_hash_file(&RealFs, &p_same, len as u64).expect("same");
        let h_start = partial_hash_file(&RealFs, &p_start, len as u64).expect("start");
        let h_mid = partial_hash_file(&RealFs, &p_mid, len as u64).expect("middle");
        let h_end = partial_hash_file(&RealFs, &p_end, len as u64).expect("end");

        assert!(!h_base.is_empty());
        assert_eq!(h_base, h_same, "identical files share partial hash");
        assert_ne!(h_base, h_start, "start diff must change partial hash");
        assert_ne!(h_base, h_mid, "middle diff must change partial hash");
        assert_ne!(h_base, h_end, "end diff must change partial hash");
    }

    struct BoundedReadChecker {
        total_len: u64,
        pos: u64,
        max_buf_seen: usize,
    }

    impl Read for BoundedReadChecker {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.max_buf_seen = self.max_buf_seen.max(buf.len());
            assert!(
                buf.len() <= HASH_BUFFER_SIZE,
                "streaming hasher buffer {} exceeded {}",
                buf.len(),
                HASH_BUFFER_SIZE
            );
            let rem = self.total_len.saturating_sub(self.pos);
            let n = buf.len().min(usize::try_from(rem).unwrap_or(usize::MAX));
            buf[..n].fill(0);
            self.pos += n as u64;
            Ok(n)
        }
    }

    impl Seek for BoundedReadChecker {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            match pos {
                SeekFrom::Start(p) => self.pos = p.min(self.total_len),
                SeekFrom::End(d) => {
                    self.pos = (self.total_len as i128 + d as i128).clamp(0, self.total_len as i128)
                        as u64;
                }
                SeekFrom::Current(d) => {
                    self.pos =
                        (self.pos as i128 + d as i128).clamp(0, self.total_len as i128) as u64;
                }
            }
            Ok(self.pos)
        }
    }

    #[test]
    fn r_hash_03_2gb_sparse_file_streams_with_fixed_buffer() {
        const TWO_GIB: u64 = 2 * 1024 * 1024 * 1024;
        assert!(std::mem::size_of::<blake3::Hasher>() < 4096);

        // 1. Verify read buffer bound over a 2 GiB stream without allocating large memory
        let mut checker = BoundedReadChecker {
            total_len: TWO_GIB,
            pos: 0,
            max_buf_seen: 0,
        };
        let mut streamed_bytes = 0u64;
        let zero_hash = full_hash_reader_with_progress(&mut checker, |n| streamed_bytes += n)
            .expect("stream 2 GiB virtual");
        assert_eq!(streamed_bytes, TWO_GIB);
        assert_eq!(checker.max_buf_seen, HASH_BUFFER_SIZE);
        assert!(!zero_hash.is_empty());

        // 2. Hash an actual 2 GiB sparse file on disk via RealFs
        let (_d, root) = tempdir();
        let sparse = root.join("sparse-2gb.bin");
        write_sparse_file(&sparse, TWO_GIB, b"HEAD", b"TAIL");
        let sparse_hash = full_hash_file(&RealFs, &sparse).expect("hash 2 GiB sparse file");
        assert_eq!(sparse_hash.len(), 64);
        assert_ne!(sparse_hash, zero_hash);

        // Second sparse file with identical markers has identical digest
        let sparse2 = root.join("sparse-2gb-copy.bin");
        write_sparse_file(&sparse2, TWO_GIB, b"HEAD", b"TAIL");
        assert_eq!(
            full_hash_file(&RealFs, &sparse2).expect("hash copy"),
            sparse_hash
        );
    }
}
