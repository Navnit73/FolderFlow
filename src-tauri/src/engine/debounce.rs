//! Event debouncing + deduplication (FR-013). Pure state machine; the caller
//! supplies time. Watcher events are never trusted to be complete or unique:
//! each debounced path is re-`stat`ed by the caller.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Debouncer {
    quiet: Duration,
    max_pending: usize,
    seq: u64,
    /// path → (first-seen sequence, last event time)
    pending: HashMap<PathBuf, (u64, Instant)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    Accepted,
    /// Too many distinct pending paths; caller must fall back to a reconcile scan.
    Overflow,
}

impl Debouncer {
    pub fn new(quiet: Duration, max_pending: usize) -> Self {
        Self {
            quiet,
            max_pending,
            seq: 0,
            pending: HashMap::new(),
        }
    }

    pub fn push(&mut self, path: PathBuf, now: Instant) -> Push {
        if let Some(entry) = self.pending.get_mut(&path) {
            entry.1 = entry.1.max(now);
            return Push::Accepted;
        }
        if self.pending.len() >= self.max_pending {
            return Push::Overflow;
        }
        self.seq += 1;
        self.pending.insert(path, (self.seq, now));
        Push::Accepted
    }

    /// Paths with no new event for at least `quiet`, in first-seen order.
    pub fn drain_ready(&mut self, now: Instant) -> Vec<PathBuf> {
        let mut ready: Vec<(u64, PathBuf)> = self
            .pending
            .iter()
            .filter(|(_, (_, last))| now.saturating_duration_since(*last) >= self.quiet)
            .map(|(p, (seq, _))| (*seq, p.clone()))
            .collect();
        ready.sort_by_key(|(seq, _)| *seq);
        for (_, p) in &ready {
            self.pending.remove(p);
        }
        ready.into_iter().map(|(_, p)| p).collect()
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending
            .values()
            .map(|(_, last)| *last + self.quiet)
            .min()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q: Duration = Duration::from_millis(500);

    #[test]
    fn r_watch_dedupe_bursts_collapse_to_one_path_after_quiet_period() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Q, 100);
        for i in 0..20 {
            assert_eq!(
                d.push(
                    PathBuf::from("/r/a.pdf"),
                    t0 + Duration::from_millis(i * 10)
                ),
                Push::Accepted
            );
        }
        d.push(PathBuf::from("/r/b.pdf"), t0);
        assert_eq!(d.len(), 2);
        assert_eq!(
            d.drain_ready(t0 + Duration::from_millis(400)),
            Vec::<PathBuf>::new()
        );
        assert_eq!(d.drain_ready(t0 + Q), vec![PathBuf::from("/r/b.pdf")]);
        assert_eq!(d.next_deadline(), Some(t0 + Duration::from_millis(190) + Q));
        assert_eq!(
            d.drain_ready(t0 + Duration::from_millis(190) + Q),
            vec![PathBuf::from("/r/a.pdf")]
        );
        assert!(d.is_empty());
        assert_eq!(d.next_deadline(), None);
    }

    #[test]
    fn r_watch_overflow_is_reported_not_buffered() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Q, 2);
        assert_eq!(d.push(PathBuf::from("/a"), t0), Push::Accepted);
        assert_eq!(d.push(PathBuf::from("/b"), t0), Push::Accepted);
        assert_eq!(
            d.push(PathBuf::from("/a"), t0),
            Push::Accepted,
            "existing path never overflows"
        );
        assert_eq!(d.push(PathBuf::from("/c"), t0), Push::Overflow);
        assert_eq!(d.len(), 2);
    }
}
