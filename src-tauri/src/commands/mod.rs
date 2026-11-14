//! Tauri command handlers. Each handler only: validate input → call a service →
//! map the error. No business logic here (docs/ARCHITECTURE.md §4).

pub mod activity;
pub mod app;
pub mod categories;
pub mod config_io;
pub mod dashboard;
pub mod duplicates;
pub mod exclusions;
pub mod folders;
pub mod license;
pub mod monitoring;
pub mod notifications;
pub mod onboarding;
pub mod preview;
pub mod rules;
pub mod schedules;
pub mod settings;
