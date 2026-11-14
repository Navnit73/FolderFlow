//! Versioned JSON config exporter (`P6-T05`, FR-190).

use std::collections::HashMap;

use crate::config_io::schema::{
    ConfigBundle, ExportedExclusion, ExportedFolder, ExportedRule, CONFIG_SCHEMA_VERSION,
};
use crate::database::{repo_categories, repo_exclusions, repo_folders, repo_rules, Db};
use crate::error::AppResult;
use crate::settings::SettingsService;

pub async fn export_config(db: &Db, app_version: &str, now_ms: i64) -> AppResult<String> {
    let settings = SettingsService::new(db.clone()).get().await?;
    let folders_raw = repo_folders::list(db).await?;
    let id_to_path: HashMap<String, String> = folders_raw
        .iter()
        .map(|f| (f.id.clone(), f.path.clone()))
        .collect();

    let folders = folders_raw
        .into_iter()
        .map(|f| ExportedFolder {
            path: f.path,
            name: f.name,
            enabled: f.enabled,
            recursive: f.recursive,
            settings: f.settings,
        })
        .collect();

    let categories = repo_categories::list(db).await?;

    let exclusions = repo_exclusions::list(db)
        .await?
        .into_iter()
        .map(|e| ExportedExclusion {
            folder_path: e.folder_id.and_then(|fid| id_to_path.get(&fid).cloned()),
            path: e.path,
            exclusion_type: e.exclusion_type,
        })
        .collect();

    let rules = repo_rules::list(db)
        .await?
        .into_iter()
        .map(|r| ExportedRule {
            name: r.name,
            description: r.description,
            enabled: r.enabled,
            priority: r.priority,
            source_folders: r
                .source_folders
                .into_iter()
                .map(|sf| id_to_path.get(&sf).cloned().unwrap_or(sf))
                .collect(),
            conditions: r.conditions,
            actions: r.actions,
            date_source: r.date_source,
            execution_mode: r.execution_mode,
            origin: r.origin,
        })
        .collect();

    let bundle = ConfigBundle {
        schema_version: CONFIG_SCHEMA_VERSION,
        exported_at: now_ms,
        app_version: app_version.to_owned(),
        settings,
        folders,
        categories,
        exclusions,
        rules,
    };

    Ok(serde_json::to_string_pretty(&bundle)?)
}
