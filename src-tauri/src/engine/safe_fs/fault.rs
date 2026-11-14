//! Deterministic fault-injecting [`Fs`] wrapper (`P4-T02`).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::copy::write_partial_copy;
use super::{Blake3Hash, CopyOutput, EntryIdentity, Fs, VerifiedSourceToken};

/// Mutating operation kinds tracked by [`FaultyFs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsOpKind {
    MkdirP,
    RenameExcl,
    CopyExcl,
    VerifyCopied,
    RemoveVerifiedSource,
    MoveToTrash,
    FsyncDir,
    StatFsFreeBytes,
}

/// Fault specification for [`FaultyFs`].
#[derive(Debug, Clone)]
pub enum FaultKind {
    /// Return an OS errno (`libc::ENOSPC`, `libc::EACCES`, `libc::ENOENT`, `libc::EXDEV`, `libc::EIO`).
    Errno(i32),
    /// Write `bytes` of partial data to `to` during `copy_excl`, then fail with `errno`.
    PartialCopy { bytes: usize, errno: i32 },
}

impl FaultKind {
    pub fn to_io_error(&self) -> io::Error {
        match self {
            Self::Errno(code) | Self::PartialCopy { errno: code, .. } => {
                io::Error::from_raw_os_error(*code)
            }
        }
    }
}

#[derive(Debug, Clone)]
struct FaultRule {
    at_step: Option<usize>,
    on_op: Option<FsOpKind>,
    kind: FaultKind,
    remaining_triggers: usize,
}

type MutationHook = Arc<dyn Fn(FsOpKind, usize) + Send + Sync>;

/// Wrapper around an inner [`Fs`] that records operations and injects deterministic faults.
pub struct FaultyFs {
    inner: Arc<dyn Fs>,
    state: Mutex<FaultState>,
    mutation_hook: Mutex<Option<MutationHook>>,
}

#[derive(Default)]
struct FaultState {
    step: usize,
    history: Vec<(usize, FsOpKind, PathBuf)>,
    rules: Vec<FaultRule>,
    cross_volume_pairs: Vec<(PathBuf, PathBuf)>,
    free_bytes_override: Option<u64>,
}

impl FaultyFs {
    pub fn new(inner: Arc<dyn Fs>) -> Self {
        Self {
            inner,
            state: Mutex::new(FaultState::default()),
            mutation_hook: Mutex::new(None),
        }
    }

    /// Injects `kind` on the `step`-th mutating operation (1-indexed).
    pub fn fail_at_step(&self, step: usize, kind: FaultKind) {
        if let Ok(mut st) = self.state.lock() {
            st.rules.push(FaultRule {
                at_step: Some(step),
                on_op: None,
                kind,
                remaining_triggers: 1,
            });
        }
    }

    /// Injects `kind` the next time `op` is called.
    pub fn fail_next_op(&self, op: FsOpKind, kind: FaultKind) {
        if let Ok(mut st) = self.state.lock() {
            st.rules.push(FaultRule {
                at_step: None,
                on_op: Some(op),
                kind,
                remaining_triggers: 1,
            });
        }
    }

    /// Forces `rename_excl(from, to)` to return `EXDEV` whenever `from` is under `src_prefix`
    /// and `to` is under `dst_prefix` (simulating cross-volume moves on a single disk).
    pub fn simulate_cross_volume(&self, src_prefix: PathBuf, dst_prefix: PathBuf) {
        if let Ok(mut st) = self.state.lock() {
            st.cross_volume_pairs.push((src_prefix, dst_prefix));
        }
    }

    /// Overrides `stat_fs_free_bytes` return value.
    pub fn set_free_bytes(&self, bytes: Option<u64>) {
        if let Ok(mut st) = self.state.lock() {
            st.free_bytes_override = bytes;
        }
    }

    /// Registers a hook invoked immediately before any mutating operation (`R-LEDG-01`).
    pub fn set_on_mutation(&self, hook: impl Fn(FsOpKind, usize) + Send + Sync + 'static) {
        if let Ok(mut guard) = self.mutation_hook.lock() {
            *guard = Some(Arc::new(hook));
        }
    }

    pub fn history(&self) -> Vec<(usize, FsOpKind, PathBuf)> {
        self.state
            .lock()
            .map(|s| s.history.clone())
            .unwrap_or_default()
    }

    pub fn step_count(&self) -> usize {
        self.state.lock().map(|s| s.step).unwrap_or(0)
    }

    fn intercept(&self, op: FsOpKind, path: &Path) -> Option<FaultKind> {
        let (step, matched) = {
            let Ok(mut st) = self.state.lock() else {
                return None;
            };
            st.step += 1;
            let current_step = st.step;
            st.history.push((current_step, op, path.to_path_buf()));

            let mut found = None;
            for rule in &mut st.rules {
                if rule.remaining_triggers == 0 {
                    continue;
                }
                let step_matches = rule.at_step.is_none_or(|s| s == current_step);
                let op_matches = rule.on_op.is_none_or(|o| o == op);
                if step_matches && op_matches {
                    rule.remaining_triggers -= 1;
                    found = Some(rule.kind.clone());
                    break;
                }
            }
            (current_step, found)
        };

        let hook = self.mutation_hook.lock().ok().and_then(|g| g.clone());
        if let Some(cb) = hook {
            cb(op, step);
        }
        matched
    }

    fn is_simulated_cross_volume(&self, from: &Path, to: &Path) -> bool {
        let Ok(st) = self.state.lock() else {
            return false;
        };
        st.cross_volume_pairs.iter().any(|(a, b)| {
            (from.starts_with(a) && to.starts_with(b)) || (from.starts_with(b) && to.starts_with(a))
        })
    }
}

impl Fs for FaultyFs {
    fn lstat(&self, path: &Path) -> io::Result<EntryIdentity> {
        self.inner.lstat(path)
    }

    fn ensure_no_symlink_ancestors(&self, path: &Path, stop_at_root: &Path) -> io::Result<()> {
        self.inner.ensure_no_symlink_ancestors(path, stop_at_root)
    }

    fn mkdir_p(&self, dir: &Path) -> io::Result<()> {
        if let Some(fault) = self.intercept(FsOpKind::MkdirP, dir) {
            return Err(fault.to_io_error());
        }
        self.inner.mkdir_p(dir)
    }

    fn rmdir_if_empty(&self, dir: &Path) -> io::Result<bool> {
        self.inner.rmdir_if_empty(dir)
    }

    fn rename_excl(&self, from: &Path, to: &Path) -> io::Result<()> {
        if let Some(fault) = self.intercept(FsOpKind::RenameExcl, to) {
            return Err(fault.to_io_error());
        }
        if self.is_simulated_cross_volume(from, to) {
            return Err(io::Error::from_raw_os_error(libc::EXDEV));
        }
        self.inner.rename_excl(from, to)
    }

    fn copy_excl(&self, from: &Path, to: &Path) -> io::Result<CopyOutput> {
        if let Some(fault) = self.intercept(FsOpKind::CopyExcl, to) {
            if let FaultKind::PartialCopy { bytes, errno } = fault {
                let _ = write_partial_copy(from, to, bytes);
                return Err(io::Error::from_raw_os_error(errno));
            }
            return Err(fault.to_io_error());
        }
        self.inner.copy_excl(from, to)
    }

    fn hash_file(&self, path: &Path) -> io::Result<(u64, Blake3Hash)> {
        self.inner.hash_file(path)
    }

    fn verify_copied(
        &self,
        from: &Path,
        expected_src: &EntryIdentity,
        dst: &Path,
        copied: &CopyOutput,
    ) -> io::Result<VerifiedSourceToken> {
        if let Some(fault) = self.intercept(FsOpKind::VerifyCopied, dst) {
            return Err(fault.to_io_error());
        }
        self.inner.verify_copied(from, expected_src, dst, copied)
    }

    fn remove_verified_source(&self, from: &Path, token: VerifiedSourceToken) -> io::Result<()> {
        if let Some(fault) = self.intercept(FsOpKind::RemoveVerifiedSource, from) {
            return Err(fault.to_io_error());
        }
        self.inner.remove_verified_source(from, token)
    }

    fn move_to_trash(&self, path: &Path) -> io::Result<PathBuf> {
        if let Some(fault) = self.intercept(FsOpKind::MoveToTrash, path) {
            return Err(fault.to_io_error());
        }
        self.inner.move_to_trash(path)
    }

    fn stat_fs_free_bytes(&self, path: &Path) -> io::Result<u64> {
        if let Some(fault) = self.intercept(FsOpKind::StatFsFreeBytes, path) {
            return Err(fault.to_io_error());
        }
        if let Ok(st) = self.state.lock() {
            if let Some(override_bytes) = st.free_bytes_override {
                return Ok(override_bytes);
            }
        }
        self.inner.stat_fs_free_bytes(path)
    }

    fn fsync_dir(&self, dir: &Path) -> io::Result<()> {
        if let Some(fault) = self.intercept(FsOpKind::FsyncDir, dir) {
            return Err(fault.to_io_error());
        }
        self.inner.fsync_dir(dir)
    }

    fn fsync_count(&self) -> usize {
        self.inner.fsync_count()
    }
}
