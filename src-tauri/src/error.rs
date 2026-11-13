//! Error model.
//!
//! `AppError` is the internal error type used across all services. It is never
//! sent to the UI directly; at the command boundary it is converted into a
//! `UiError`, which carries a stable `code`, a user-facing `message` and an
//! optional `hint`. Internal details (SQL, paths, OS error text) stay in the
//! log, not in the UI.

use serde::Serialize;
use specta::Type;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("database migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("path error: {0}")]
    Path(#[from] crate::engine::paths::PathError),

    #[error("invalid {field}: {message}")]
    Validation { field: String, message: String },

    #[error("not found: {0}")]
    NotFound(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            message: message.into(),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// Stable error codes. The UI switches on these; never rename a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Database,
    Io,
    PermissionDenied,
    NotFound,
    Validation,
    Internal,
}

/// Error shape returned by every Tauri command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UiError {
    pub code: ErrorCode,
    pub message: String,
    pub hint: Option<String>,
    /// Set for validation errors: which field was invalid (e.g. `monitoring.stabilityIntervalSecs`).
    pub field: Option<String>,
}

impl UiError {
    fn new(code: ErrorCode, message: impl Into<String>, hint: Option<&str>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: hint.map(str::to_owned),
            field: None,
        }
    }
}

impl From<AppError> for UiError {
    /// Converts at the command boundary. This is the single place command
    /// errors are logged, so every error is logged exactly once.
    fn from(err: AppError) -> Self {
        tracing::error!(target: "folderflow::command", error = %err, "command failed");
        match err {
            AppError::Validation { field, message } => Self {
                code: ErrorCode::Validation,
                message,
                hint: None,
                field: Some(field),
            },
            AppError::NotFound(what) => Self::new(
                ErrorCode::NotFound,
                format!("{what} could not be found."),
                Some("It may have been removed. Refresh and try again."),
            ),
            AppError::Path(crate::engine::paths::PathError::PermissionDenied) => Self::new(
                ErrorCode::PermissionDenied,
                "FolderFlow doesn't have permission to access this folder.",
                Some("Grant access in System Settings → Privacy & Security → Files and Folders, then try again."),
            ),
            AppError::Path(crate::engine::paths::PathError::NotFound) => Self::new(
                ErrorCode::NotFound,
                "That folder could not be found.",
                Some("It may have been moved, renamed, or be on a disconnected drive."),
            ),
            AppError::Path(e @ crate::engine::paths::PathError::Io(_)) => Self::new(
                ErrorCode::Io,
                format!("FolderFlow couldn't check this location ({e})."),
                Some("Check that the folder or drive is still available."),
            ),
            AppError::Path(e) => Self {
                code: ErrorCode::Validation,
                message: capitalize(&e.to_string()),
                hint: None,
                field: Some("path".into()),
            },
            AppError::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Self::new(
                ErrorCode::PermissionDenied,
                "FolderFlow doesn't have permission to access this location.",
                Some("Grant access in System Settings → Privacy & Security → Files and Folders."),
            ),
            AppError::Io(_) => Self::new(
                ErrorCode::Io,
                "A file system error occurred.",
                Some("Check that the folder or drive is still available. Details are in the log."),
            ),
            AppError::Database(_) | AppError::Migration(_) => Self::new(
                ErrorCode::Database,
                "FolderFlow couldn't read or write its local database.",
                Some("Restart FolderFlow. If this keeps happening, see the log for details."),
            ),
            AppError::Serde(_) | AppError::Internal(_) => Self::new(
                ErrorCode::Internal,
                "Something went wrong inside FolderFlow.",
                Some("Details were written to the log."),
            ),
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect::<String>() + ".",
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r_err_01_validation_maps_to_validation_code_with_field() {
        let ui: UiError =
            AppError::validation("monitoring.stabilityIntervalSecs", "must be 1–300").into();
        assert_eq!(ui.code, ErrorCode::Validation);
        assert_eq!(
            ui.field.as_deref(),
            Some("monitoring.stabilityIntervalSecs")
        );
        assert_eq!(ui.message, "must be 1–300");
    }

    #[test]
    fn r_err_02_internal_details_do_not_leak_to_ui() {
        let secret = "/Users/alice/Documents/tax-2026.pdf";
        let cases = [
            AppError::Io(std::io::Error::other(format!("failed at {secret}"))),
            AppError::Internal(format!("panic near {secret}")),
            AppError::Database(sqlx::Error::Protocol(format!(
                "SELECT * FROM x WHERE path='{secret}'"
            ))),
        ];
        for err in cases {
            let ui: UiError = err.into();
            let json = serde_json::to_string(&ui).expect("serialize");
            assert!(!json.contains(secret), "leaked internal detail: {json}");
            assert!(!json.contains("SELECT"), "leaked SQL: {json}");
        }
    }

    #[test]
    fn r_err_03_codes_are_stable_snake_case_and_permission_denied_has_hint() {
        let ui: UiError =
            AppError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).into();
        assert_eq!(ui.code, ErrorCode::PermissionDenied);
        assert!(ui
            .hint
            .as_deref()
            .is_some_and(|h| h.contains("System Settings")));
        let json = serde_json::to_value(&ui).expect("serialize");
        assert_eq!(json["code"], "permission_denied");

        let nf: UiError = AppError::NotFound("Rule".into()).into();
        assert_eq!(
            serde_json::to_value(&nf).expect("serialize")["code"],
            "not_found"
        );
    }
}
