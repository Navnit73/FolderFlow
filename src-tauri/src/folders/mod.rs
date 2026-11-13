//! Monitored-folder registry (FR-010, FR-011, FR-002/003).
//!
//! Adding a folder only records it — nothing in the folder is touched.
//! Folders may not overlap (one inside another) so no file is ever owned by
//! two folders' rules.

mod model;

pub use model::*;

use std::path::{Path, PathBuf};

use crate::database::{repo_folders, Db};
use crate::engine::paths::{validate_new_root, PathError, RootPolicy};
use crate::error::{AppError, AppResult};

/// Probes read access without modifying anything (TCC denial → `Denied`).
pub fn probe_access(path: &Path) -> FolderAccess {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return FolderAccess::Missing,
        Err(_) => return FolderAccess::Denied,
        Ok(m) if !m.is_dir() => return FolderAccess::NotAFolder,
        Ok(_) => {}
    }
    match std::fs::read_dir(path) {
        Ok(_) => FolderAccess::Granted,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => FolderAccess::Missing,
        Err(_) => FolderAccess::Denied,
    }
}

/// Lower-cases, strips dots, de-duplicates and validates extensions.
pub fn normalize_extensions(field: &str, raw: &[String]) -> AppResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for r in raw {
        let e = r.trim().trim_start_matches('.').to_lowercase();
        if e.is_empty()
            || e.len() > 20
            || !e
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '!')
        {
            return Err(AppError::validation(
                field,
                format!("\"{r}\" is not a valid extension."),
            ));
        }
        if !out.contains(&e) {
            out.push(e);
        }
    }
    Ok(out)
}

#[derive(Clone)]
pub struct FolderService {
    db: Db,
    policy: RootPolicy,
}

impl FolderService {
    pub fn new(db: Db, policy: RootPolicy) -> Self {
        Self { db, policy }
    }

    pub async fn add(&self, path: &Path, recursive: bool) -> AppResult<MonitoredFolder> {
        let canon = validate_new_root(path, &self.policy)?;
        match probe_access(&canon) {
            FolderAccess::Granted => {}
            FolderAccess::Denied => return Err(PathError::PermissionDenied.into()),
            FolderAccess::Missing => return Err(PathError::NotFound.into()),
            FolderAccess::NotAFolder => return Err(PathError::NotADirectory.into()),
        }
        for existing in self.list().await? {
            let other = PathBuf::from(&existing.path);
            if canon == other {
                return Err(AppError::validation(
                    "path",
                    format!("\"{}\" is already monitored.", existing.name),
                ));
            }
            if canon.starts_with(&other) || other.starts_with(&canon) {
                return Err(AppError::validation(
                    "path",
                    format!("This folder overlaps \"{}\", which is already monitored. Monitored folders can't be inside each other.", existing.name),
                ));
            }
        }
        let id = uuid::Uuid::now_v7().to_string();
        let name = canon
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Folder".into());
        let path_str = canon
            .to_str()
            .ok_or_else(|| AppError::validation("path", "Folder path is not valid UTF-8."))?;
        repo_folders::insert(&self.db, &id, &name, path_str, recursive).await?;
        tracing::info!(target: "folderflow::folders", folder_id = %id, "monitored folder added");
        self.get(&id).await
    }

    pub async fn list(&self) -> AppResult<Vec<MonitoredFolder>> {
        repo_folders::list(&self.db).await
    }

    pub async fn get(&self, id: &str) -> AppResult<MonitoredFolder> {
        repo_folders::get(&self.db, id)
            .await?
            .ok_or_else(|| AppError::NotFound("Folder".into()))
    }

    pub async fn update(&self, id: &str, patch: FolderPatch) -> AppResult<MonitoredFolder> {
        let mut f = self.get(id).await?;
        if let Some(name) = patch.name {
            let name = name.trim();
            if name.is_empty() || name.chars().count() > 80 {
                return Err(AppError::validation(
                    "name",
                    "Name must be 1–80 characters.",
                ));
            }
            f.name = name.to_owned();
        }
        if let Some(enabled) = patch.enabled {
            f.enabled = enabled;
        }
        if let Some(recursive) = patch.recursive {
            f.recursive = recursive;
        }
        if let Some(settings) = patch.settings {
            f.settings = self.normalize_settings(Path::new(&f.path), settings)?;
        }
        let before = self.get(id).await?;
        let scope_changed = before.recursive != f.recursive
            || before.settings.include_extensions != f.settings.include_extensions
            || before.settings.exclude_extensions != f.settings.exclude_extensions
            || before.settings.excluded_subfolders != f.settings.excluded_subfolders;
        repo_folders::update(&self.db, &f).await?;
        if scope_changed {
            // Files that enter scope already existed; they must not be reported as new (BUG-009).
            repo_folders::reset_scan(&self.db, id).await?;
        }
        self.get(id).await
    }

    fn normalize_settings(&self, root: &Path, s: FolderSettings) -> AppResult<FolderSettings> {
        let mut excluded = Vec::new();
        for raw in &s.excluded_subfolders {
            let bad = || {
                AppError::validation(
                    "excludedSubfolders",
                    format!("\"{raw}\" must be an existing folder inside this folder."),
                )
            };
            let p = Path::new(raw);
            if !p.is_absolute() {
                return Err(bad());
            }
            let canon = p.canonicalize().map_err(|_| bad())?;
            if !canon.is_dir() || canon == root || !canon.starts_with(root) {
                return Err(bad());
            }
            let c = canon.to_string_lossy().into_owned();
            if !excluded.contains(&c) {
                excluded.push(c);
            }
        }
        if let Some(i) = s.stability_interval_secs {
            if !(1..=300).contains(&i) {
                return Err(AppError::validation(
                    "stabilityIntervalSecs",
                    "Must be between 1 and 300 seconds.",
                ));
            }
        }
        Ok(FolderSettings {
            include_extensions: normalize_extensions("includeExtensions", &s.include_extensions)?,
            exclude_extensions: normalize_extensions("excludeExtensions", &s.exclude_extensions)?,
            excluded_subfolders: excluded,
            stability_interval_secs: s.stability_interval_secs,
        })
    }

    /// Stops tracking a folder. Never touches the files themselves.
    pub async fn remove(&self, id: &str) -> AppResult<()> {
        if !repo_folders::delete(&self.db, id).await? {
            return Err(AppError::NotFound("Folder".into()));
        }
        tracing::info!(target: "folderflow::folders", folder_id = %id, "monitored folder removed");
        Ok(())
    }

    pub async fn set_health(
        &self,
        id: &str,
        health: FolderHealth,
        error: Option<&str>,
    ) -> AppResult<()> {
        if !repo_folders::set_health(&self.db, id, health, error).await? {
            return Err(AppError::NotFound("Folder".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::error::{AppError, ErrorCode, UiError};
    use crate::test_support::{mkdir, tempdir, write_file};
    use std::path::PathBuf;

    struct Env {
        _tmp: tempfile::TempDir,
        base: PathBuf,
        svc: FolderService,
        db: Db,
    }

    async fn env() -> Env {
        let (tmp, base) = tempdir();
        let home = base.join("home");
        mkdir(&home.join("Library/Application Support/com.folderflow.desktop"));
        let db = database::open_in_memory().await.expect("db");
        let policy = RootPolicy {
            home: home.clone(),
            app_data_dir: home.join("Library/Application Support/com.folderflow.desktop"),
        };
        Env {
            _tmp: tmp,
            base,
            svc: FolderService::new(db.clone(), policy),
            db,
        }
    }

    fn chmod(p: &Path, mode: u32) {
        crate::test_support::set_mode(p, mode);
    }

    #[tokio::test]
    async fn r_fold_01_add_lists_with_defaults_and_rejects_duplicates_and_overlap() {
        let e = env().await;
        let dl = e.base.join("home/Downloads");
        mkdir(&dl.join("Sub"));
        let f = e.svc.add(&dl, true).await.expect("add");
        assert_eq!(
            (f.name.as_str(), f.path.as_str(), f.enabled, f.recursive),
            ("Downloads", dl.to_str().expect("utf8"), true, true)
        );
        assert_eq!(
            (f.health, f.file_count, f.last_scanned_at),
            (FolderHealth::Unknown, 0, None)
        );
        assert_eq!(e.svc.list().await.expect("list"), vec![f.clone()]);

        assert!(
            matches!(
                e.svc.add(&dl, false).await,
                Err(AppError::Validation { .. })
            ),
            "duplicate"
        );
        assert!(
            matches!(
                e.svc.add(&dl.join("Sub"), false).await,
                Err(AppError::Validation { .. })
            ),
            "inside existing"
        );
        let docs = e.base.join("home/Documents");
        mkdir(&docs);
        e.svc.add(&docs, false).await.expect("sibling ok");
        assert!(e
            .svc
            .add(&e.base.join("home/Documents/../Documents"), false)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn r_fold_02_forbidden_missing_and_file_paths_are_rejected_with_ui_codes() {
        let e = env().await;
        let ui = |err: AppError| UiError::from(err).code;
        assert_eq!(
            ui(e.svc
                .add(&e.base.join("home"), true)
                .await
                .expect_err("home")),
            ErrorCode::Validation
        );
        assert_eq!(
            ui(e.svc
                .add(Path::new("/System"), true)
                .await
                .expect_err("system")),
            ErrorCode::Validation
        );
        assert_eq!(
            ui(e.svc
                .add(&e.base.join("nope"), true)
                .await
                .expect_err("missing")),
            ErrorCode::NotFound
        );
        write_file(&e.base.join("file.txt"), b"x");
        assert_eq!(
            ui(e.svc
                .add(&e.base.join("file.txt"), true)
                .await
                .expect_err("file")),
            ErrorCode::Validation
        );
    }

    #[tokio::test]
    async fn r_fold_03_update_normalises_settings_and_validates_subfolders() {
        let e = env().await;
        let dl = e.base.join("Downloads");
        mkdir(&dl.join("Keep"));
        let f = e.svc.add(&dl, false).await.expect("add");
        let patch = FolderPatch {
            name: Some("  My Downloads ".into()),
            recursive: Some(true),
            settings: Some(FolderSettings {
                include_extensions: vec![],
                exclude_extensions: vec![".DMG".into(), "dmg".into()],
                excluded_subfolders: vec![dl.join("Keep").to_string_lossy().into_owned()],
                stability_interval_secs: Some(5),
            }),
            enabled: None,
        };
        let u = e.svc.update(&f.id, patch).await.expect("update");
        assert_eq!(
            (u.name.as_str(), u.recursive, u.enabled),
            ("My Downloads", true, true)
        );
        assert_eq!(u.settings.exclude_extensions, vec!["dmg"]);

        let outside = e.base.join("Elsewhere");
        mkdir(&outside);
        for bad in [
            outside.to_string_lossy().into_owned(),
            dl.join("missing").to_string_lossy().into_owned(),
            dl.to_string_lossy().into_owned(),
        ] {
            let p = FolderPatch {
                settings: Some(FolderSettings {
                    excluded_subfolders: vec![bad.clone()],
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert!(
                matches!(
                    e.svc.update(&f.id, p).await,
                    Err(AppError::Validation { .. })
                ),
                "{bad}"
            );
        }
        let bad_interval = FolderPatch {
            settings: Some(FolderSettings {
                stability_interval_secs: Some(0),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(e.svc.update(&f.id, bad_interval).await.is_err());
        assert!(matches!(
            e.svc
                .update(
                    &f.id,
                    FolderPatch {
                        name: Some(" ".into()),
                        ..Default::default()
                    }
                )
                .await,
            Err(AppError::Validation { .. })
        ));
        assert!(matches!(
            e.svc.update("nope", FolderPatch::default()).await,
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn r_fold_04_remove_cascades_records_and_never_touches_files() {
        let e = env().await;
        let dl = e.base.join("Downloads");
        write_file(&dl.join("a.pdf"), b"%PDF");
        let f = e.svc.add(&dl, false).await.expect("add");
        sqlx::query("INSERT INTO file_records (id, folder_id, path, filename, size, dev, ino, modified_at_ns, last_scanned_at) VALUES ('r1', ?1, ?2, 'a.pdf', 4, 1, 1, 0, 0)")
            .bind(&f.id).bind(dl.join("a.pdf").to_string_lossy().into_owned()).execute(&e.db).await.expect("insert");
        e.svc.remove(&f.id).await.expect("remove");
        assert!(e.svc.list().await.expect("list").is_empty());
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM file_records")
            .fetch_one(&e.db)
            .await
            .expect("count");
        assert_eq!(n, 0);
        assert!(
            dl.join("a.pdf").exists(),
            "removing a folder must not touch files"
        );
        assert!(matches!(
            e.svc.remove(&f.id).await,
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn r_fold_05_health_is_persisted() {
        let e = env().await;
        let dl = e.base.join("Downloads");
        mkdir(&dl);
        let f = e.svc.add(&dl, false).await.expect("add");
        e.svc
            .set_health(&f.id, FolderHealth::Unavailable, Some("Drive disconnected"))
            .await
            .expect("health");
        let g = e.svc.get(&f.id).await.expect("get");
        assert_eq!(
            (g.health, g.last_error.as_deref()),
            (FolderHealth::Unavailable, Some("Drive disconnected"))
        );
    }

    /// BUG-009: widening or narrowing a folder's scope must reset the scan
    /// baseline, so existing files in the new scope are not reported as new.
    #[tokio::test]
    async fn bug_009_scope_changes_reset_the_scan_baseline() {
        let e = env().await;
        let dl = e.base.join("Downloads");
        mkdir(&dl.join("Sub"));
        let f = e.svc.add(&dl, false).await.expect("add");
        crate::database::repo_folders::record_scan(&e.db, &f.id, 1234)
            .await
            .expect("scan");

        let renamed = e
            .svc
            .update(
                &f.id,
                FolderPatch {
                    name: Some("Inbox".into()),
                    ..Default::default()
                },
            )
            .await
            .expect("rename");
        assert_eq!(
            renamed.last_scanned_at,
            Some(1234),
            "cosmetic edits keep the baseline"
        );

        let widened = e
            .svc
            .update(
                &f.id,
                FolderPatch {
                    recursive: Some(true),
                    ..Default::default()
                },
            )
            .await
            .expect("recursive");
        assert_eq!(widened.last_scanned_at, None);

        crate::database::repo_folders::record_scan(&e.db, &f.id, 5678)
            .await
            .expect("scan");
        let same = e
            .svc
            .update(
                &f.id,
                FolderPatch {
                    recursive: Some(true),
                    ..Default::default()
                },
            )
            .await
            .expect("same");
        assert_eq!(
            same.last_scanned_at,
            Some(5678),
            "unchanged scope keeps the baseline"
        );

        let excl = FolderSettings {
            excluded_subfolders: vec![dl.join("Sub").to_string_lossy().into_owned()],
            ..Default::default()
        };
        let narrowed = e
            .svc
            .update(
                &f.id,
                FolderPatch {
                    settings: Some(excl),
                    ..Default::default()
                },
            )
            .await
            .expect("exclude");
        assert_eq!(narrowed.last_scanned_at, None);
    }

    #[tokio::test]
    async fn r_perm_01_unreadable_folder_is_rejected_as_permission_denied() {
        let e = env().await;
        let locked = e.base.join("Locked");
        mkdir(&locked);
        chmod(&locked, 0o000);
        let err = e.svc.add(&locked, false).await.expect_err("locked");
        chmod(&locked, 0o755);
        let ui = UiError::from(err);
        assert_eq!(ui.code, ErrorCode::PermissionDenied);
        assert!(ui.hint.unwrap_or_default().contains("System Settings"));
    }

    #[test]
    fn r_perm_02_probe_access() {
        let (_d, base) = tempdir();
        mkdir(&base.join("ok"));
        mkdir(&base.join("locked"));
        write_file(&base.join("f.txt"), b"x");
        chmod(&base.join("locked"), 0o000);
        assert_eq!(probe_access(&base.join("ok")), FolderAccess::Granted);
        assert_eq!(probe_access(&base.join("locked")), FolderAccess::Denied);
        assert_eq!(probe_access(&base.join("gone")), FolderAccess::Missing);
        assert_eq!(probe_access(&base.join("f.txt")), FolderAccess::NotAFolder);
        chmod(&base.join("locked"), 0o755);
    }
}
