//! Filesystem engine. Phase 2: read-only discovery (paths, scanning,
//! classification, watching). Mutation arrives in Phase 4 (`safe_fs`).

pub mod activity;
pub mod classifier;
pub mod conflicts;
pub mod debounce;
pub mod fs_view;
pub mod ignore;
pub mod monitor;
pub mod operations;
pub mod organizer;
pub mod paths;
pub mod pipeline;
pub mod planner;
pub mod reconcile;
pub mod recovery;
pub mod safe_fs;
pub mod scanner;
pub mod scheduler;
pub mod stability;
pub mod undo;

#[cfg(test)]
mod monitor_tests;
