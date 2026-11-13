//! Structured logging (tracing) to rotating local files.
//!
//! Logs go to `~/Library/Logs/com.folderflow.desktop/folderflow.YYYY-MM-DD.log`,
//! rotate daily and keep `retention_days` files. Logs never contain file
//! contents. Paths should be passed through [`redact_path`] before logging.

use std::hash::{Hash, Hasher};
use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::error::{AppError, AppResult};

pub const LOG_FILE_PREFIX: &str = "folderflow";
pub const LOG_FILE_SUFFIX: &str = "log";

/// Builds the daily-rotating file appender with retention.
pub fn build_appender(log_dir: &Path, retention_days: u32) -> AppResult<RollingFileAppender> {
    std::fs::create_dir_all(log_dir)?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(retention_days.max(1) as usize)
        .build(log_dir)
        .map_err(|e| AppError::Internal(format!("log appender: {e}")))
}

/// Installs the global subscriber: file (always) + stderr (debug builds).
/// Returns the guard that must be kept alive for the app's lifetime so
/// buffered log lines are flushed on exit.
pub fn init(log_dir: &Path, retention_days: u32, verbose: bool) -> AppResult<WorkerGuard> {
    let appender = build_appender(log_dir, retention_days)?;
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let filter = EnvFilter::try_from_env("FOLDERFLOW_LOG")
        .unwrap_or_else(|_| EnvFilter::new(default_directives(verbose)));

    let file_layer = fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true);
    let stderr_layer = cfg!(debug_assertions).then(|| fmt::layer().with_writer(std::io::stderr));

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .map_err(|e| AppError::Internal(format!("logging already initialised: {e}")))?;

    Ok(guard)
}

/// Default filter: FolderFlow's own events at info (debug when verbose),
/// everything else (tauri, sqlx, webview…) at warn.
pub fn default_directives(verbose: bool) -> String {
    let level = if verbose { "debug" } else { "info" };
    format!("folderflow={level},folderflow_lib={level},warn")
}

/// Returns a log-safe representation of a path.
///
/// * `redact = true`  → `[path:1a2b3c4d].pdf` — a stable short id plus extension,
///   enough to correlate log lines without revealing names or folders.
/// * `redact = false` → the path with the home directory replaced by `~`.
pub fn redact_path(path: &Path, redact: bool) -> String {
    if redact {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        let id = hasher.finish() as u32;
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => format!("[path:{id:08x}].{ext}"),
            None => format!("[path:{id:08x}]"),
        }
    } else {
        let s = path.display().to_string();
        match std::env::var("HOME") {
            Ok(home) if !home.is_empty() && s.starts_with(&home) => {
                format!("~{}", &s[home.len()..])
            }
            _ => s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    #[test]
    fn r_log_01_appender_creates_log_file_in_log_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_dir = dir.path().join("Logs");
        let mut appender = build_appender(&log_dir, 14).expect("appender");
        appender.write_all(b"hello\n").expect("write");
        appender.flush().expect("flush");

        let files: Vec<_> = std::fs::read_dir(&log_dir)
            .expect("read_dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files.len(), 1, "{files:?}");
        assert!(
            files[0].starts_with("folderflow.") && files[0].ends_with(".log"),
            "{files:?}"
        );
    }

    /// BUG-007: events logged with `target: "folderflow::…"` were filtered out
    /// because the default directive only enabled `folderflow_lib`.
    #[test]
    fn bug_007_default_filter_keeps_folderflow_targets_and_drops_noise() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("lock").extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        for verbose in [false, true] {
            let buf = Buf(Arc::new(Mutex::new(Vec::new())));
            let sink = buf.clone();
            let subscriber = tracing_subscriber::registry()
                .with(EnvFilter::new(default_directives(verbose)))
                .with(
                    fmt::layer()
                        .with_ansi(false)
                        .with_writer(move || sink.clone()),
                );
            tracing::subscriber::with_default(subscriber, || {
                tracing::info!(target: "folderflow::app", "app-info");
                tracing::info!(target: "folderflow_lib::settings", "lib-info");
                tracing::debug!(target: "folderflow::app", "app-debug");
                tracing::info!(target: "sqlx::query", "noise-info");
                tracing::warn!(target: "tauri", "tauri-warn");
            });
            let out = String::from_utf8(buf.0.lock().expect("lock").clone()).expect("utf8");
            assert!(out.contains("app-info"), "verbose={verbose}: {out}");
            assert!(out.contains("lib-info"), "verbose={verbose}: {out}");
            assert_eq!(
                out.contains("app-debug"),
                verbose,
                "verbose={verbose}: {out}"
            );
            assert!(!out.contains("noise-info"), "verbose={verbose}: {out}");
            assert!(out.contains("tauri-warn"), "verbose={verbose}: {out}");
        }
    }

    #[test]
    fn r_log_02_redaction_hides_names_and_folders_but_is_stable() {
        let p = PathBuf::from("/Users/alice/Documents/Taxes/invoice-acme.pdf");
        let r = redact_path(&p, true);
        assert!(
            !r.contains("alice") && !r.contains("Taxes") && !r.contains("invoice"),
            "{r}"
        );
        assert!(r.ends_with(".pdf"), "{r}");
        assert_eq!(r, redact_path(&p, true), "must be stable for correlation");
        assert_ne!(r, redact_path(&PathBuf::from("/tmp/other.pdf"), true));
        assert!(!redact_path(&PathBuf::from("/tmp/noext"), true).contains('.'));
    }

    #[test]
    fn r_log_02b_unredacted_replaces_home_with_tilde() {
        let home = std::env::var("HOME").expect("HOME");
        let p = PathBuf::from(&home).join("Downloads/a.zip");
        assert_eq!(redact_path(&p, false), "~/Downloads/a.zip");
    }
}
