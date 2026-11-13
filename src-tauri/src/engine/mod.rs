//! Filesystem engine. Phase 2: read-only discovery (paths, scanning,
//! classification, watching). Mutation arrives in Phase 4 (`safe_fs`).

pub mod classifier;
pub mod debounce;
pub mod fs_view;
pub mod ignore;
pub mod monitor;
pub mod paths;
pub mod reconcile;
pub mod scanner;
pub mod stability;

#[cfg(test)]
mod monitor_tests;
