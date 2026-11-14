//! Strict validation, diff preview, and transactional import of configuration bundles
//! (`P6-T05`, FR-191..192).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::categories::{builtin_categories, normalize_input, CategoryInput, COLOR_TOKENS, OTHER};
use crate::config_io::schema::{
    ConfigBundle, ConfigImportMode, ConfigImportPreview, ConfigImportReport, ExportedRule,
    SettingsDiffItem, CONFIG_SCHEMA_VERSION,
};
use crate::database::repo_exclusions::{self, ExclusionInput, ExclusionKind};
use crate::database::{repo_categories, repo_folders, repo_rules, Db};
use crate::engine::paths::{check_not_forbidden_root, lexical_check, nfc, RootPolicy};
use crate::error::{AppError, AppResult};
use crate::folders::FolderHealth;
use crate::rules::models::{
    encode_actions_json, encode_conditions_json, Action, RuleDraft, RuleOrigin,
};
use crate::rules::validate::{ensure_valid_rule, MonitoredFolderRef, ValidationContext};
use crate::settings::SettingsService;

fn nfc_str(raw: &str) -> String {
    nfc(Path::new(raw)).to_string_lossy().into_owned()
}

fn rule_action_roots(actions: &[Action]) -> Vec<&str> {
    let mut out = Vec::new();
    for a in actions {
        match a {
            Action::Move { destination } | Action::Copy { destination } => {
                out.push(destination.as_str());
            }
            Action::CreateDestinationFolder { path } => {
                out.push(path.as_str());
            }
            Action::MoveIntoDynamicFolder {
                root: Some(root), ..
            } => {
                out.push(root.as_str());
            }
            _ => {}
        }
    }
    out
}

fn rule_action_targets(actions: &[Action]) -> Vec<String> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::ApplyAnotherRule { rule_id } if !rule_id.trim().is_empty() => {
                Some(rule_id.trim().to_owned())
            }
            _ => None,
        })
        .collect()
}

fn validate_rule_to_draft(
    r: &ExportedRule,
    policy: &RootPolicy,
    warnings: &mut Vec<String>,
) -> AppResult<(RuleDraft, Vec<PathBuf>)> {
    for sf in &r.source_folders {
        if sf.trim().is_empty() || sf.contains('\0') || sf.contains("..") {
            return Err(AppError::validation(
                "rules.sourceFolders",
                "Invalid source folder filter in imported rule.",
            ));
        }
    }
    let mut approved_roots = Vec::new();
    for dest in rule_action_roots(&r.actions) {
        let p = Path::new(dest);
        let safe = check_not_forbidden_root(p, policy).map_err(|e| {
            AppError::validation(
                "rules.actions",
                format!("Rule \"{}\" has unsafe path \"{dest}\": {e}", r.name),
            )
        })?;
        approved_roots.push(safe);
    }
    if r.enabled {
        warnings.push(format!(
            "Rule \"{}\" was enabled in export and will be imported disabled until you review it.",
            r.name
        ));
    }
    let draft = RuleDraft {
        name: r.name.clone(),
        description: r.description.clone(),
        enabled: false,
        source_folders: r.source_folders.clone(),
        conditions: r.conditions.clone(),
        actions: r.actions.clone(),
        date_source: r.date_source,
        execution_mode: r.execution_mode,
    };
    Ok((draft, approved_roots))
}

struct ValidatedBundle {
    bundle: ConfigBundle,
    extra_approved_roots: Vec<PathBuf>,
    warnings: Vec<String>,
}

async fn parse_and_validate_bundle(
    db: &Db,
    policy: &RootPolicy,
    raw_json: &str,
    mode: ConfigImportMode,
) -> AppResult<ValidatedBundle> {
    let mut bundle: ConfigBundle = serde_json::from_str(raw_json)
        .map_err(|e| AppError::validation("config", format!("Invalid configuration JSON: {e}")))?;

    if bundle.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(AppError::validation(
            "schemaVersion",
            format!(
                "Unsupported configuration schema version {} (expected {}).",
                bundle.schema_version, CONFIG_SCHEMA_VERSION
            ),
        ));
    }

    bundle.settings.validate()?;
    let mut extra_approved_roots = Vec::new();
    if let Some(dest) = &bundle.settings.organization.default_destination {
        let safe = check_not_forbidden_root(Path::new(dest), policy).map_err(|e| {
            AppError::validation(
                "settings.organization.defaultDestination",
                format!("Invalid default destination: {e}"),
            )
        })?;
        extra_approved_roots.push(safe);
    }

    let mut warnings = Vec::new();

    // Validate folders + ensure no internal duplicates or overlaps.
    let mut seen_folder_paths: Vec<PathBuf> = Vec::new();
    for folder in &mut bundle.folders {
        folder.name = folder.name.trim().to_owned();
        if folder.name.is_empty() || folder.name.chars().count() > 80 {
            return Err(AppError::validation(
                "folders.name",
                "Imported folder name must be 1–80 characters.",
            ));
        }
        let clean = check_not_forbidden_root(Path::new(&folder.path), policy).map_err(|e| {
            AppError::validation(
                "folders.path",
                format!("Invalid folder path \"{}\": {e}", folder.path),
            )
        })?;
        for prev in &seen_folder_paths {
            if &clean == prev || clean.starts_with(prev) || prev.starts_with(&clean) {
                return Err(AppError::validation(
                    "folders.path",
                    format!("Imported folders overlap around \"{}\".", folder.path),
                ));
            }
        }
        for sub in &folder.settings.excluded_subfolders {
            lexical_check(Path::new(sub)).map_err(|e| {
                AppError::validation(
                    "folders.excludedSubfolders",
                    format!("Invalid excluded subfolder \"{sub}\": {e}"),
                )
            })?;
            if !nfc(Path::new(sub)).starts_with(&clean) {
                return Err(AppError::validation(
                    "folders.excludedSubfolders",
                    format!(
                        "Excluded subfolder \"{sub}\" must lie inside \"{}\".",
                        folder.path
                    ),
                ));
            }
        }
        if !clean.exists() {
            warnings.push(format!(
                "Folder \"{}\" does not exist on this Mac yet.",
                clean.display()
            ));
        }
        folder.path = clean.to_string_lossy().into_owned();
        seen_folder_paths.push(clean);
    }

    // Validate categories: protect built-ins (especially `'other'`), forbid collisions and duplicates.
    let builtins = builtin_categories();
    let mut seen_cat_ids: HashSet<String> = HashSet::new();
    let mut seen_cat_names: HashSet<String> = HashSet::new();
    for cat in &mut bundle.categories {
        if cat.id == OTHER {
            if !cat.builtin
                || !cat.enabled
                || !cat.extensions.is_empty()
                || !cat.name.trim().eq_ignore_ascii_case("Other")
            {
                return Err(AppError::validation(
                    "categories",
                    "The built-in 'Other' category cannot be disabled or modified.",
                ));
            }
            cat.name = "Other".to_owned();
        } else if cat.builtin {
            let Some(expected_builtin) = builtins.iter().find(|b| b.id == cat.id) else {
                return Err(AppError::validation(
                    "categories.id",
                    format!("Unknown built-in category ID \"{}\".", cat.id),
                ));
            };
            let norm = normalize_input(CategoryInput {
                name: cat.name.clone(),
                description: cat.description.clone(),
                icon: cat.icon.clone(),
                color_token: cat.color_token.clone(),
                extensions: cat.extensions.clone(),
                destination_path: cat.destination_path.clone(),
                priority: cat.priority,
                enabled: cat.enabled,
            })?;
            if builtins
                .iter()
                .any(|b| b.id != expected_builtin.id && b.name.eq_ignore_ascii_case(&norm.name))
            {
                return Err(AppError::validation(
                    "categories.name",
                    format!(
                        "Built-in category \"{}\" collides with another built-in name.",
                        norm.name
                    ),
                ));
            }
            cat.name = norm.name;
            cat.description = norm.description;
            cat.icon = norm.icon;
            cat.color_token = norm.color_token;
            cat.extensions = norm.extensions;
            cat.destination_path = norm.destination_path;
        } else {
            let norm = normalize_input(CategoryInput {
                name: cat.name.clone(),
                description: cat.description.clone(),
                icon: cat.icon.clone(),
                color_token: cat.color_token.clone(),
                extensions: cat.extensions.clone(),
                destination_path: cat.destination_path.clone(),
                priority: cat.priority,
                enabled: cat.enabled,
            })?;
            if builtins
                .iter()
                .any(|b| b.id == cat.id || b.name.eq_ignore_ascii_case(&norm.name))
            {
                return Err(AppError::validation(
                    "categories.name",
                    format!(
                        "Custom category \"{}\" conflicts with a built-in category.",
                        norm.name
                    ),
                ));
            }
            cat.name = norm.name;
            cat.description = norm.description;
            cat.icon = norm.icon;
            cat.color_token = norm.color_token;
            cat.extensions = norm.extensions;
            cat.destination_path = norm.destination_path;
        }
        let cid_trimmed = cat.id.trim();
        if !cid_trimmed.is_empty() && !seen_cat_ids.insert(cid_trimmed.to_owned()) {
            return Err(AppError::validation(
                "categories.id",
                format!("Duplicate category ID \"{}\".", cid_trimmed),
            ));
        }
        if !seen_cat_names.insert(cat.name.to_lowercase()) {
            return Err(AppError::validation(
                "categories.name",
                format!("Duplicate category name \"{}\".", cat.name),
            ));
        }
        if !COLOR_TOKENS.contains(&cat.color_token.as_str()) {
            return Err(AppError::validation(
                "categories.colorToken",
                format!("Unknown category color token \"{}\".", cat.color_token),
            ));
        }
        if let Some(dest) = &cat.destination_path {
            let safe = check_not_forbidden_root(Path::new(dest), policy).map_err(|e| {
                AppError::validation(
                    "categories.destinationPath",
                    format!("Invalid category destination \"{dest}\": {e}"),
                )
            })?;
            extra_approved_roots.push(safe);
        }
    }

    // Validate exclusions and reject internal duplicates.
    let mut seen_exclusions: HashSet<(Option<String>, ExclusionKind, String)> = HashSet::new();
    for ex in &mut bundle.exclusions {
        if let Some(fp) = &ex.folder_path {
            lexical_check(Path::new(fp)).map_err(|e| {
                AppError::validation("exclusions.folderPath", format!("Invalid path: {e}"))
            })?;
            ex.folder_path = Some(nfc_str(fp));
        }
        let validated = repo_exclusions::validate_input(ExclusionInput {
            folder_id: None,
            path: ex.path.clone(),
            exclusion_type: ex.exclusion_type,
        })?;
        ex.path = validated.path;
        let key = (ex.folder_path.clone(), ex.exclusion_type, ex.path.clone());
        if !seen_exclusions.insert(key) {
            return Err(AppError::validation(
                "exclusions",
                format!("Duplicate exclusion \"{}\".", ex.path),
            ));
        }
    }

    // Build RuleValidationContext combining incoming + existing (when merging).
    let existing_folders = repo_folders::list(db).await?;
    let existing_dests = repo_rules::list_destination_roots(db).await?;
    let mut monitored_refs: Vec<MonitoredFolderRef> = Vec::new();
    for f in &bundle.folders {
        monitored_refs.push(MonitoredFolderRef {
            id: f.path.clone(),
            path: PathBuf::from(&f.path),
            recursive: f.recursive,
            excluded_subfolders: f
                .settings
                .excluded_subfolders
                .iter()
                .map(PathBuf::from)
                .collect(),
        });
    }
    if mode == ConfigImportMode::Merge {
        for f in &existing_folders {
            let p = PathBuf::from(&f.path);
            if !monitored_refs.iter().any(|m| m.path == p) {
                monitored_refs.push(MonitoredFolderRef {
                    id: f.id.clone(),
                    path: p,
                    recursive: f.recursive,
                    excluded_subfolders: f
                        .settings
                        .excluded_subfolders
                        .iter()
                        .map(PathBuf::from)
                        .collect(),
                });
            }
        }
    }

    let mut seen_rule_names: HashSet<String> = HashSet::new();
    let mut drafts_and_roots = Vec::with_capacity(bundle.rules.len());
    let mut rule_links: HashMap<String, Vec<String>> = HashMap::new();
    if mode == ConfigImportMode::Merge {
        for er in repo_rules::list(db).await? {
            let targets = rule_action_targets(&er.actions);
            if !targets.is_empty() {
                rule_links.insert(er.id, targets);
            }
        }
    }
    for r in &bundle.rules {
        let norm_name = r.name.trim().to_lowercase();
        if !seen_rule_names.insert(norm_name) {
            return Err(AppError::validation(
                "rules.name",
                format!("Duplicate rule name \"{}\".", r.name.trim()),
            ));
        }
        let targets = rule_action_targets(&r.actions);
        if !targets.is_empty() {
            rule_links.insert(r.name.trim().to_owned(), targets);
        }
        let (draft, roots) = validate_rule_to_draft(r, policy, &mut warnings)?;
        for root in roots {
            if !extra_approved_roots.contains(&root) {
                extra_approved_roots.push(root);
            }
        }
        drafts_and_roots.push(draft);
    }

    let mut approved_roots: Vec<PathBuf> = extra_approved_roots.clone();
    for m in &monitored_refs {
        approved_roots.push(m.path.clone());
    }
    for d in existing_dests {
        let p = PathBuf::from(d);
        if !approved_roots.contains(&p) {
            approved_roots.push(p);
        }
    }

    let val_ctx = ValidationContext {
        current_rule_id: None,
        monitored_folders: monitored_refs,
        approved_roots,
        rule_links,
    };
    for draft in &drafts_and_roots {
        ensure_valid_rule(draft, &val_ctx)?;
    }

    Ok(ValidatedBundle {
        bundle,
        extra_approved_roots,
        warnings,
    })
}

pub async fn preview_config_import(
    db: &Db,
    policy: &RootPolicy,
    raw_json: &str,
) -> AppResult<ConfigImportPreview> {
    preview_config_import_with_mode(db, policy, raw_json, ConfigImportMode::Merge).await
}

pub async fn preview_config_import_with_mode(
    db: &Db,
    policy: &RootPolicy,
    raw_json: &str,
    mode: ConfigImportMode,
) -> AppResult<ConfigImportPreview> {
    let mut validated = parse_and_validate_bundle(db, policy, raw_json, mode).await?;
    let b = &validated.bundle;
    if mode == ConfigImportMode::Replace {
        validated.warnings.insert(
            0,
            "Replace mode will remove existing monitored folders, custom categories, exclusions, rules, and stale duplicate groups before importing."
                .to_owned(),
        );
    }

    let current_settings = SettingsService::new(db.clone()).get().await?;
    let mut settings_changes = Vec::new();
    let cur_secs = current_settings.to_sections()?;
    let inc_secs = b.settings.to_sections()?;
    for ((k, cur_v), (_, inc_v)) in cur_secs.iter().zip(inc_secs.iter()) {
        if cur_v != inc_v {
            settings_changes.push(SettingsDiffItem {
                section: (*k).to_owned(),
                current: cur_v.clone(),
                incoming: inc_v.clone(),
            });
        }
    }

    let cur_folders = repo_folders::list(db).await?;
    let cur_folder_by_path: HashMap<String, _> =
        cur_folders.iter().map(|f| (nfc_str(&f.path), f)).collect();
    let mut folders_to_add = Vec::new();
    let mut folders_to_update = Vec::new();
    let mut folders_unchanged = Vec::new();
    for f in &b.folders {
        if mode == ConfigImportMode::Replace {
            folders_to_add.push(f.name.clone());
            continue;
        }
        match cur_folder_by_path.get(&nfc_str(&f.path)) {
            None => folders_to_add.push(f.name.clone()),
            Some(existing)
                if existing.name == f.name
                    && existing.enabled == f.enabled
                    && existing.recursive == f.recursive
                    && existing.settings == f.settings =>
            {
                folders_unchanged.push(f.name.clone());
            }
            Some(_) => folders_to_update.push(f.name.clone()),
        }
    }

    let cur_cats = repo_categories::list(db).await?;
    let mut categories_to_add = Vec::new();
    let mut categories_to_update = Vec::new();
    for c in &b.categories {
        if c.id == OTHER {
            continue;
        }
        if mode == ConfigImportMode::Replace && !c.builtin {
            categories_to_add.push(c.name.clone());
            continue;
        }
        let existing = if c.builtin {
            cur_cats.iter().find(|ec| ec.id == c.id)
        } else {
            cur_cats
                .iter()
                .find(|ec| !ec.builtin && (ec.id == c.id || ec.name.eq_ignore_ascii_case(&c.name)))
        };
        match existing {
            None => categories_to_add.push(c.name.clone()),
            Some(ec) if ec != c => categories_to_update.push(c.name.clone()),
            Some(_) => {}
        }
    }

    let mut target_folder_paths: HashSet<String> =
        b.folders.iter().map(|f| nfc_str(&f.path)).collect();
    let mut target_folder_ids: HashSet<String> = HashSet::new();
    if mode == ConfigImportMode::Merge {
        for f in &cur_folders {
            target_folder_paths.insert(nfc_str(&f.path));
            target_folder_ids.insert(f.id.clone());
        }
    }

    let cur_exclusions = repo_exclusions::list(db).await?;
    let id_to_path: HashMap<String, String> = cur_folders
        .iter()
        .map(|f| (f.id.clone(), nfc_str(&f.path)))
        .collect();
    let existing_ex_keys: HashSet<(Option<String>, ExclusionKind, String)> = cur_exclusions
        .into_iter()
        .map(|e| {
            (
                e.folder_id.and_then(|id| id_to_path.get(&id).cloned()),
                e.exclusion_type,
                e.path,
            )
        })
        .collect();
    let mut exclusions_to_add = Vec::new();
    let mut exclusions_unchanged = Vec::new();
    for ex in &b.exclusions {
        if let Some(fp) = &ex.folder_path {
            if !target_folder_paths.contains(&nfc_str(fp)) {
                validated.warnings.push(format!(
                    "Will skip exclusion \"{}\" because monitored folder \"{fp}\" is not present in target configuration.",
                    ex.path
                ));
                continue;
            }
        }
        if mode == ConfigImportMode::Replace {
            exclusions_to_add.push(ex.path.clone());
            continue;
        }
        let key = (
            ex.folder_path.as_deref().map(nfc_str),
            ex.exclusion_type,
            ex.path.clone(),
        );
        if existing_ex_keys.contains(&key) {
            exclusions_unchanged.push(ex.path.clone());
        } else {
            exclusions_to_add.push(ex.path.clone());
        }
    }

    let cur_rules = repo_rules::list(db).await?;
    let cur_rule_names: HashSet<String> = cur_rules
        .into_iter()
        .map(|r| r.name.to_lowercase())
        .collect();
    let mut rules_to_add = Vec::new();
    let mut rules_to_update = Vec::new();
    for r in &b.rules {
        if let Some(missing) = r.source_folders.iter().find(|sf| {
            !target_folder_paths.contains(&nfc_str(sf)) && !target_folder_ids.contains(*sf)
        }) {
            validated.warnings.push(format!(
                "Will skip rule \"{}\" because source folder \"{missing}\" is not present in target configuration.",
                r.name
            ));
            continue;
        }
        if mode == ConfigImportMode::Merge && cur_rule_names.contains(&r.name.trim().to_lowercase())
        {
            rules_to_update.push(r.name.clone());
        } else {
            rules_to_add.push(r.name.clone());
        }
    }

    let forced_disabled_rule_count =
        u32::try_from(rules_to_add.len().saturating_add(rules_to_update.len())).unwrap_or(u32::MAX);

    Ok(ConfigImportPreview {
        schema_version: b.schema_version,
        exported_at: b.exported_at,
        settings_changes,
        folders_to_add,
        folders_to_update,
        folders_unchanged,
        categories_to_add,
        categories_to_update,
        exclusions_to_add,
        exclusions_unchanged,
        rules_to_add,
        rules_to_update,
        forced_disabled_rule_count,
        warnings: validated.warnings,
    })
}

pub async fn apply_config_import(
    db: &Db,
    policy: &RootPolicy,
    raw_json: &str,
    mode: ConfigImportMode,
    now_ms: i64,
) -> AppResult<ConfigImportReport> {
    let validated = parse_and_validate_bundle(db, policy, raw_json, mode).await?;
    let mut warnings = validated.warnings;
    let b = validated.bundle;

    let mut tx = db.begin().await?;

    // 1. Settings
    for (key, value) in b.settings.to_sections()? {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .bind(now_ms)
        .execute(&mut *tx)
        .await?;
    }

    if mode == ConfigImportMode::Replace {
        sqlx::query("DELETE FROM duplicate_groups")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM rules").execute(&mut *tx).await?;
        sqlx::query("DELETE FROM exclusions")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM categories WHERE builtin = 0")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM monitored_folders")
            .execute(&mut *tx)
            .await?;
    }

    // 2. Categories
    let mut categories_imported: u32 = 0;
    for cat in &b.categories {
        if cat.id == OTHER {
            categories_imported = categories_imported.saturating_add(1);
            continue;
        }
        let exts_json = serde_json::to_string(&cat.extensions)?;
        if cat.builtin {
            let r = sqlx::query(
                "UPDATE categories SET name = ?2, description = ?3, icon = ?4, color_token = ?5, \
                 extensions_json = ?6, destination_path = ?7, priority = ?8, enabled = ?9, updated_at = ?10 \
                 WHERE id = ?1",
            )
            .bind(&cat.id)
            .bind(&cat.name)
            .bind(&cat.description)
            .bind(&cat.icon)
            .bind(&cat.color_token)
            .bind(&exts_json)
            .bind(&cat.destination_path)
            .bind(cat.priority)
            .bind(cat.enabled)
            .bind(now_ms)
            .execute(&mut *tx)
            .await?;
            if r.rows_affected() > 0 {
                categories_imported = categories_imported.saturating_add(1);
            }
        } else {
            let existing_id: Option<String> = sqlx::query_scalar(
                "SELECT id FROM categories WHERE builtin = 0 AND (id = ?1 OR LOWER(name) = LOWER(?2)) LIMIT 1",
            )
            .bind(&cat.id)
            .bind(&cat.name)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(eid) = existing_id {
                sqlx::query(
                    "UPDATE categories SET name = ?2, description = ?3, icon = ?4, color_token = ?5, \
                     extensions_json = ?6, destination_path = ?7, priority = ?8, enabled = ?9, updated_at = ?10 \
                     WHERE id = ?1",
                )
                .bind(&eid)
                .bind(&cat.name)
                .bind(&cat.description)
                .bind(&cat.icon)
                .bind(&cat.color_token)
                .bind(&exts_json)
                .bind(&cat.destination_path)
                .bind(cat.priority)
                .bind(cat.enabled)
                .bind(now_ms)
                .execute(&mut *tx)
                .await?;
            } else {
                let id = if cat.id.trim().is_empty() {
                    Uuid::now_v7().to_string()
                } else {
                    cat.id.clone()
                };
                sqlx::query(
                    "INSERT INTO categories (id, name, description, icon, color_token, extensions_json, \
                     destination_path, priority, builtin, enabled, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, ?10)",
                )
                .bind(&id)
                .bind(&cat.name)
                .bind(&cat.description)
                .bind(&cat.icon)
                .bind(&cat.color_token)
                .bind(&exts_json)
                .bind(&cat.destination_path)
                .bind(cat.priority)
                .bind(cat.enabled)
                .bind(now_ms)
                .execute(&mut *tx)
                .await?;
            }
            categories_imported = categories_imported.saturating_add(1);
        }
    }

    // 3. Monitored folders & destination roots
    let mut path_to_folder_id: HashMap<String, String> = HashMap::new();
    let existing_folders: Vec<(String, String)> =
        sqlx::query_as("SELECT id, path FROM monitored_folders")
            .fetch_all(&mut *tx)
            .await?;
    for (id, path) in existing_folders {
        path_to_folder_id.insert(nfc_str(&path), id);
    }

    let mut folders_imported: u32 = 0;
    for folder in &b.folders {
        let norm_path = nfc_str(&folder.path);
        let settings_json = serde_json::to_string(&folder.settings)?;
        if let Some(eid) = path_to_folder_id.get(&norm_path).cloned() {
            sqlx::query(
                "UPDATE monitored_folders SET name = ?2, enabled = ?3, recursive = ?4, \
                 settings_json = ?5, updated_at = ?6 WHERE id = ?1",
            )
            .bind(&eid)
            .bind(&folder.name)
            .bind(folder.enabled)
            .bind(folder.recursive)
            .bind(&settings_json)
            .bind(now_ms)
            .execute(&mut *tx)
            .await?;
        } else {
            let new_id = Uuid::now_v7().to_string();
            sqlx::query(
                "INSERT INTO monitored_folders (id, name, path, enabled, recursive, settings_json, health, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            )
            .bind(&new_id)
            .bind(&folder.name)
            .bind(&norm_path)
            .bind(folder.enabled)
            .bind(folder.recursive)
            .bind(&settings_json)
            .bind(FolderHealth::Unknown.as_db())
            .bind(now_ms)
            .execute(&mut *tx)
            .await?;
            path_to_folder_id.insert(norm_path, new_id);
        }
        folders_imported = folders_imported.saturating_add(1);
    }

    for root_path in &validated.extra_approved_roots {
        let p_str = root_path.to_string_lossy();
        sqlx::query(
            "INSERT INTO destination_roots (id, path, created_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(path) DO NOTHING",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(p_str.as_ref())
        .bind(now_ms)
        .execute(&mut *tx)
        .await?;
    }

    // 4. Exclusions — NEVER widen an unresolved per-folder exclusion into a global NULL exclusion.
    let mut exclusions_imported: u32 = 0;
    for ex in &b.exclusions {
        let fid = if let Some(fp) = &ex.folder_path {
            let Some(id) = path_to_folder_id.get(&nfc_str(fp)).cloned() else {
                warnings.push(format!(
                    "Skipped exclusion \"{}\" because monitored folder \"{fp}\" does not exist in target configuration.",
                    ex.path
                ));
                continue;
            };
            Some(id)
        } else {
            None
        };
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM exclusions \
             WHERE COALESCE(folder_id, '') = COALESCE(?1, '') AND path = ?2 AND exclusion_type = ?3 LIMIT 1",
        )
        .bind(&fid)
        .bind(&ex.path)
        .bind(ex.exclusion_type.as_db())
        .fetch_optional(&mut *tx)
        .await?;

        if exists.is_none() {
            sqlx::query(
                "INSERT INTO exclusions (id, folder_id, path, exclusion_type, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(&fid)
            .bind(&ex.path)
            .bind(ex.exclusion_type.as_db())
            .bind(now_ms)
            .execute(&mut *tx)
            .await?;
            exclusions_imported = exclusions_imported.saturating_add(1);
        }
    }

    // 5. Rules — ALWAYS force enabled = false and origin = RuleOrigin::Import (FR-192),
    // and remap source_folders paths to target DB folder IDs without widening unresolved entries!
    let valid_folder_ids: HashSet<String> = path_to_folder_id.values().cloned().collect();
    let mut rules_imported_disabled: u32 = 0;
    for r in &b.rules {
        let mut resolved_source_folders = Vec::with_capacity(r.source_folders.len());
        let mut missing_sf: Option<&str> = None;
        for sf in &r.source_folders {
            if let Some(id) = path_to_folder_id.get(&nfc_str(sf)) {
                if !resolved_source_folders.contains(id) {
                    resolved_source_folders.push(id.clone());
                }
            } else if valid_folder_ids.contains(sf) {
                if !resolved_source_folders.contains(sf) {
                    resolved_source_folders.push(sf.clone());
                }
            } else {
                missing_sf = Some(sf.as_str());
                break;
            }
        }
        if let Some(missing) = missing_sf {
            warnings.push(format!(
                "Skipped rule \"{}\" because source folder \"{missing}\" does not exist in target configuration.",
                r.name
            ));
            continue;
        }
        let sf_json = serde_json::to_string(&resolved_source_folders)?;
        let cond_json = encode_conditions_json(&r.conditions)?;
        let act_json = encode_actions_json(&r.actions, r.date_source)?;
        let existing_id: Option<String> = if mode == ConfigImportMode::Merge {
            sqlx::query_scalar("SELECT id FROM rules WHERE LOWER(name) = LOWER(?1) LIMIT 1")
                .bind(r.name.trim())
                .fetch_optional(&mut *tx)
                .await?
        } else {
            None
        };

        if let Some(rid) = existing_id {
            sqlx::query(
                "UPDATE rules SET name = ?2, description = ?3, enabled = 0, priority = ?4, \
                 source_folders_json = ?5, conditions_json = ?6, actions_json = ?7, \
                 execution_mode = ?8, origin = ?9, updated_at = ?10 WHERE id = ?1",
            )
            .bind(&rid)
            .bind(r.name.trim())
            .bind(&r.description)
            .bind(r.priority)
            .bind(&sf_json)
            .bind(&cond_json)
            .bind(&act_json)
            .bind(r.execution_mode.as_db())
            .bind(RuleOrigin::Import.as_db())
            .bind(now_ms)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO rules (id, name, description, enabled, priority, source_folders_json, \
                 conditions_json, actions_json, execution_mode, origin, last_run_at, \
                 stat_matched, stat_succeeded, stat_failed, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?8, ?9, NULL, 0, 0, 0, ?10, ?10)",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(r.name.trim())
            .bind(&r.description)
            .bind(r.priority)
            .bind(&sf_json)
            .bind(&cond_json)
            .bind(&act_json)
            .bind(r.execution_mode.as_db())
            .bind(RuleOrigin::Import.as_db())
            .bind(now_ms)
            .execute(&mut *tx)
            .await?;
        }
        rules_imported_disabled = rules_imported_disabled.saturating_add(1);
    }

    tx.commit().await?;

    Ok(ConfigImportReport {
        mode,
        folders_imported,
        categories_imported,
        exclusions_imported,
        rules_imported_disabled,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::{CategoryInput, CategoryService};
    use crate::config_io::export_config;
    use crate::config_io::schema::{ConfigBundle, CONFIG_SCHEMA_VERSION};
    use crate::database::repo_exclusions::{self, ExclusionInput, ExclusionKind};
    use crate::database::{repo_folders, repo_rules};
    use crate::folders::FolderSettings;
    use crate::rules::models::{
        Action, Condition, ConditionNode, ExecutionMode, RuleDraft, RuleOrigin, TextFieldKind,
        TextOp,
    };
    use crate::rules::service::RuleService;
    use crate::settings::SettingsService;
    use crate::test_support::{mkdir, tempdir};
    use std::path::Path;

    fn test_policy(root: &Path) -> RootPolicy {
        RootPolicy {
            home: root.join("home"),
            app_data_dir: root.join("app_data"),
        }
    }

    fn simple_rule(name: &str, dest: &str, enabled: bool) -> RuleDraft {
        RuleDraft {
            name: name.into(),
            description: Some("Sort docs".into()),
            enabled,
            source_folders: Vec::new(),
            conditions: ConditionNode::Leaf {
                condition: Condition::Text {
                    field: TextFieldKind::Name,
                    op: TextOp::Contains,
                    value: "report".into(),
                    case_sensitive: false,
                },
            },
            actions: vec![Action::Move {
                destination: dest.into(),
            }],
            date_source: Default::default(),
            execution_mode: ExecutionMode::FirstMatch,
        }
    }

    #[tokio::test]
    async fn r_io_01_round_trip_preserves_settings_folders_categories_exclusions_and_rules() {
        let (_d, root) = tempdir();
        let policy = test_policy(&root);
        let downloads = root.join("home").join("Downloads");
        let docs_dest = root.join("home").join("SortedDocs");
        mkdir(&policy.home);
        mkdir(&downloads);
        mkdir(&docs_dest);

        // Source DB populated with settings, folder, custom category, exclusion, enabled rule
        let src_db = crate::database::open_in_memory().await.expect("src db");
        let cat_svc = CategoryService::new(src_db.clone());
        cat_svc.ensure_builtins().await.expect("builtins");
        cat_svc
            .create(CategoryInput {
                name: "3D Prints".into(),
                description: Some("CAD meshes".into()),
                icon: "box".into(),
                color_token: "cat-design".into(),
                extensions: vec!["stl".into(), "3mf".into()],
                destination_path: Some(docs_dest.to_string_lossy().into_owned()),
                priority: 10,
                enabled: true,
            })
            .await
            .expect("custom cat");

        let set_svc = SettingsService::new(src_db.clone());
        let mut s = set_svc.get().await.expect("load settings");
        s.general.confirm_bulk_operations = false;
        s.organization.default_destination = Some(docs_dest.to_string_lossy().into_owned());
        set_svc.update(s).await.expect("save settings");

        repo_folders::insert(
            &src_db,
            "f-dl",
            "Downloads",
            &downloads.to_string_lossy(),
            true,
        )
        .await
        .expect("folder");
        let mut f = repo_folders::get(&src_db, "f-dl")
            .await
            .expect("get folder")
            .expect("exists");
        f.settings = FolderSettings {
            exclude_extensions: vec!["iso".into()],
            ..Default::default()
        };
        repo_folders::update(&src_db, &f)
            .await
            .expect("update folder");

        repo_exclusions::create(
            &src_db,
            ExclusionInput {
                folder_id: Some("f-dl".into()),
                path: "*.part".into(),
                exclusion_type: ExclusionKind::Glob,
            },
        )
        .await
        .expect("exclusion");

        let rule_svc = RuleService::new(src_db.clone(), policy.clone());
        rule_svc
            .add_destination_root(&docs_dest)
            .await
            .expect("dest root");
        let r = rule_svc
            .create(
                simple_rule("Reports Rule", &docs_dest.to_string_lossy(), true),
                RuleOrigin::User,
            )
            .await
            .expect("rule");
        assert!(r.enabled);

        let exported_json = export_config(&src_db, "0.6.0", 5_000)
            .await
            .expect("export");
        let bundle: ConfigBundle = serde_json::from_str(&exported_json).expect("valid bundle");
        assert_eq!(bundle.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(bundle.rules.len(), 1);
        assert_eq!(bundle.folders.len(), 1);
        assert_eq!(bundle.exclusions.len(), 1);

        // Import into a completely fresh target DB
        let dst_db = crate::database::open_in_memory().await.expect("dst db");
        CategoryService::new(dst_db.clone())
            .ensure_builtins()
            .await
            .expect("dst builtins");

        let report = apply_config_import(
            &dst_db,
            &policy,
            &exported_json,
            ConfigImportMode::Merge,
            6_000,
        )
        .await
        .expect("apply import");

        assert_eq!(report.rules_imported_disabled, 1);
        assert_eq!(report.folders_imported, 1);
        assert_eq!(report.exclusions_imported, 1);

        let imported_settings = SettingsService::new(dst_db.clone()).get().await.expect("s");
        assert!(!imported_settings.general.confirm_bulk_operations);
        assert_eq!(
            imported_settings
                .organization
                .default_destination
                .as_deref(),
            Some(docs_dest.to_string_lossy().as_ref())
        );

        let dst_folders = repo_folders::list(&dst_db).await.expect("folders");
        assert_eq!(dst_folders.len(), 1);
        assert_eq!(dst_folders[0].settings.exclude_extensions, vec!["iso"]);

        let dst_cats = CategoryService::new(dst_db.clone())
            .list()
            .await
            .expect("cats");
        assert!(dst_cats.iter().any(|c| c.name == "3D Prints" && !c.builtin));

        let dst_ex = repo_exclusions::list(&dst_db).await.expect("ex");
        assert_eq!(dst_ex.len(), 1);
        assert_eq!(dst_ex[0].path, "*.part");

        let dst_rules = repo_rules::list(&dst_db).await.expect("rules");
        assert_eq!(dst_rules.len(), 1);
        assert_eq!(dst_rules[0].name, "Reports Rule");
        assert!(
            !dst_rules[0].enabled,
            "FR-192: imported rules MUST be disabled"
        );
        assert_eq!(dst_rules[0].origin, RuleOrigin::Import);
    }

    #[tokio::test]
    async fn r_io_02_rejects_unknown_missing_or_malformed_schema_version() {
        let (_d, root) = tempdir();
        let policy = test_policy(&root);
        let db = crate::database::open_in_memory().await.expect("db");
        CategoryService::new(db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");

        let base_json = export_config(&db, "0.6.0", 1_000).await.expect("export");
        assert!(!base_json.is_empty());

        let mut val: serde_json::Value = serde_json::from_str(&base_json).expect("json");
        val["schemaVersion"] = serde_json::json!(99);
        assert!(preview_config_import(&db, &policy, &val.to_string())
            .await
            .is_err());
        assert!(apply_config_import(
            &db,
            &policy,
            &val.to_string(),
            ConfigImportMode::Merge,
            2_000
        )
        .await
        .is_err());

        val["schemaVersion"] = serde_json::json!(0);
        assert!(preview_config_import(&db, &policy, &val.to_string())
            .await
            .is_err());

        val.as_object_mut().expect("obj").remove("schemaVersion");
        assert!(preview_config_import(&db, &policy, &val.to_string())
            .await
            .is_err());

        assert!(preview_config_import(&db, &policy, "{not json")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn r_io_03_rejects_malicious_paths_traversal_relative_nul_and_protected_roots() {
        let (_d, root) = tempdir();
        let policy = test_policy(&root);
        mkdir(&policy.home);
        let db = crate::database::open_in_memory().await.expect("db");
        CategoryService::new(db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        let base_json = export_config(&db, "0.6.0", 1_000).await.expect("export");

        // Inject malicious monitored folder paths
        for bad_path in [
            "relative/folder",
            "/Users/someone/../etc",
            "/System",
            "/Library",
            "/Applications",
            "/etc",
            "/Users/bad\0null",
        ] {
            let mut b: ConfigBundle = serde_json::from_str(&base_json).expect("parse");
            b.folders.push(crate::config_io::ExportedFolder {
                path: bad_path.into(),
                name: "Bad".into(),
                enabled: true,
                recursive: true,
                settings: FolderSettings::default(),
            });
            let raw = serde_json::to_string(&b).expect("ser");
            assert!(
                preview_config_import(&db, &policy, &raw).await.is_err(),
                "folder path {bad_path} must be rejected"
            );
        }

        // Inject malicious default destination and rule move destinations
        for bad_dest in ["../../etc", "relative/dest", "/System/Library", "/usr/bin"] {
            let mut b: ConfigBundle = serde_json::from_str(&base_json).expect("parse");
            b.rules.push(crate::config_io::ExportedRule {
                name: "Evil Move".into(),
                description: None,
                enabled: true,
                priority: 1,
                source_folders: Vec::new(),
                conditions: ConditionNode::Leaf {
                    condition: Condition::Hidden { is_hidden: false },
                },
                actions: vec![Action::Move {
                    destination: bad_dest.into(),
                }],
                date_source: Default::default(),
                execution_mode: ExecutionMode::FirstMatch,
                origin: RuleOrigin::User,
            });
            let raw = serde_json::to_string(&b).expect("ser");
            assert!(
                preview_config_import(&db, &policy, &raw).await.is_err(),
                "rule destination {bad_dest} must be rejected"
            );
        }

        // Inject path traversal exclusion
        let mut b: ConfigBundle = serde_json::from_str(&base_json).expect("parse");
        b.exclusions.push(crate::config_io::ExportedExclusion {
            folder_path: None,
            path: "/tmp/../etc/passwd".into(),
            exclusion_type: ExclusionKind::Path,
        });
        assert!(
            preview_config_import(&db, &policy, &serde_json::to_string(&b).expect("ser"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn r_io_04_imported_rules_always_forced_disabled_and_origin_import() {
        let (_d, root) = tempdir();
        let policy = test_policy(&root);
        let safe_dest = root.join("home").join("Archive");
        mkdir(&policy.home);
        mkdir(&safe_dest);

        let src_db = crate::database::open_in_memory().await.expect("db");
        CategoryService::new(src_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        let base_json = export_config(&src_db, "0.6.0", 1_000)
            .await
            .expect("export");
        let mut bundle: ConfigBundle = serde_json::from_str(&base_json).expect("parse");
        bundle.rules.push(crate::config_io::ExportedRule {
            name: "Auto Run Rule".into(),
            description: None,
            enabled: true, // incoming claims enabled=true!
            priority: 1,
            source_folders: Vec::new(),
            conditions: ConditionNode::Leaf {
                condition: Condition::Hidden { is_hidden: false },
            },
            actions: vec![Action::Move {
                destination: safe_dest.to_string_lossy().into_owned(),
            }],
            date_source: Default::default(),
            execution_mode: ExecutionMode::FirstMatch,
            origin: RuleOrigin::User,
        });
        let incoming = serde_json::to_string(&bundle).expect("ser");

        let dst_db = crate::database::open_in_memory().await.expect("dst db");
        CategoryService::new(dst_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        let preview = preview_config_import(&dst_db, &policy, &incoming)
            .await
            .expect("preview");
        assert_eq!(preview.forced_disabled_rule_count, 1);

        apply_config_import(&dst_db, &policy, &incoming, ConfigImportMode::Merge, 2_000)
            .await
            .expect("apply");
        let rules = repo_rules::list(&dst_db).await.expect("rules");
        assert_eq!(rules.len(), 1);
        assert!(!rules[0].enabled, "imported rule must be forced disabled");
        assert_eq!(rules[0].origin, RuleOrigin::Import);
    }

    #[tokio::test]
    async fn r_io_05_preview_computes_accurate_diff_without_mutating_db() {
        let (_d, root) = tempdir();
        let policy = test_policy(&root);
        let dl = root.join("home").join("Downloads");
        let dest = root.join("home").join("Sorted");
        mkdir(&policy.home);
        mkdir(&dl);
        mkdir(&dest);

        let src_db = crate::database::open_in_memory().await.expect("src db");
        CategoryService::new(src_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        repo_folders::insert(&src_db, "f1", "Downloads", &dl.to_string_lossy(), true)
            .await
            .expect("folder");
        let src_rules = RuleService::new(src_db.clone(), policy.clone());
        src_rules
            .add_destination_root(&dest)
            .await
            .expect("dest root");
        src_rules
            .create(
                simple_rule("New Backup Rule", &dest.to_string_lossy(), true),
                RuleOrigin::User,
            )
            .await
            .expect("rule");
        let exported = export_config(&src_db, "0.6.0", 1_000)
            .await
            .expect("export");

        let target_db = crate::database::open_in_memory().await.expect("dst db");
        CategoryService::new(target_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");

        let preview = preview_config_import(&target_db, &policy, &exported)
            .await
            .expect("preview");
        assert_eq!(preview.folders_to_add, vec!["Downloads"]);
        assert_eq!(preview.rules_to_add, vec!["New Backup Rule"]);
        assert_eq!(preview.forced_disabled_rule_count, 1);

        // Verify zero mutations occurred in target_db during preview!
        assert!(repo_folders::list(&target_db).await.expect("f").is_empty());
        assert!(repo_rules::list(&target_db).await.expect("r").is_empty());
    }

    #[tokio::test]
    async fn r_io_06_apply_supports_merge_and_replace_transactionally() {
        let (_d, root) = tempdir();
        let policy = test_policy(&root);
        let dest = root.join("home").join("Out");
        mkdir(&policy.home);
        mkdir(&dest);

        let dst_db = crate::database::open_in_memory().await.expect("dst");
        CategoryService::new(dst_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        let rule_svc = RuleService::new(dst_db.clone(), policy.clone());
        rule_svc
            .add_destination_root(&dest)
            .await
            .expect("dst root");
        rule_svc
            .create(
                simple_rule("Existing Rule", &dest.to_string_lossy(), true),
                RuleOrigin::User,
            )
            .await
            .expect("existing");

        let src_db = crate::database::open_in_memory().await.expect("src");
        CategoryService::new(src_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        let src_rules = RuleService::new(src_db.clone(), policy.clone());
        src_rules
            .add_destination_root(&dest)
            .await
            .expect("src root");
        src_rules
            .create(
                simple_rule("Incoming Rule", &dest.to_string_lossy(), true),
                RuleOrigin::User,
            )
            .await
            .expect("incoming");
        let incoming_json = export_config(&src_db, "0.6.0", 1_000)
            .await
            .expect("export");

        // Merge keeps Existing Rule + adds Incoming Rule (disabled)
        apply_config_import(
            &dst_db,
            &policy,
            &incoming_json,
            ConfigImportMode::Merge,
            2_000,
        )
        .await
        .expect("merge");
        let after_merge = repo_rules::list(&dst_db).await.expect("after merge");
        assert_eq!(after_merge.len(), 2);

        // Invalid bundle mid-replace must NOT disturb Existing Rule or Incoming Rule
        assert!(apply_config_import(
            &dst_db,
            &policy,
            "{bad json",
            ConfigImportMode::Replace,
            3_000
        )
        .await
        .is_err());
        assert_eq!(repo_rules::list(&dst_db).await.expect("intact").len(), 2);

        // Replace mode replaces rule set with only Incoming Rule (disabled)
        apply_config_import(
            &dst_db,
            &policy,
            &incoming_json,
            ConfigImportMode::Replace,
            4_000,
        )
        .await
        .expect("replace");
        let after_replace = repo_rules::list(&dst_db).await.expect("after replace");
        assert_eq!(after_replace.len(), 1);
        assert_eq!(after_replace[0].name, "Incoming Rule");
        assert!(!after_replace[0].enabled);
    }

    #[tokio::test]
    async fn bug_031_per_folder_exclusion_never_widens_to_global_and_source_folders_remap() {
        let (_d, root) = tempdir();
        let policy = test_policy(&root);
        let dl = root.join("home").join("Downloads");
        let docs = root.join("home").join("Sorted");
        mkdir(&policy.home);
        mkdir(&dl);
        mkdir(&docs);

        // 1. Export rule scoped to a specific source folder ("f-src"), then import into a fresh DB
        // where Replace/fresh import allocates a brand-new folder UUID.
        let src_db = crate::database::open_in_memory().await.expect("src");
        CategoryService::new(src_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        repo_folders::insert(&src_db, "f-src", "Downloads", &dl.to_string_lossy(), true)
            .await
            .expect("folder");
        let src_rules = RuleService::new(src_db.clone(), policy.clone());
        src_rules
            .add_destination_root(&docs)
            .await
            .expect("dest root");
        let mut scoped_draft = simple_rule("Scoped Rule", &docs.to_string_lossy(), true);
        scoped_draft.source_folders = vec!["f-src".into()];
        src_rules
            .create(scoped_draft, RuleOrigin::User)
            .await
            .expect("scoped rule");

        let exported = export_config(&src_db, "0.6.0", 1_000)
            .await
            .expect("export");

        let dst_db = crate::database::open_in_memory().await.expect("dst");
        CategoryService::new(dst_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        apply_config_import(
            &dst_db,
            &policy,
            &exported,
            ConfigImportMode::Replace,
            2_000,
        )
        .await
        .expect("import");

        let dst_folders = repo_folders::list(&dst_db).await.expect("dst folders");
        let new_folder_id = &dst_folders[0].id;
        assert_ne!(new_folder_id, "f-src");
        let dst_rules = repo_rules::list(&dst_db).await.expect("dst rules");
        assert_eq!(
            dst_rules[0].source_folders,
            vec![new_folder_id.clone()],
            "BUG-031: imported folder-scoped rule must remap source_folders to the target DB folder ID"
        );

        // 2. Per-folder exclusion referencing a missing folder must NEVER silently become global (folder_id = NULL)!
        let mut orphan_excl_bundle: ConfigBundle =
            serde_json::from_str(&exported).expect("parse bundle");
        orphan_excl_bundle
            .exclusions
            .push(crate::config_io::ExportedExclusion {
                folder_path: Some(
                    root.join("home")
                        .join("MissingFolder")
                        .to_string_lossy()
                        .into_owned(),
                ),
                path: "*.tmp".into(),
                exclusion_type: crate::database::repo_exclusions::ExclusionKind::Glob,
            });
        let orphan_json = serde_json::to_string(&orphan_excl_bundle).expect("serialize");
        let orphan_db = crate::database::open_in_memory().await.expect("orphan db");
        CategoryService::new(orphan_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");

        let rep = apply_config_import(
            &orphan_db,
            &policy,
            &orphan_json,
            ConfigImportMode::Merge,
            3_000,
        )
        .await
        .expect("import with orphan folder exclusion");
        assert!(
            repo_exclusions::list(&orphan_db).await.expect("excls").is_empty(),
            "BUG-031: per-folder exclusion with missing folder_path must not be widened to global NULL exclusion"
        );
        assert!(
            !rep.warnings.is_empty(),
            "BUG-031: skipped orphan folder exclusion must emit a warning"
        );
    }

    #[tokio::test]
    async fn bug_031_category_other_protected_builtin_collision_and_bundle_duplicates_rejected() {
        let (_d, root) = tempdir();
        let policy = test_policy(&root);
        mkdir(&policy.home);

        let src_db = crate::database::open_in_memory().await.expect("src");
        CategoryService::new(src_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        let base_json = export_config(&src_db, "0.6.0", 1_000)
            .await
            .expect("export");

        let dst_db = crate::database::open_in_memory().await.expect("dst");
        CategoryService::new(dst_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");

        // 1. Attempting to disable built-in 'other' via import must be rejected or blocked.
        let mut disable_other: ConfigBundle = serde_json::from_str(&base_json).expect("bundle");
        if let Some(other) = disable_other
            .categories
            .iter_mut()
            .find(|c| c.id == "other")
        {
            other.enabled = false;
        }
        let disable_other_json = serde_json::to_string(&disable_other).expect("json");
        assert!(
            apply_config_import(
                &dst_db,
                &policy,
                &disable_other_json,
                ConfigImportMode::Merge,
                2_000
            )
            .await
            .is_err(),
            "BUG-031: bundle disabling built-in 'other' category must be rejected"
        );

        // 2. Custom category with same name as built-in ('Documents') must be rejected during validation (not crash mid-SQL).
        let mut collide_cat: ConfigBundle = serde_json::from_str(&base_json).expect("bundle");
        collide_cat.categories.push(crate::categories::Category {
            id: "custom-dup".into(),
            name: "documents".into(),
            description: None,
            icon: "file".into(),
            color_token: "cat-documents".into(),
            extensions: vec!["foo".into()],
            destination_path: None,
            priority: 15,
            builtin: false,
            enabled: true,
        });
        let collide_json = serde_json::to_string(&collide_cat).expect("json");
        assert!(
            preview_config_import(&dst_db, &policy, &collide_json)
                .await
                .is_err(),
            "BUG-031: custom category colliding with built-in name must be rejected at validation"
        );
    }

    #[tokio::test]
    async fn bug_031_replace_mode_clears_orphan_duplicate_groups() {
        use crate::duplicates::finder::{scan_duplicates, DuplicateScanOptions};
        use crate::engine::classifier::Classifier;
        use crate::settings::MonitoringSettings;

        let (_d, root) = tempdir();
        let policy = test_policy(&root);
        let dl = root.join("home").join("Downloads");
        mkdir(&policy.home);
        mkdir(&dl);
        crate::test_support::write_file(&dl.join("a.bin"), b"dup-bytes");
        crate::test_support::write_file(&dl.join("b.bin"), b"dup-bytes");

        let dst_db = crate::database::open_in_memory().await.expect("dst");
        CategoryService::new(dst_db.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        repo_folders::insert(&dst_db, "f1", "Downloads", &dl.to_string_lossy(), true)
            .await
            .expect("folder");
        let cls = std::sync::Arc::new(Classifier::new(
            CategoryService::new(dst_db.clone())
                .list()
                .await
                .expect("cats"),
        ));
        scan_duplicates(
            &dst_db,
            cls,
            &MonitoringSettings::default(),
            DuplicateScanOptions::default(),
            500,
            None,
        )
        .await
        .expect("scan dups");

        let empty_src = crate::database::open_in_memory().await.expect("src");
        CategoryService::new(empty_src.clone())
            .ensure_builtins()
            .await
            .expect("builtins");
        let empty_json = export_config(&empty_src, "0.6.0", 1_000)
            .await
            .expect("export");

        apply_config_import(
            &dst_db,
            &policy,
            &empty_json,
            ConfigImportMode::Replace,
            2_000,
        )
        .await
        .expect("replace");

        let rem: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM duplicate_groups")
            .fetch_one(&dst_db)
            .await
            .expect("count");
        assert_eq!(
            rem, 0,
            "BUG-031: Replace mode must clear orphan duplicate_groups rows when monitored_folders are replaced"
        );
    }

    #[tokio::test]
    async fn bug_031_preview_replace_mode_and_bundle_duplicate_exclusions_or_rules_rejected() {
        let (_d, root) = tempdir();
        let policy = test_policy(&root);
        let dl = root.join("home").join("Downloads");
        let docs = root.join("home").join("Sorted 🎉 Café");
        mkdir(&policy.home);
        mkdir(&dl);
        mkdir(&docs);

        let src_db = crate::database::open_in_memory().await.expect("src");
        let src_cats = CategoryService::new(src_db.clone());
        src_cats.ensure_builtins().await.expect("builtins");
        src_cats
            .create(CategoryInput {
                name: "Receipts 🎉 \"2026\"".into(),
                description: None,
                icon: "tag".into(),
                color_token: "cat-documents".into(),
                extensions: vec!["ofx".into()],
                destination_path: Some(docs.to_string_lossy().into_owned()),
                priority: 15,
                enabled: true,
            })
            .await
            .expect("cat");
        repo_folders::insert(&src_db, "f1", "Downloads", &dl.to_string_lossy(), true)
            .await
            .expect("folder");
        repo_exclusions::create(
            &src_db,
            ExclusionInput {
                folder_id: None,
                path: "*.part".into(),
                exclusion_type: repo_exclusions::ExclusionKind::Glob,
            },
        )
        .await
        .expect("excl");
        let src_rules = RuleService::new(src_db.clone(), policy.clone());
        src_rules.add_destination_root(&docs).await.expect("dest");
        src_rules
            .create(
                simple_rule("Rule \"Café\" 🎉", &docs.to_string_lossy(), false),
                RuleOrigin::User,
            )
            .await
            .expect("rule");

        let exported = export_config(&src_db, "0.6.0", 1_000)
            .await
            .expect("export");

        // Replace preview against existing identical DB must show Replace warning and count items as additions
        let prev =
            preview_config_import_with_mode(&src_db, &policy, &exported, ConfigImportMode::Replace)
                .await
                .expect("replace preview");
        assert!(prev.warnings.iter().any(|w| w.contains("Replace mode")));
        assert_eq!(prev.folders_to_add, vec!["Downloads".to_owned()]);
        assert!(prev.folders_to_update.is_empty());
        assert_eq!(prev.rules_to_add, vec!["Rule \"Café\" 🎉".to_owned()]);
        assert_eq!(
            prev.categories_to_add,
            vec!["Receipts 🎉 \"2026\"".to_owned()]
        );
        assert_eq!(prev.exclusions_to_add, vec!["*.part".to_owned()]);

        // Duplicate exclusions inside bundle must be rejected
        let mut dup_excl: ConfigBundle = serde_json::from_str(&exported).expect("parse");
        let clone_ex = dup_excl.exclusions[0].clone();
        dup_excl.exclusions.push(clone_ex);
        assert!(
            preview_config_import(
                &src_db,
                &policy,
                &serde_json::to_string(&dup_excl).expect("ser")
            )
            .await
            .is_err(),
            "duplicate exclusion inside bundle must fail validation"
        );

        // Duplicate rule names (case-insensitive) inside bundle must be rejected
        let mut dup_rule: ConfigBundle = serde_json::from_str(&exported).expect("parse");
        let mut second = dup_rule.rules[0].clone();
        second.name = second.name.to_uppercase();
        dup_rule.rules.push(second);
        assert!(
            preview_config_import(
                &src_db,
                &policy,
                &serde_json::to_string(&dup_rule).expect("ser")
            )
            .await
            .is_err(),
            "duplicate rule name inside bundle must fail validation"
        );
    }
}
