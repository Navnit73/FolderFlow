use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

use crate::error::AppResult;

pub type Db = SqlitePool;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

fn base_options(opts: SqliteConnectOptions) -> SqliteConnectOptions {
    opts.foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
}

/// Opens (creating if needed) the database file and applies pending migrations.
///
/// Per `NFR-032` (`P4-T09`), if the existing database file is corrupted (`SQLITE_CORRUPT` /
/// `SQLITE_NOTADB` / malformed), it is backed up to `<filename>.corrupt-<epoch_ms>.bak`
/// using `SafeFs::rename_excl` and a fresh database is initialized in its place.
pub async fn open(path: &Path) -> AppResult<Db> {
    use crate::engine::safe_fs::Fs;
    if let Some(parent) = path.parent() {
        crate::engine::safe_fs::SafeFs::production().mkdir_p(parent)?;
    }
    match try_open_and_migrate(path).await {
        Ok(pool) => {
            tracing::info!(target: "folderflow::db", "database ready");
            Ok(pool)
        }
        Err(err) if is_corruption_error(&err) && path.exists() => {
            backup_corrupt_database(path)?;
            let pool = try_open_and_migrate(path).await?;
            tracing::warn!(
                target: "folderflow::db",
                error = %err,
                "corrupted database was backed up and recreated"
            );
            Ok(pool)
        }
        Err(err) => Err(err),
    }
}

async fn try_open_and_migrate(path: &Path) -> AppResult<Db> {
    let opts = base_options(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    );
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

fn is_corruption_error(err: &crate::error::AppError) -> bool {
    let msg = err.to_string().to_ascii_lowercase();
    msg.contains("not a database")
        || msg.contains("malformed")
        || msg.contains("corrupt")
        || msg.contains("code: 11")
        || msg.contains("code: 26")
}

fn backup_corrupt_database(path: &Path) -> AppResult<()> {
    use crate::engine::safe_fs::{Fs, SafeFs};

    let fs = SafeFs::production();
    let stamp = crate::utils::time::now_ms();
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "folderflow.db".to_owned());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));

    let backup_path = parent.join(format!("{file_name}.corrupt-{stamp}.bak"));
    fs.rename_excl(path, &backup_path)?;

    for suffix in ["-wal", "-shm"] {
        let sidecar = parent.join(format!("{file_name}{suffix}"));
        if fs.exists_no_follow(&sidecar) {
            let sidecar_bak = parent.join(format!("{file_name}{suffix}.corrupt-{stamp}.bak"));
            let _ = fs.rename_excl(&sidecar, &sidecar_bak);
        }
    }
    Ok(())
}

/// In-memory database for tests. Single connection so all queries share it.
pub async fn open_in_memory() -> AppResult<Db> {
    let opts = base_options(SqliteConnectOptions::from_str("sqlite::memory:")?)
        .journal_mode(SqliteJournalMode::Memory);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_TABLES: &[&str] = &[
        "categories",
        "destination_roots",
        "duplicate_groups",
        "duplicate_members",
        "exclusions",
        "file_records",
        "license_state",
        "monitored_folders",
        "operation_batches",
        "operations",
        "review_queue",
        "rules",
        "schedules",
        "settings",
        "watcher_errors",
    ];

    async fn tables(db: &Db) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' \
             AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx%' ORDER BY name",
        )
        .fetch_all(db)
        .await
        .expect("list tables")
    }

    #[tokio::test]
    async fn r_db_01_migrations_create_full_schema_on_empty_db() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = open(&dir.path().join("nested/ff.db")).await.expect("open");
        assert_eq!(tables(&db).await, EXPECTED_TABLES);
    }

    #[tokio::test]
    async fn r_db_02_foreign_keys_are_enforced() {
        let db = open_in_memory().await.expect("open");
        let res = sqlx::query(
            "INSERT INTO file_records (id, folder_id, path, filename, size, dev, ino, modified_at_ns, last_scanned_at) \
             VALUES ('f1', 'no-such-folder', '/x', 'x', 0, 1, 1, 0, 0)",
        )
        .execute(&db)
        .await;
        let err = res.expect_err("FK violation must fail");
        assert!(err.to_string().contains("FOREIGN KEY"), "{err}");
    }

    #[tokio::test]
    async fn r_db_03_file_db_uses_wal_journal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = open(&dir.path().join("ff.db")).await.expect("open");
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&db)
            .await
            .expect("pragma");
        assert_eq!(mode.to_lowercase(), "wal");
        let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&db)
            .await
            .expect("pragma");
        assert_eq!(fk, 1);
    }

    #[tokio::test]
    async fn r_db_04_reopen_is_idempotent_and_keeps_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ff.db");
        {
            let db = open(&path).await.expect("open 1");
            sqlx::query("INSERT INTO settings (key, value, updated_at) VALUES ('k', '1', 0)")
                .execute(&db)
                .await
                .expect("insert");
            db.close().await;
        }
        let db = open(&path).await.expect("open 2");
        let v: String = sqlx::query_scalar("SELECT value FROM settings WHERE key='k'")
            .fetch_one(&db)
            .await
            .expect("select");
        assert_eq!(v, "1");
        assert_eq!(tables(&db).await, EXPECTED_TABLES);
    }

    #[tokio::test]
    async fn r_db_05_check_constraints_reject_invalid_status() {
        let db = open_in_memory().await.expect("open");
        let res = sqlx::query(
            "INSERT INTO operations (id, operation_type, source_path, destination_path, original_name, new_name, status, started_at) \
             VALUES ('o1', 'move', '/a', '/b', 'a', 'b', 'DONE', 0)",
        )
        .execute(&db)
        .await;
        assert!(res.is_err(), "unknown op status must be rejected");
    }
}
