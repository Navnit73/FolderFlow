//! Live folder monitoring (FR-012 – FR-025).
//!
//! Each monitored folder runs one supervised task:
//!
//! ```text
//! availability check ─▶ start watcher ─▶ reconcile ─▶ watch loop
//!        ▲   (Unavailable / PermissionDenied / Error + backoff)   │
//!        └────────────────────────────────────────────────────────┘
//! watch loop: raw event ─▶ debounce ─▶ re-stat ─▶ stability ─▶ classify ─▶ record ─▶ observer
//! ```
//!
//! Watcher events are hints, never facts: every path is re-`stat`ed, queues
//! are bounded, and any overflow or watcher rescan request falls back to a
//! reconcile scan (FR-014). Phase 2 is read-only: "stable" files are recorded
//! and reported to the observer; organizing them arrives in Phase 5.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use specta::Type;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::task::JoinHandle;

use crate::database::{repo_files, repo_folders, Db};
use crate::engine::classifier::{Classifier, HEADER_BYTES};
use crate::engine::debounce::{Debouncer, Push};
use crate::engine::fs_view::{EntryKind, FileFacts, ReadOnlyFs, RealFs, PACKAGE_EXTENSIONS};
use crate::engine::paths::{nfc, PathError};
use crate::engine::reconcile::{reconcile, FolderScanSpec, ReconcileMode};
use crate::engine::scanner::{package_size, ScannedFile};
use crate::engine::stability::{Observation, StabilityEvent, StabilityTracker};
use crate::error::AppError;
use crate::folders::{probe_access, FolderAccess, FolderHealth, MonitoredFolder};
use crate::settings::MonitoringSettings;
use crate::utils::time::now_ms;

/// Raw watcher → folder task channel capacity. When full, the watcher sets
/// the overflow flag instead of blocking, and the task reconciles.
pub const RAW_CHANNEL_CAPACITY: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Starting,
    Scanning,
    Watching,
    Paused,
    Unavailable,
    PermissionDenied,
    Error,
    Stopped,
}

impl RuntimeState {
    /// Health persisted to `monitored_folders.health` for this state, if any.
    fn persisted_health(self) -> Option<FolderHealth> {
        match self {
            Self::Watching => Some(FolderHealth::Healthy),
            Self::Paused => Some(FolderHealth::Paused),
            Self::Unavailable => Some(FolderHealth::Unavailable),
            Self::PermissionDenied => Some(FolderHealth::PermissionDenied),
            Self::Error => Some(FolderHealth::Error),
            Self::Starting | Self::Scanning | Self::Stopped => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderRuntime {
    pub folder_id: String,
    pub state: RuntimeState,
    pub pending_events: u32,
    pub tracked_files: u32,
    pub last_error: Option<String>,
    #[specta(type = Option<f64>)]
    pub last_event_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringStatus {
    pub paused: bool,
    pub folders: Vec<FolderRuntime>,
}

pub trait MonitorObserver: Send + Sync {
    fn on_status(&self, status: &FolderRuntime);
    fn on_stable(&self, folder_id: &str, file: &ScannedFile);
    fn on_removed(&self, folder_id: &str, path: &Path);
}

#[derive(Debug)]
pub enum RawEvent {
    Paths(Vec<PathBuf>),
    Rescan,
    Error(String),
}

/// Creates a running watcher for `root`. The returned box keeps it alive;
/// dropping it stops watching. Injectable so tests can simulate failures.
pub type WatcherFactory = Arc<
    dyn Fn(&Path, bool, mpsc::Sender<RawEvent>, Arc<AtomicBool>) -> Result<Box<dyn Send>, String>
        + Send
        + Sync,
>;

/// FSEvents-backed watcher via `notify`.
pub fn default_watcher_factory() -> WatcherFactory {
    Arc::new(
        |root: &Path, recursive: bool, tx: mpsc::Sender<RawEvent>, overflow: Arc<AtomicBool>| {
            use notify::Watcher;
            let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                let msg = match res {
                    Ok(ev) if ev.need_rescan() => RawEvent::Rescan,
                    Ok(ev) => RawEvent::Paths(ev.paths),
                    Err(e) => RawEvent::Error(e.to_string()),
                };
                if tx.try_send(msg).is_err() {
                    overflow.store(true, Ordering::SeqCst);
                }
            })
            .map_err(|e| e.to_string())?;
            let mode = if recursive {
                notify::RecursiveMode::Recursive
            } else {
                notify::RecursiveMode::NonRecursive
            };
            w.watch(root, mode).map_err(|e| e.to_string())?;
            Ok(Box::new(w) as Box<dyn Send>)
        },
    )
}

#[derive(Debug, Clone)]
pub struct MonitorTiming {
    pub debounce: Duration,
    pub stability_interval: Duration,
    pub stability_checks: u32,
    pub reconcile_interval: Duration,
    pub health_interval: Duration,
    pub max_pending: usize,
    pub max_tracked: usize,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    /// How long `stop` waits for a folder task before cancelling it.
    pub stop_timeout: Duration,
}

impl MonitorTiming {
    pub fn from_settings(m: &MonitoringSettings) -> Self {
        Self {
            debounce: Duration::from_millis(500),
            stability_interval: Duration::from_secs(u64::from(m.stability_interval_secs)),
            stability_checks: m.stability_checks,
            reconcile_interval: Duration::from_secs(u64::from(m.reconciliation_interval_mins) * 60),
            health_interval: Duration::from_secs(30),
            max_pending: 10_000,
            max_tracked: 10_000,
            backoff_min: Duration::from_secs(1),
            backoff_max: Duration::from_secs(300),
            stop_timeout: Duration::from_secs(5),
        }
    }
}

pub struct MonitorDeps {
    pub db: Db,
    pub classifier: Arc<Classifier>,
    pub settings: MonitoringSettings,
    pub observer: Arc<dyn MonitorObserver>,
    pub timing: MonitorTiming,
    pub watcher_factory: WatcherFactory,
}

impl MonitorDeps {
    pub fn new(
        db: Db,
        classifier: Arc<Classifier>,
        settings: MonitoringSettings,
        observer: Arc<dyn MonitorObserver>,
    ) -> Self {
        Self {
            timing: MonitorTiming::from_settings(&settings),
            db,
            classifier,
            settings,
            observer,
            watcher_factory: default_watcher_factory(),
        }
    }
}

struct Shared {
    db: Db,
    classifier: std::sync::RwLock<Arc<Classifier>>,
    settings: std::sync::RwLock<MonitoringSettings>,
    observer: Arc<dyn MonitorObserver>,
    timing: MonitorTiming,
    watcher_factory: WatcherFactory,
}

impl Shared {
    fn classifier(&self) -> Arc<Classifier> {
        self.classifier
            .read()
            .map(|c| c.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }
    fn settings(&self) -> MonitoringSettings {
        self.settings
            .read()
            .map(|s| s.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Pause,
    Resume,
    Rescan,
    Stop,
}

struct FolderHandle {
    cmd: mpsc::Sender<Command>,
    task: JoinHandle<()>,
    status: watch::Receiver<FolderRuntime>,
}

#[derive(Default)]
struct Inner {
    folders: HashMap<String, FolderHandle>,
    global_paused: bool,
    user_paused: HashSet<String>,
}

/// Owns every folder task. Safe to call from commands, the tray and the scheduler.
pub struct MonitorService {
    shared: Arc<Shared>,
    inner: Mutex<Inner>,
}

impl MonitorService {
    pub fn new(deps: MonitorDeps) -> Self {
        Self {
            shared: Arc::new(Shared {
                db: deps.db,
                classifier: std::sync::RwLock::new(deps.classifier),
                settings: std::sync::RwLock::new(deps.settings),
                observer: deps.observer,
                timing: deps.timing,
                watcher_factory: deps.watcher_factory,
            }),
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Replaces the classifier (after category changes). Applies to new work.
    pub fn set_classifier(&self, c: Arc<Classifier>) {
        if let Ok(mut g) = self.shared.classifier.write() {
            *g = c;
        }
    }

    /// Updates ignore settings for folders started afterwards (restart to apply).
    pub fn set_settings(&self, s: MonitoringSettings) {
        if let Ok(mut g) = self.shared.settings.write() {
            *g = s;
        }
    }

    /// Starts (or restarts) monitoring a folder.
    pub async fn start(&self, folder: MonitoredFolder) {
        let mut inner = self.inner.lock().await;
        if let Some(old) = inner.folders.remove(&folder.id) {
            stop_handle(old, self.shared.timing.stop_timeout).await;
        }
        let paused = inner.global_paused || inner.user_paused.contains(&folder.id);
        let handle = spawn_folder(self.shared.clone(), folder.clone(), paused);
        inner.folders.insert(folder.id, handle);
    }

    pub async fn stop(&self, id: &str) {
        let mut inner = self.inner.lock().await;
        inner.user_paused.remove(id);
        if let Some(h) = inner.folders.remove(id) {
            stop_handle(h, self.shared.timing.stop_timeout).await;
        }
    }

    pub async fn pause(&self, id: &str) {
        let mut inner = self.inner.lock().await;
        inner.user_paused.insert(id.to_owned());
        if let Some(h) = inner.folders.get(id) {
            let _ = h.cmd.send(Command::Pause).await;
        }
    }

    pub async fn resume(&self, id: &str) {
        let mut inner = self.inner.lock().await;
        inner.user_paused.remove(id);
        if !inner.global_paused {
            if let Some(h) = inner.folders.get(id) {
                let _ = h.cmd.send(Command::Resume).await;
            }
        }
    }

    pub async fn rescan(&self, id: &str) {
        let inner = self.inner.lock().await;
        if let Some(h) = inner.folders.get(id) {
            let _ = h.cmd.send(Command::Rescan).await;
        }
    }

    /// Rescans every folder (e.g. after wake from sleep).
    pub async fn rescan_all(&self) {
        let inner = self.inner.lock().await;
        for h in inner.folders.values() {
            let _ = h.cmd.send(Command::Rescan).await;
        }
    }

    pub async fn pause_all(&self) {
        let mut inner = self.inner.lock().await;
        inner.global_paused = true;
        for h in inner.folders.values() {
            let _ = h.cmd.send(Command::Pause).await;
        }
    }

    pub async fn resume_all(&self) {
        let mut inner = self.inner.lock().await;
        inner.global_paused = false;
        for (id, h) in &inner.folders {
            if !inner.user_paused.contains(id) {
                let _ = h.cmd.send(Command::Resume).await;
            }
        }
    }

    pub async fn status(&self) -> MonitoringStatus {
        let inner = self.inner.lock().await;
        let mut folders: Vec<FolderRuntime> = inner
            .folders
            .values()
            .map(|h| h.status.borrow().clone())
            .collect();
        folders.sort_by(|a, b| a.folder_id.cmp(&b.folder_id));
        MonitoringStatus {
            paused: inner.global_paused,
            folders,
        }
    }

    pub async fn shutdown(&self) {
        let mut inner = self.inner.lock().await;
        for (_, h) in inner.folders.drain() {
            stop_handle(h, self.shared.timing.stop_timeout).await;
        }
    }
}

/// Asks a folder task to stop; cancels it if it doesn't within `timeout`.
/// Never blocks on a full command queue (BUG-010).
async fn stop_handle(mut h: FolderHandle, timeout: Duration) {
    let _ = h.cmd.try_send(Command::Stop);
    if tokio::time::timeout(timeout, &mut h.task).await.is_err() {
        tracing::warn!(target: "folderflow::monitor", "folder task did not stop in time; cancelling");
        h.task.abort();
    }
}

fn spawn_folder(shared: Arc<Shared>, folder: MonitoredFolder, paused: bool) -> FolderHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let initial = FolderRuntime {
        folder_id: folder.id.clone(),
        state: RuntimeState::Starting,
        pending_events: 0,
        tracked_files: 0,
        last_error: None,
        last_event_at: None,
    };
    let (status_tx, status_rx) = watch::channel(initial.clone());
    let settings = shared.settings();
    let timing = shared.timing.clone();
    let stability_interval = folder
        .settings
        .stability_interval_secs
        .map(|s| Duration::from_secs(u64::from(s)))
        .unwrap_or(timing.stability_interval);
    let task = FolderTask {
        spec: FolderScanSpec::from_folder(&folder, &settings),
        root: PathBuf::from(&folder.path),
        scanned_before: folder.last_scanned_at.is_some(),
        debouncer: Debouncer::new(timing.debounce, timing.max_pending),
        stability: StabilityTracker::new(
            stability_interval,
            timing.stability_checks,
            timing.max_tracked,
        ),
        reconcile_at: None,
        runtime: initial,
        paused,
        shared,
        cmd_rx,
        status_tx,
        folder,
    };
    FolderHandle {
        cmd: cmd_tx,
        task: tokio::spawn(task.run()),
        status: status_rx,
    }
}

enum Exit {
    Stop,
    Pause,
    /// Re-check availability and restart the watcher.
    Recheck,
    WatcherError(String),
}

enum Flow {
    Timeout,
    Command(Command),
}

struct FolderTask {
    shared: Arc<Shared>,
    folder: MonitoredFolder,
    spec: FolderScanSpec,
    root: PathBuf,
    cmd_rx: mpsc::Receiver<Command>,
    status_tx: watch::Sender<FolderRuntime>,
    runtime: FolderRuntime,
    paused: bool,
    scanned_before: bool,
    debouncer: Debouncer,
    stability: StabilityTracker,
    reconcile_at: Option<Instant>,
}

const MSG_UNAVAILABLE: &str =
    "The folder can't be found. It may have been moved, renamed or deleted, or its drive may be disconnected.";
const MSG_DENIED: &str =
    "FolderFlow no longer has permission to read this folder. Grant access in System Settings → Privacy & Security → Files and Folders.";

impl FolderTask {
    async fn run(mut self) {
        let t = self.shared.timing.clone();
        let mut backoff = t.backoff_min;
        loop {
            if self.paused {
                self.set_state(RuntimeState::Paused, None).await;
                match self.cmd_rx.recv().await {
                    Some(Command::Resume) => self.paused = false,
                    Some(Command::Stop) | None => return self.finish().await,
                    Some(_) => {}
                }
                continue;
            }

            match probe_access(&self.root) {
                FolderAccess::Granted => {}
                access => {
                    let (state, msg) = if access == FolderAccess::Denied {
                        (RuntimeState::PermissionDenied, MSG_DENIED)
                    } else {
                        (RuntimeState::Unavailable, MSG_UNAVAILABLE)
                    };
                    self.set_state(state, Some(msg.to_owned())).await;
                    if self.wait(t.health_interval).await.is_err() {
                        return self.finish().await;
                    }
                    continue;
                }
            }

            let (tx, rx) = mpsc::channel(RAW_CHANNEL_CAPACITY);
            let overflow = Arc::new(AtomicBool::new(false));
            let watcher = match (self.shared.watcher_factory)(
                &self.root,
                self.spec.recursive,
                tx,
                overflow.clone(),
            ) {
                Ok(w) => w,
                Err(msg) => {
                    self.watcher_error("watcher_start", &msg).await;
                    if self.wait(backoff).await.is_err() {
                        return self.finish().await;
                    }
                    backoff = (backoff * 2).min(t.backoff_max);
                    continue;
                }
            };

            self.set_state(RuntimeState::Scanning, None).await;
            if let Err(e) = self.reconcile_now().await {
                drop(watcher);
                if !self.on_reconcile_error(e).await {
                    if self.wait(backoff).await.is_err() {
                        return self.finish().await;
                    }
                    backoff = (backoff * 2).min(t.backoff_max);
                }
                continue;
            }
            backoff = t.backoff_min;
            self.set_state(RuntimeState::Watching, None).await;

            let exit = self.watch_loop(rx, &overflow).await;
            drop(watcher);
            match exit {
                Exit::Stop => return self.finish().await,
                Exit::Pause => self.paused = true,
                Exit::Recheck => {}
                Exit::WatcherError(msg) => {
                    self.watcher_error("watcher_runtime", &msg).await;
                    if self.wait(backoff).await.is_err() {
                        return self.finish().await;
                    }
                    backoff = (backoff * 2).min(t.backoff_max);
                }
            }
        }
    }

    async fn watch_loop(
        &mut self,
        mut rx: mpsc::Receiver<RawEvent>,
        overflow: &AtomicBool,
    ) -> Exit {
        let t = self.shared.timing.clone();
        let start = tokio::time::Instant::now();
        let mut reconcile_tick =
            tokio::time::interval_at(start + t.reconcile_interval, t.reconcile_interval);
        let mut health_tick =
            tokio::time::interval_at(start + t.health_interval, t.health_interval);
        loop {
            if overflow.swap(false, Ordering::SeqCst) {
                tracing::debug!(target: "folderflow::monitor", folder_id = %self.folder.id, "event overflow; reconciling");
                self.request_reconcile();
            }
            let next = [
                self.debouncer.next_deadline(),
                self.stability.next_deadline(),
                self.reconcile_at,
            ]
            .into_iter()
            .flatten()
            .min();
            let sleep_until = tokio::time::Instant::from_std(
                next.unwrap_or_else(|| Instant::now() + Duration::from_secs(3600)),
            );

            tokio::select! {
                cmd = self.cmd_rx.recv() => match cmd {
                    None | Some(Command::Stop) => return Exit::Stop,
                    Some(Command::Pause) => return Exit::Pause,
                    Some(Command::Rescan) => self.reconcile_at = Some(Instant::now()),
                    Some(Command::Resume) => {}
                },
                raw = rx.recv() => match raw {
                    None => return Exit::WatcherError("the file watcher stopped unexpectedly".into()),
                    Some(RawEvent::Rescan) => self.request_reconcile(),
                    Some(RawEvent::Error(m)) => return Exit::WatcherError(m),
                    Some(RawEvent::Paths(paths)) => {
                        let mut root_touched = false;
                        for p in paths {
                            root_touched |= self.on_raw_path(&p);
                        }
                        if root_touched && probe_access(&self.root) != FolderAccess::Granted {
                            return Exit::Recheck;
                        }
                        self.publish();
                    }
                },
                _ = tokio::time::sleep_until(sleep_until), if next.is_some() => {
                    if let Some(exit) = self.process_due().await {
                        return exit;
                    }
                    self.publish();
                },
                _ = reconcile_tick.tick() => self.request_reconcile(),
                _ = health_tick.tick() => {
                    if probe_access(&self.root) != FolderAccess::Granted {
                        return Exit::Recheck;
                    }
                },
            }
        }
    }

    /// Returns true if the event concerns the monitored root itself.
    fn on_raw_path(&mut self, raw: &Path) -> bool {
        let p = nfc(raw);
        if p == self.root {
            return true;
        }
        let Some(target) = self.event_target(&p) else {
            return false;
        };
        self.runtime.last_event_at = Some(now_ms());
        if self.debouncer.push(target, Instant::now()) == Push::Overflow {
            self.request_reconcile();
        }
        false
    }

    /// Maps a raw event path to the item FolderFlow tracks: the path itself,
    /// or the enclosing package. `None` if the path is outside scope (deeper
    /// than allowed, or inside an ignored/excluded folder).
    fn event_target(&self, p: &Path) -> Option<PathBuf> {
        let rel = p.strip_prefix(&self.root).ok()?;
        let comps: Vec<_> = rel.components().map(|c| c.as_os_str().to_owned()).collect();
        let last = comps.len().checked_sub(1)?;
        let mut cur = self.root.clone();
        for (i, c) in comps.iter().enumerate() {
            cur.push(c);
            if i == last {
                return Some(cur);
            }
            let name = c.to_string_lossy();
            if self
                .spec
                .rules
                .check_dir(&cur, &name, name.starts_with('.'))
                .is_some()
            {
                return None;
            }
            let is_package = crate::engine::classifier::extension_of(&name)
                .is_some_and(|e| PACKAGE_EXTENSIONS.contains(&e.as_str()));
            if is_package {
                return Some(cur);
            }
            if !self.spec.recursive {
                return None;
            }
        }
        None
    }

    async fn process_due(&mut self) -> Option<Exit> {
        let now = Instant::now();
        for p in self.debouncer.drain_ready(now) {
            self.handle_ready_path(p).await;
        }
        for p in self.stability.due(now) {
            let obs = observe(&p);
            if let Some(ev) = self.stability.observe(&p, obs, now) {
                self.handle_stability_event(ev).await;
            }
        }
        if self.reconcile_at.is_some_and(|at| at <= now) {
            self.reconcile_at = None;
            if let Err(e) = self.reconcile_now().await {
                self.on_reconcile_error(e).await;
                return Some(Exit::Recheck);
            }
        }
        None
    }

    async fn handle_ready_path(&mut self, p: PathBuf) {
        let rules = &self.spec.rules;
        match RealFs.stat(&p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(ev) = self
                    .stability
                    .observe(&p, Observation::Missing, Instant::now())
                {
                    self.handle_stability_event(ev).await;
                } else {
                    self.forget(&p).await;
                }
            }
            Err(e) => {
                tracing::debug!(target: "folderflow::monitor", error = %e.kind(), "stat failed")
            }
            Ok(f) => match f.kind {
                EntryKind::Dir => {
                    if self.spec.recursive
                        && rules.check_dir(&f.path, &f.name, f.is_hidden).is_none()
                    {
                        self.request_reconcile(); // a new subtree: scan it
                    }
                }
                EntryKind::Symlink | EntryKind::Special => {}
                EntryKind::File | EntryKind::Package => {
                    if rules
                        .check_file(&f.path, &f.name, f.extension.as_deref(), f.is_hidden)
                        .is_none()
                        && !self.stability.track(f.path, Instant::now())
                    {
                        self.request_reconcile();
                    }
                }
            },
        }
    }

    async fn handle_stability_event(&mut self, ev: StabilityEvent) {
        match ev {
            StabilityEvent::Removed(p) => self.forget(&p).await,
            StabilityEvent::Stable(facts) => {
                let file = classify(&self.shared.classifier(), facts);
                match repo_files::upsert_batch(
                    &self.shared.db,
                    &self.folder.id,
                    std::slice::from_ref(&file),
                    now_ms(),
                )
                .await
                {
                    Ok(_) => self.shared.observer.on_stable(&self.folder.id, &file),
                    Err(e) => {
                        tracing::error!(target: "folderflow::monitor", error = %e, "failed to record stable file")
                    }
                }
            }
        }
    }

    async fn forget(&self, p: &Path) {
        match repo_files::delete_path(&self.shared.db, &p.to_string_lossy()).await {
            Ok(true) => self.shared.observer.on_removed(&self.folder.id, p),
            Ok(false) => {}
            Err(e) => {
                tracing::error!(target: "folderflow::monitor", error = %e, "failed to remove file record")
            }
        }
    }

    fn request_reconcile(&mut self) {
        if self.reconcile_at.is_none() {
            self.reconcile_at = Some(Instant::now() + self.shared.timing.debounce);
        }
    }

    async fn reconcile_now(&mut self) -> Result<(), AppError> {
        let mode = if self.scanned_before {
            ReconcileMode::Diff
        } else {
            ReconcileMode::Baseline
        };
        let mut candidates = Vec::new();
        reconcile(
            &self.shared.db,
            &self.spec,
            self.shared.classifier(),
            mode,
            &mut |p| candidates.push(p),
        )
        .await?;
        self.scanned_before = true;
        let now = Instant::now();
        let mut dropped = 0usize;
        for p in candidates {
            if !self.stability.track(p, now) {
                dropped += 1;
            }
        }
        if dropped > 0 {
            tracing::warn!(target: "folderflow::monitor", folder_id = %self.folder.id, dropped, "stability tracker full; will retry on next reconcile");
            self.request_reconcile();
        }
        Ok(())
    }

    /// Maps a reconcile failure to a folder state. Returns true if the state
    /// already implies waiting (unavailable / denied) — no extra backoff needed.
    async fn on_reconcile_error(&mut self, e: AppError) -> bool {
        match e {
            AppError::Path(PathError::NotFound) => {
                self.set_state(RuntimeState::Unavailable, Some(MSG_UNAVAILABLE.into()))
                    .await;
                true
            }
            AppError::Path(PathError::PermissionDenied) => {
                self.set_state(RuntimeState::PermissionDenied, Some(MSG_DENIED.into()))
                    .await;
                true
            }
            other => {
                self.watcher_error("reconcile", &other.to_string()).await;
                false
            }
        }
    }

    async fn watcher_error(&mut self, code: &str, msg: &str) {
        tracing::warn!(target: "folderflow::monitor", folder_id = %self.folder.id, code, error = msg, "monitoring error");
        if let Err(e) =
            repo_folders::log_watcher_error(&self.shared.db, &self.folder.id, code, msg).await
        {
            tracing::error!(target: "folderflow::monitor", error = %e, "failed to record watcher error");
        }
        self.set_state(
            RuntimeState::Error,
            Some(format!(
                "Monitoring hit a problem and will retry automatically ({msg})."
            )),
        )
        .await;
    }

    /// Waits for `d` or a command. `Err(())` means stop.
    async fn wait(&mut self, d: Duration) -> Result<(), ()> {
        let flow = tokio::select! {
            _ = tokio::time::sleep(d) => Flow::Timeout,
            cmd = self.cmd_rx.recv() => Flow::Command(cmd.unwrap_or(Command::Stop)),
        };
        match flow {
            Flow::Command(Command::Stop) => Err(()),
            Flow::Command(Command::Pause) => {
                self.paused = true;
                Ok(())
            }
            Flow::Timeout | Flow::Command(Command::Resume | Command::Rescan) => Ok(()),
        }
    }

    async fn set_state(&mut self, state: RuntimeState, error: Option<String>) {
        let changed = self.runtime.state != state || self.runtime.last_error != error;
        self.runtime.state = state;
        self.runtime.last_error = error;
        self.publish();
        if !changed {
            return;
        }
        self.shared.observer.on_status(&self.runtime);
        if let Some(h) = state.persisted_health() {
            let err = self.runtime.last_error.as_deref();
            if let Err(e) = repo_folders::set_health(&self.shared.db, &self.folder.id, h, err).await
            {
                tracing::error!(target: "folderflow::monitor", error = %e, "failed to persist folder health");
            }
        }
        tracing::info!(target: "folderflow::monitor", folder_id = %self.folder.id, state = ?state, "folder state");
    }

    fn publish(&mut self) {
        self.runtime.pending_events = u32::try_from(self.debouncer.len()).unwrap_or(u32::MAX);
        self.runtime.tracked_files = u32::try_from(self.stability.len()).unwrap_or(u32::MAX);
        self.status_tx.send_replace(self.runtime.clone());
    }

    async fn finish(mut self) {
        self.set_state(RuntimeState::Stopped, None).await;
    }
}

/// Re-stats a tracked path for the stability check.
fn observe(p: &Path) -> Observation {
    match RealFs.stat(p) {
        Ok(mut f) if f.kind == EntryKind::File || f.kind == EntryKind::Package => {
            if f.kind == EntryKind::Package {
                f.size = package_size(p);
            }
            Observation::Present(f)
        }
        _ => Observation::Missing,
    }
}

fn classify(classifier: &Classifier, facts: FileFacts) -> ScannedFile {
    let is_package = facts.kind == EntryKind::Package;
    let header = if is_package {
        None
    } else if facts.size == 0 {
        Some(Vec::new())
    } else {
        RealFs.read_header(&facts.path, HEADER_BYTES).ok()
    };
    let c = classifier.classify(&facts.name, header.as_deref(), is_package);
    ScannedFile {
        facts,
        category: c.category,
        mime: c.mime,
        confidence: c.confidence,
    }
}
