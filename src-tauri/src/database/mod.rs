//! SQLite persistence: connection pool, migrations, repositories.

mod pool;
pub mod repo_settings;

pub use pool::{open, open_in_memory, Db};
