//! SQLite persistence: connection pool, migrations, repositories.

mod pool;
pub mod repo_categories;
pub mod repo_files;
pub mod repo_folders;
pub mod repo_operations;
pub mod repo_rules;
pub mod repo_schedules;
pub mod repo_settings;

pub use pool::{open, open_in_memory, Db};
