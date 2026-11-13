//! Application settings: typed model, defaults, validation, persistence.
//!
//! Settings are stored as one JSON row per section in the `settings` table.
//! A missing or corrupted section falls back to its defaults (and is logged),
//! so a bad settings row can never stop the app from starting.

mod model;

pub use model::*;

use crate::database::{repo_settings, Db};
use crate::error::AppResult;

#[derive(Clone)]
pub struct SettingsService {
    db: Db,
}

impl SettingsService {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn get(&self) -> AppResult<Settings> {
        let rows = repo_settings::load_all(&self.db).await?;
        Ok(Settings::from_sections(rows))
    }

    /// Validates and persists the full settings object atomically.
    pub async fn update(&self, settings: Settings) -> AppResult<Settings> {
        settings.validate()?;
        repo_settings::save_all(&self.db, &settings.to_sections()?).await?;
        tracing::info!(target: "folderflow::settings", "settings updated");
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::error::AppError;

    #[tokio::test]
    async fn r_set_01_fresh_db_returns_defaults() {
        let svc = SettingsService::new(database::open_in_memory().await.expect("db"));
        assert_eq!(svc.get().await.expect("get"), Settings::default());
    }

    #[tokio::test]
    async fn r_set_02_invalid_values_are_rejected_and_not_persisted() {
        let svc = SettingsService::new(database::open_in_memory().await.expect("db"));
        let mut s = Settings::default();
        s.monitoring.stability_interval_secs = 0;
        let err = svc.update(s).await.expect_err("must reject");
        match err {
            AppError::Validation { field, .. } => {
                assert_eq!(field, "monitoring.stabilityIntervalSecs")
            }
            other => panic!("unexpected error {other:?}"),
        }
        assert_eq!(svc.get().await.expect("get"), Settings::default());
    }

    #[tokio::test]
    async fn r_set_03_updates_persist_across_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ff.db");
        let mut s = Settings::default();
        s.appearance.theme = ThemeMode::Dark;
        s.appearance.density = Density::Compact;
        s.monitoring.max_concurrent_ops = 4;
        {
            let db = database::open(&path).await.expect("open");
            SettingsService::new(db.clone())
                .update(s.clone())
                .await
                .expect("update");
            db.close().await;
        }
        let svc = SettingsService::new(database::open(&path).await.expect("reopen"));
        assert_eq!(svc.get().await.expect("get"), s);
    }

    #[tokio::test]
    async fn r_set_04_corrupted_section_falls_back_to_defaults_others_kept() {
        let db = database::open_in_memory().await.expect("db");
        let svc = SettingsService::new(db.clone());
        let mut s = Settings::default();
        s.appearance.theme = ThemeMode::Light;
        svc.update(s).await.expect("update");
        sqlx::query("UPDATE settings SET value = '{not json' WHERE key = 'monitoring'")
            .execute(&db)
            .await
            .expect("corrupt");
        let got = svc.get().await.expect("get must not fail on corrupt row");
        assert_eq!(got.monitoring, MonitoringSettings::default());
        assert_eq!(got.appearance.theme, ThemeMode::Light);
    }

    #[tokio::test]
    async fn r_set_05_unknown_and_missing_fields_are_tolerated() {
        let db = database::open_in_memory().await.expect("db");
        sqlx::query("INSERT INTO settings (key, value, updated_at) VALUES ('appearance', '{\"theme\":\"dark\",\"futureField\":1}', 0)")
            .execute(&db)
            .await
            .expect("insert");
        let got = SettingsService::new(db).get().await.expect("get");
        assert_eq!(got.appearance.theme, ThemeMode::Dark);
        assert_eq!(got.appearance.density, Density::Comfortable);
    }
}
