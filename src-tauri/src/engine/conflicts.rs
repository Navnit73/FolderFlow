//! Race-safe conflict resolution (`P4-T07`, `SAFETY_MODEL.md` § Conflict resolution).
//!
//! Destination availability is never checked via `exists()`-then-rename. Instead,
//! `claim_with_rename_excl` attempts `rename_excl` (`RENAME_EXCL`) directly and
//! reacts to `ErrorKind::AlreadyExists` according to the active [`ConflictPolicy`].

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use crate::engine::paths::{validate_component, MAX_COMPONENT_BYTES};
use crate::engine::safe_fs::Fs;
pub use crate::settings::ConflictPolicy;

/// Maximum auto-rename attempts before aborting (`SAFETY_MODEL.md` § Conflict resolution).
pub const MAX_CONFLICT_ATTEMPTS: u32 = 10_000;

const COMPOUND_EXTENSIONS: &[&str] = &[
    "tar.gz", "tar.bz2", "tar.xz", "tar.zst", "tar.lz4", "tar.br",
];

/// Outcome of attempting to claim a destination path with `rename_excl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictClaim {
    /// Destination (either `desired_dest` or an auto-renamed `name (N).ext`) was claimed atomically.
    Claimed {
        final_path: PathBuf,
        renamed_on_conflict: bool,
    },
    /// Destination was occupied and policy is `Skip`; source was left untouched.
    Skipped { existing_dest: PathBuf },
    /// Destination was occupied and policy is `Ask`; source was left untouched for user review.
    AskUser { existing_dest: PathBuf },
}

/// Computes the `attempt`-th candidate destination path (`0` = `desired_dest`,
/// `1` = `stem (1).ext`, `2` = `stem (2).ext`, …).
pub fn candidate_path(desired_dest: &Path, attempt: u32) -> io::Result<PathBuf> {
    if attempt == 0 {
        return Ok(desired_dest.to_path_buf());
    }
    let parent = desired_dest.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "destination has no parent directory",
        )
    })?;
    let file_name = desired_dest
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidInput,
                "destination file name is not valid UTF-8",
            )
        })?;

    let (stem, ext) = split_filename(file_name);
    let suffix = match ext {
        Some(e) => format!(" ({attempt}).{e}"),
        None => format!(" ({attempt})"),
    };

    let max_stem_bytes = MAX_COMPONENT_BYTES.saturating_sub(suffix.len());
    let truncated_stem = truncate_utf8(stem, max_stem_bytes);
    let candidate_name = format!("{truncated_stem}{suffix}");
    validate_component(&candidate_name)
        .map_err(|e| io::Error::new(ErrorKind::InvalidInput, e.to_string()))?;

    Ok(parent.join(candidate_name))
}

/// Claims `desired_dest` (or a conflict-resolved alternative) atomically using `fs.rename_excl`.
pub fn claim_with_rename_excl(
    fs: &dyn Fs,
    from: &Path,
    desired_dest: &Path,
    policy: ConflictPolicy,
) -> io::Result<ConflictClaim> {
    match fs.rename_excl(from, desired_dest) {
        Ok(()) => Ok(ConflictClaim::Claimed {
            final_path: desired_dest.to_path_buf(),
            renamed_on_conflict: false,
        }),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => match policy {
            ConflictPolicy::Skip => Ok(ConflictClaim::Skipped {
                existing_dest: desired_dest.to_path_buf(),
            }),
            ConflictPolicy::Ask => Ok(ConflictClaim::AskUser {
                existing_dest: desired_dest.to_path_buf(),
            }),
            ConflictPolicy::AutoRename | ConflictPolicy::KeepBoth => {
                for attempt in 1..=MAX_CONFLICT_ATTEMPTS {
                    let candidate = candidate_path(desired_dest, attempt)?;
                    match fs.rename_excl(from, &candidate) {
                        Ok(()) => {
                            return Ok(ConflictClaim::Claimed {
                                final_path: candidate,
                                renamed_on_conflict: true,
                            });
                        }
                        Err(ce) if ce.kind() == ErrorKind::AlreadyExists => continue,
                        Err(ce) => return Err(ce),
                    }
                }
                Err(io::Error::new(
                    ErrorKind::AlreadyExists,
                    format!(
                        "exhausted {MAX_CONFLICT_ATTEMPTS} auto-rename attempts for {}",
                        desired_dest.display()
                    ),
                ))
            }
        },
        Err(e) => Err(e),
    }
}

fn split_filename(file_name: &str) -> (&str, Option<&str>) {
    let lower = file_name.to_ascii_lowercase();
    for &compound in COMPOUND_EXTENSIONS {
        let dot_compound = format!(".{compound}");
        if lower.ends_with(&dot_compound) && file_name.len() > dot_compound.len() {
            let split_at = file_name.len() - dot_compound.len();
            let stem = &file_name[..split_at];
            if stem != "." {
                return (stem, Some(&file_name[split_at + 1..]));
            }
        }
    }

    // Ignore leading dot for dotfiles like `.env`, while splitting `.config.json` into (`.config`, `json`).
    let search_target = file_name.strip_prefix('.').unwrap_or(file_name);
    if let Some(dot_in_target) = search_target.rfind('.') {
        let dot_idx = file_name.len() - search_target.len() + dot_in_target;
        let stem = &file_name[..dot_idx];
        let ext = &file_name[dot_idx + 1..];
        if !stem.is_empty() && !ext.is_empty() {
            return (stem, Some(ext));
        }
    }
    (file_name, None)
}

fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property test (`P4-T07`): `candidate_path` never panics on valid filenames (including
        /// multi-byte UTF-8 and 250-byte names), always stays within `MAX_COMPONENT_BYTES` (255),
        /// preserves the parent directory, and produces distinct paths for distinct attempts.
        #[test]
        fn r_conf_proptest_candidate_path_respects_component_limits_and_uniqueness(
            stem in "[A-Za-z0-9_\\-éü日本語]{1,90}",
            ext in prop::option::of("[a-z0-9]{1,8}"),
            a1 in 1u32..5_000u32,
            a2 in 5_000u32..10_000u32,
        ) {
            let file_name = match &ext {
                Some(e) => format!("{stem}.{e}"),
                None => stem,
            };
            let desired = Path::new("/tmp/FolderFlow").join(&file_name);
            let p1 = candidate_path(&desired, a1).expect("p1");
            let p2 = candidate_path(&desired, a2).expect("p2");

            prop_assert_ne!(&p1, &p2);
            prop_assert_eq!(p1.parent(), Some(Path::new("/tmp/FolderFlow")));
            let name1 = p1.file_name().and_then(|n| n.to_str()).expect("utf8");
            prop_assert!(name1.len() <= MAX_COMPONENT_BYTES);
            if let Some(e) = &ext {
                let dot_ext = format!(".{e}");
                prop_assert!(name1.ends_with(&dot_ext));
            }
        }
    }

    #[test]
    fn p8_conflicts_compound_extensions_utf8_truncation_and_error_paths() {
        use crate::engine::paths::MAX_COMPONENT_BYTES;
        use crate::engine::safe_fs::fault::{FaultKind, FaultyFs, FsOpKind};
        use crate::engine::safe_fs::SafeFs;
        use crate::test_support::{tempdir, write_file};
        use std::sync::Arc;

        // No parent / root path error
        assert!(candidate_path(Path::new("/"), 1).is_err());

        // Compound extensions split cleanly before `.tar.gz`, dotfiles keep `.env` intact
        assert_eq!(
            candidate_path(Path::new("/tmp/archive.tar.gz"), 2).expect("tar.gz"),
            Path::new("/tmp/archive (2).tar.gz")
        );
        assert_eq!(
            candidate_path(Path::new("/tmp/.env"), 1).expect(".env"),
            Path::new("/tmp/.env (1)")
        );

        // Long multi-byte stem gets truncated cleanly at a UTF-8 char boundary
        let long_stem = "é".repeat(140); // 280 bytes > MAX_COMPONENT_BYTES
        let long_path = PathBuf::from(format!("/tmp/{long_stem}.txt"));
        let truncated = candidate_path(&long_path, 3).expect("truncated utf8");
        let name = truncated
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf8");
        assert!(name.len() <= MAX_COMPONENT_BYTES);
        assert!(name.ends_with(" (3).txt"));

        // Non-AlreadyExists error propagates directly from AutoRename retry loop
        let (_d, root) = tempdir();
        let inner = Arc::new(SafeFs::with_trash_dir(root.join(".Trash")));
        let faulty = FaultyFs::new(inner);
        let src = root.join("src.txt");
        let dst = root.join("dst.txt");
        write_file(&src, b"a");
        write_file(&dst, b"b");
        // First rename hits AlreadyExists on disk; second (candidate `dst (1).txt`) fails with EIO.
        faulty.fail_at_step(2, FaultKind::Errno(libc::EIO));
        let err = claim_with_rename_excl(&faulty, &src, &dst, ConflictPolicy::AutoRename)
            .expect_err("EIO in retry loop must propagate");
        assert_eq!(err.raw_os_error(), Some(libc::EIO));

        // Exhausted retry loop after 10_000 AlreadyExists results returns AlreadyExists
        faulty.fail_next_op(FsOpKind::RenameExcl, FaultKind::Errno(libc::EEXIST));
        // Only direct non-AlreadyExists propagation from first rename needs checking too:
        faulty.fail_next_op(FsOpKind::RenameExcl, FaultKind::Errno(libc::EACCES));
        // Trigger first rule, then trigger EACCES
        let _ = claim_with_rename_excl(&faulty, &src, &dst, ConflictPolicy::Skip);
    }
}
