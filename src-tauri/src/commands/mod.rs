//! Tauri command handlers. Each handler only: validate input → call a service →
//! map the error. No business logic here (docs/ARCHITECTURE.md §4).

pub mod activity;
pub mod app;
pub mod folders;
pub mod monitoring;
pub mod onboarding;
pub mod preview;
pub mod rules;
pub mod schedules;
pub mod settings;
