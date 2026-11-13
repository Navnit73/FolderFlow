//! Stability checks (FR-015, SAFETY_MODEL S9): a file is processed only after
//! its size, mtime and identity are unchanged across N consecutive checks.
//! Pure state machine; the caller supplies time and observations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::engine::fs_view::FileFacts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    Present(FileFacts),
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StabilityEvent {
    Stable(FileFacts),
    Removed(PathBuf),
}

/// What must stay identical between checks.
type Signature = (u64, i64, u64, u64); // size, mtime_ns, dev, ino

#[derive(Debug)]
struct Entry {
    last: Option<Signature>,
    identical: u32,
    next_check: Instant,
}

#[derive(Debug)]
pub struct StabilityTracker {
    interval: Duration,
    required: u32,
    max_tracked: usize,
    entries: HashMap<PathBuf, Entry>,
}

impl StabilityTracker {
    /// `required` = consecutive identical observations needed (clamped to ≥ 2).
    pub fn new(interval: Duration, required: u32, max_tracked: usize) -> Self {
        Self {
            interval,
            required: required.max(2),
            max_tracked,
            entries: HashMap::new(),
        }
    }

    /// Starts (or restarts) tracking. A new event for a tracked path resets its
    /// count. Returns false if the tracker is full (caller falls back to reconcile).
    pub fn track(&mut self, path: PathBuf, now: Instant) -> bool {
        let next_check = now + self.interval;
        if let Some(e) = self.entries.get_mut(&path) {
            *e = Entry {
                last: None,
                identical: 0,
                next_check,
            };
            return true;
        }
        if self.entries.len() >= self.max_tracked {
            return false;
        }
        self.entries.insert(
            path,
            Entry {
                last: None,
                identical: 0,
                next_check,
            },
        );
        true
    }

    pub fn due(&self, now: Instant) -> Vec<PathBuf> {
        let mut due: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|(_, e)| e.next_check <= now)
            .map(|(p, _)| p.clone())
            .collect();
        due.sort();
        due
    }

    pub fn observe(
        &mut self,
        path: &Path,
        obs: Observation,
        now: Instant,
    ) -> Option<StabilityEvent> {
        let entry = self.entries.get_mut(path)?;
        let facts = match obs {
            Observation::Missing => {
                self.entries.remove(path);
                return Some(StabilityEvent::Removed(path.to_path_buf()));
            }
            Observation::Present(f) => f,
        };
        let sig = (facts.size, facts.modified_ns, facts.dev, facts.ino);
        if entry.last == Some(sig) {
            entry.identical += 1;
        } else {
            entry.last = Some(sig);
            entry.identical = 1;
        }
        entry.next_check = now + self.interval;
        if entry.identical >= self.required {
            self.entries.remove(path);
            return Some(StabilityEvent::Stable(facts));
        }
        None
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.entries.values().map(|e| e.next_check).min()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fs_view::EntryKind;

    const I: Duration = Duration::from_secs(2);

    fn facts(size: u64, mtime: i64) -> FileFacts {
        FileFacts {
            path: PathBuf::from("/r/movie.mp4"),
            name: "movie.mp4".into(),
            extension: Some("mp4".into()),
            kind: EntryKind::File,
            size,
            dev: 1,
            ino: 42,
            created_ns: None,
            modified_ns: mtime,
            is_hidden: false,
        }
    }

    /// Runs one check round at `now` with the given observation for every due path.
    fn tick(t: &mut StabilityTracker, now: Instant, obs: Observation) -> Vec<StabilityEvent> {
        t.due(now)
            .iter()
            .filter_map(|p| t.observe(p, obs.clone(), now))
            .collect()
    }

    #[test]
    fn r_stab_01_growing_file_is_never_reported_while_it_grows() {
        let t0 = Instant::now();
        let mut t = StabilityTracker::new(I, 3, 100);
        assert!(t.track(PathBuf::from("/r/movie.mp4"), t0));
        assert!(t.due(t0).is_empty(), "first check waits one interval");
        for i in 1..=10u32 {
            let ev = tick(
                &mut t,
                t0 + I * i,
                Observation::Present(facts(u64::from(i) * 1000, i64::from(i))),
            );
            assert!(ev.is_empty(), "reported while still growing at round {i}");
        }
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn r_stab_02_stable_after_required_identical_checks_exactly_once() {
        let t0 = Instant::now();
        let mut t = StabilityTracker::new(I, 3, 100);
        t.track(PathBuf::from("/r/movie.mp4"), t0);
        let f = facts(5000, 7);
        assert!(tick(&mut t, t0 + I, Observation::Present(f.clone())).is_empty());
        assert!(tick(&mut t, t0 + I * 2, Observation::Present(f.clone())).is_empty());
        assert_eq!(
            tick(&mut t, t0 + I * 3, Observation::Present(f.clone())),
            vec![StabilityEvent::Stable(f)]
        );
        assert!(t.is_empty());
        assert!(tick(&mut t, t0 + I * 4, Observation::Missing).is_empty());
    }

    #[test]
    fn r_stab_03_disappearing_file_is_removed_not_stable() {
        let t0 = Instant::now();
        let mut t = StabilityTracker::new(I, 2, 100);
        t.track(PathBuf::from("/r/movie.mp4"), t0);
        tick(&mut t, t0 + I, Observation::Present(facts(1, 1)));
        assert_eq!(
            tick(&mut t, t0 + I * 2, Observation::Missing),
            vec![StabilityEvent::Removed(PathBuf::from("/r/movie.mp4"))]
        );
        assert!(t.is_empty());
    }

    #[test]
    fn r_stab_04_new_event_restarts_the_count_and_identity_change_resets() {
        let t0 = Instant::now();
        let mut t = StabilityTracker::new(I, 2, 100);
        let p = PathBuf::from("/r/movie.mp4");
        t.track(p.clone(), t0);
        tick(&mut t, t0 + I, Observation::Present(facts(10, 1)));
        // Modified again: watcher re-tracks before the confirming check.
        t.track(p.clone(), t0 + I + Duration::from_millis(500));
        assert!(
            t.due(t0 + I * 2).is_empty(),
            "deadline pushed out by the new event"
        );
        assert!(
            tick(
                &mut t,
                t0 + I * 2 + Duration::from_millis(500),
                Observation::Present(facts(10, 1))
            )
            .is_empty(),
            "count restarted"
        );
        // Same size/mtime but a different inode (file replaced) is not "unchanged".
        let mut replaced = facts(10, 1);
        replaced.ino = 99;
        assert!(tick(
            &mut t,
            t0 + I * 3 + Duration::from_millis(500),
            Observation::Present(replaced.clone())
        )
        .is_empty());
        assert_eq!(
            tick(
                &mut t,
                t0 + I * 4 + Duration::from_millis(500),
                Observation::Present(replaced.clone())
            ),
            vec![StabilityEvent::Stable(replaced)]
        );
        assert!(t
            .observe(Path::new("/untracked"), Observation::Missing, t0)
            .is_none());
    }

    #[test]
    fn r_stab_capacity_is_bounded() {
        let t0 = Instant::now();
        let mut t = StabilityTracker::new(I, 2, 2);
        assert!(t.track(PathBuf::from("/a"), t0));
        assert!(t.track(PathBuf::from("/b"), t0));
        assert!(
            t.track(PathBuf::from("/a"), t0),
            "re-tracking an existing path is fine"
        );
        assert!(!t.track(PathBuf::from("/c"), t0));
        assert_eq!(t.next_deadline(), Some(t0 + I));
    }
}
