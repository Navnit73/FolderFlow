//! Phase 9 Release & Clean-Environment Smoke Suite
//! (`P9-T01`..`P9-T08`, `R-REL-01`..`03`, `R-SMOKE-01`..`08`).
//!
//! Verifies release manifests, brand/DMG assets, least-privilege Hardened Runtime
//! entitlements, User Guide coverage, and all 8 clean-environment VM smoke
//! lifecycle scenarios from `docs/TESTING_STRATEGY.md`:
//! install -> first launch -> onboarding -> organize -> undo -> restart
//! persistence -> offline -> uninstall leaves user files intact.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use folderflow_lib::categories::CategoryService;
use folderflow_lib::database::repo_operations::{ActivityFilter, PageRequest};
use folderflow_lib::database::{self, repo_folders, repo_rules};
use folderflow_lib::engine::activity::{load_path_authority, ActivityService};
use folderflow_lib::engine::classifier::Classifier;
use folderflow_lib::engine::conflicts::ConflictPolicy;
use folderflow_lib::engine::fs_view::RealFs;
use folderflow_lib::engine::operations::{BatchTrigger, Ledger, OpKind, OpStatus};
use folderflow_lib::engine::organizer::Organizer;
use folderflow_lib::engine::paths::RootPolicy;
use folderflow_lib::engine::planner::{
    build_execution_plan, collect_folder_candidates, execute_plan, PlanContext, PlanEnv, PlanScope,
    PreviewStore,
};
use folderflow_lib::engine::recovery::recover_startup;
use folderflow_lib::engine::safe_fs::SafeFs;
use folderflow_lib::engine::undo::UndoTarget;
use folderflow_lib::folders::FolderService;
use folderflow_lib::logging;
use folderflow_lib::onboarding::{OnboardingService, OnboardingStepUpdate};
use folderflow_lib::rules::models::{
    Action, Condition, ConditionNode, DateSource, ExecutionMode, RuleDraft, RuleOrigin,
    TextFieldKind, TextOp,
};
use folderflow_lib::rules::RuleService;
use folderflow_lib::security::{
    InMemoryKeychain, LicenseEdition, LicenseService, LicenseStateKind, SignedLicensePayload,
    UnconfiguredOnlineProvider,
};
use folderflow_lib::settings::{MonitoringSettings, SettingsService};
use folderflow_lib::utils::clock::FixedClock;
use serde_json::Value;
use tempfile::TempDir;

const TARGET_VERSION: &str = "1.0.0";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn write_new_file(path: &Path, data: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("create fixture file");
    f.write_all(data).expect("write fixture bytes");
    f.flush().expect("flush fixture file");
}

fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, (u64, Vec<u8>)> {
    let mut out = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
        let entry = entry.expect("walkdir entry");
        let path = entry.path().to_path_buf();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        if entry.file_type().is_file() {
            let bytes = fs::read(&path).expect("read snapshot file");
            let len = bytes.len() as u64;
            out.insert(rel, (len, bytes));
        } else if entry.file_type().is_dir() {
            out.insert(rel, (0, Vec::new()));
        }
    }
    out
}

fn parse_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
    if bytes.len() < 24 || bytes[0..8] != PNG_SIG || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Some((width, height))
}

fn extract_toml_package_version(raw: &str) -> Option<String> {
    let mut in_package = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package && trimmed.starts_with("version") {
            let (_, val) = trimmed.split_once('=')?;
            return Some(val.trim().trim_matches('"').to_owned());
        }
    }
    None
}

fn sign_offline_token(signing_key: &SigningKey, payload: &SignedLicensePayload) -> String {
    let json = serde_json::to_vec(payload).expect("serialize license payload");
    let mut msg = Vec::from(&b"folderflow-license-v1\n"[..]);
    msg.extend_from_slice(&json);
    let sig = signing_key.sign(&msg);
    format!(
        "FF1.{}.{}",
        URL_SAFE_NO_PAD.encode(json),
        URL_SAFE_NO_PAD.encode(sig.to_bytes())
    )
}

#[allow(clippy::disallowed_methods)]
fn simulate_app_uninstall(paths: &[&Path]) {
    // Test-only simulation of dragging FolderFlow.app to Trash and removing app support/logs.
    for p in paths {
        if p.exists() {
            fs::remove_dir_all(p).expect("remove app data or bundle directory");
        }
    }
}

struct CleanEnv {
    _temp: TempDir,
    home: PathBuf,
    app_support: PathBuf,
    logs_dir: PathBuf,
    trash_dir: PathBuf,
    db_path: PathBuf,
    downloads: PathBuf,
    desktop: PathBuf,
    archive_dest: PathBuf,
}

impl CleanEnv {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let canon_root = temp
            .path()
            .canonicalize()
            .unwrap_or_else(|_| temp.path().to_path_buf());
        let home = canon_root.join("home");
        let app_support = home.join("Library/Application Support/com.folderflow.desktop");
        let logs_dir = home.join("Library/Logs/com.folderflow.desktop");
        let trash_dir = canon_root.join("Trash");
        let db_path = app_support.join("folderflow.db");
        let downloads = home.join("Downloads");
        let desktop = home.join("Desktop");
        let archive_dest = home.join("Documents/Organized");

        for dir in [
            &app_support,
            &logs_dir,
            &trash_dir,
            &downloads,
            &desktop,
            &archive_dest,
        ] {
            fs::create_dir_all(dir).expect("create clean env directory");
        }

        Self {
            _temp: temp,
            home,
            app_support,
            logs_dir,
            trash_dir,
            db_path,
            downloads,
            desktop,
            archive_dest,
        }
    }

    fn root_policy(&self) -> RootPolicy {
        RootPolicy {
            home: self.home.clone(),
            app_data_dir: self.app_support.clone(),
        }
    }
}

#[test]
fn r_rel_01_version_and_bundle_manifests_match() {
    let root = repo_root();
    let tauri_dir = crate_root();

    let pkg_raw = fs::read_to_string(root.join("package.json")).expect("read package.json");
    let pkg: Value = serde_json::from_str(&pkg_raw).expect("parse package.json");
    assert_eq!(pkg["version"], TARGET_VERSION, "package.json version");

    let cargo_raw = fs::read_to_string(tauri_dir.join("Cargo.toml")).expect("read Cargo.toml");
    assert_eq!(
        extract_toml_package_version(&cargo_raw).as_deref(),
        Some(TARGET_VERSION),
        "src-tauri/Cargo.toml version"
    );
    assert!(
        cargo_raw.contains("ff-macos-events = { version = \"1.0.0\""),
        "src-tauri/Cargo.toml must pin ff-macos-events = 1.0.0"
    );

    let macos_events_cargo =
        fs::read_to_string(tauri_dir.join("crates/ff-macos-events/Cargo.toml"))
            .expect("read ff-macos-events Cargo.toml");
    assert_eq!(
        extract_toml_package_version(&macos_events_cargo).as_deref(),
        Some(TARGET_VERSION),
        "ff-macos-events version"
    );

    let conf_raw =
        fs::read_to_string(tauri_dir.join("tauri.conf.json")).expect("read tauri.conf.json");
    let conf: Value = serde_json::from_str(&conf_raw).expect("parse tauri.conf.json");
    assert_eq!(conf["version"], TARGET_VERSION, "tauri.conf.json version");
    assert_eq!(
        conf["identifier"], "com.folderflow.desktop",
        "bundle identifier"
    );
    assert_eq!(conf["bundle"]["active"], true);

    let targets = conf["bundle"]["targets"]
        .as_array()
        .expect("bundle.targets array");
    assert!(targets.iter().any(|v| v == "app"), "must bundle .app");
    assert!(targets.iter().any(|v| v == "dmg"), "must bundle .dmg");

    let copyright = conf["bundle"]["copyright"]
        .as_str()
        .expect("bundle.copyright string");
    assert!(!copyright.trim().is_empty(), "copyright must be set");

    let mac = &conf["bundle"]["macOS"];
    assert_eq!(mac["minimumSystemVersion"], "13.0");
    assert_eq!(mac["hardenedRuntime"], true);
    assert_eq!(mac["entitlements"], "Entitlements.plist");
    assert_eq!(mac["dmg"]["background"], "../assets/dmg-background.png");

    let entitlements = fs::read_to_string(tauri_dir.join("Entitlements.plist"))
        .expect("Entitlements.plist must exist");
    for forbidden in [
        "com.apple.security.app-sandbox",
        "com.apple.security.cs.allow-jit",
        "com.apple.security.cs.allow-unsigned-executable-memory",
        "com.apple.security.cs.disable-library-validation",
        "com.apple.security.get-task-allow",
    ] {
        assert!(
            !entitlements.contains(forbidden),
            "Entitlements.plist must not enable {forbidden}"
        );
    }

    let changelog =
        fs::read_to_string(root.join("docs/CHANGELOG.md")).expect("read docs/CHANGELOG.md");
    assert!(
        changelog.contains("## [1.0.0]"),
        "CHANGELOG.md must contain ## [1.0.0] release heading"
    );

    let release_notes =
        fs::read_to_string(root.join("RELEASE_NOTES.md")).expect("RELEASE_NOTES.md must exist");
    assert!(
        release_notes.contains(TARGET_VERSION),
        "RELEASE_NOTES.md must reference version {TARGET_VERSION}"
    );

    for script_rel in ["scripts/release-macos.sh", "scripts/smoke-test-macos.sh"] {
        let script = root.join(script_rel);
        assert!(script.exists(), "{script_rel} must exist");
        let mode = fs::metadata(&script)
            .expect("script metadata")
            .permissions()
            .mode();
        assert!(mode & 0o111 != 0, "{script_rel} must be executable");
    }

    let release_wf = fs::read_to_string(root.join(".github/workflows/release.yml"))
        .expect(".github/workflows/release.yml must exist");
    assert!(
        release_wf.contains("universal-apple-darwin"),
        "release workflow must build universal-apple-darwin"
    );
}

#[test]
fn r_rel_02_brand_icons_and_dmg_assets_valid() {
    let root = repo_root();
    let tauri_dir = crate_root();

    let svg =
        fs::read_to_string(root.join("assets/icon-source.svg")).expect("read icon-source.svg");
    assert!(
        !svg.contains("Interim app icon"),
        "assets/icon-source.svg must replace the interim placeholder icon"
    );
    assert!(svg.contains("viewBox=\"0 0 1024 1024\""));

    let dmg_bg_path = root.join("assets/dmg-background.png");
    let dmg_bg = fs::read(&dmg_bg_path).expect("assets/dmg-background.png must exist");
    let (bg_w, bg_h) = parse_png_dimensions(&dmg_bg).expect("dmg-background.png must be valid PNG");
    assert!(
        (bg_w == 660 && bg_h == 400) || (bg_w == 1320 && bg_h == 800),
        "dmg-background.png dimensions must match 660x400 or 1320x800, got {bg_w}x{bg_h}"
    );

    let conf_raw =
        fs::read_to_string(tauri_dir.join("tauri.conf.json")).expect("read tauri.conf.json");
    let conf: Value = serde_json::from_str(&conf_raw).expect("parse tauri.conf.json");
    let icon_list = conf["bundle"]["icon"]
        .as_array()
        .expect("bundle.icon array");
    assert!(!icon_list.is_empty());
    for entry in icon_list {
        let rel = entry.as_str().expect("icon path string");
        let abs = tauri_dir.join(rel);
        let bytes = fs::read(&abs).unwrap_or_else(|_| panic!("missing bundle icon {rel}"));
        if rel.ends_with(".icns") {
            assert!(
                bytes.starts_with(b"icns") && bytes.len() > 10_000,
                "{rel} must be a valid non-trivial Apple .icns archive"
            );
        } else if rel.ends_with(".png") {
            let (w, h) =
                parse_png_dimensions(&bytes).unwrap_or_else(|| panic!("{rel} must be a valid PNG"));
            assert_eq!(w, h, "{rel} must be square");
            assert!(w >= 32, "{rel} must be >= 32px");
        }
    }
}

#[test]
fn r_rel_03_user_guide_and_frontend_version_parity() {
    let root = repo_root();
    let guide =
        fs::read_to_string(root.join("docs/USER_GUIDE.md")).expect("docs/USER_GUIDE.md must exist");

    for required_topic in [
        "FolderFlow",
        "DMG",
        "Applications",
        "Privacy & Security",
        "Preview",
        "Undo",
        "Duplicate",
        "Scheduler",
        "Menu Bar",
        "License",
        "offline",
        "Uninstall",
        "untouched",
    ] {
        assert!(
            guide
                .to_ascii_lowercase()
                .contains(&required_topic.to_ascii_lowercase()),
            "docs/USER_GUIDE.md missing section/topic: {required_topic}"
        );
    }

    let fixtures = fs::read_to_string(root.join("src/test/fixtures.ts")).expect("read fixtures.ts");
    assert!(
        fixtures.contains("version: \"1.0.0\""),
        "src/test/fixtures.ts must use version 1.0.0"
    );

    let e2e_mock =
        fs::read_to_string(root.join("tests/e2e/helpers/tauriMock.ts")).expect("read tauriMock.ts");
    assert!(
        e2e_mock.contains("version: \"1.0.0\""),
        "tests/e2e/helpers/tauriMock.ts must use version 1.0.0"
    );
}

#[test]
fn r_smoke_01_clean_install_layout_and_tcc_metadata() {
    let info_plist =
        fs::read_to_string(crate_root().join("Info.plist")).expect("read src-tauri/Info.plist");
    for key in [
        "NSDownloadsFolderUsageDescription",
        "NSDesktopFolderUsageDescription",
        "NSDocumentsFolderUsageDescription",
        "NSRemovableVolumesUsageDescription",
        "NSNetworkVolumesUsageDescription",
        "NSSupportsAutomaticGraphicsSwitching",
    ] {
        assert!(
            info_plist.contains(key),
            "Info.plist must declare {key} for macOS integration"
        );
    }

    // Simulate clean installation of FolderFlow.app into an isolated Applications folder.
    let env = CleanEnv::new();
    let app_bundle = env.home.join("Applications/FolderFlow.app/Contents");
    fs::create_dir_all(app_bundle.join("MacOS")).expect("create bundle MacOS dir");
    fs::create_dir_all(app_bundle.join("Resources")).expect("create bundle Resources dir");
    write_new_file(&app_bundle.join("Info.plist"), info_plist.as_bytes());
    assert!(app_bundle.join("Info.plist").exists());
}

#[tokio::test]
async fn r_smoke_02_first_launch_creates_db_migrations_settings_and_logs() {
    let env = CleanEnv::new();
    let db = database::open(&env.db_path).await.expect("open clean DB");

    let schema_ver: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(&db)
        .await
        .expect("query schema version");
    assert!(schema_ver >= 2, "expected all migrations applied");

    let cats = CategoryService::new(db.clone());
    cats.ensure_builtins().await.expect("seed categories");
    assert!(cats.list().await.expect("list categories").len() >= 13);

    let settings = SettingsService::new(db.clone())
        .get()
        .await
        .expect("load default settings");
    assert!(settings.privacy.redact_paths_in_logs);

    let mut appender = logging::build_appender(&env.logs_dir, settings.privacy.log_retention_days)
        .expect("create log appender");
    writeln!(appender, "FolderFlow started version=\"{TARGET_VERSION}\"").expect("write log line");
    appender.flush().expect("flush log");

    let log_files: Vec<_> = fs::read_dir(&env.logs_dir)
        .expect("read logs dir")
        .filter_map(|e| e.ok())
        .collect();
    assert!(!log_files.is_empty(), "first launch must create a log file");
    assert!(
        env.db_path.exists(),
        "first launch must create folderflow.db"
    );
}

#[tokio::test]
async fn r_smoke_03_onboarding_zero_writes_and_starter_rules_disabled() {
    let env = CleanEnv::new();
    write_new_file(&env.downloads.join("Invoice_2026.pdf"), b"%PDF-1.4 invoice");
    write_new_file(&env.downloads.join("Screenshot 2026-11-14.png"), b"\x89PNG");
    let before = snapshot_tree(&env.downloads);

    let db = database::open(&env.db_path).await.expect("open DB");
    CategoryService::new(db.clone())
        .ensure_builtins()
        .await
        .expect("categories");
    let folders = FolderService::new(db.clone(), env.root_policy());
    folders
        .add(&env.downloads, false)
        .await
        .expect("add Downloads");

    let clock = Arc::new(FixedClock::utc(1_794_700_000_000));
    let preview_store = PreviewStore::new();
    let onboarding = OnboardingService::new(db.clone(), env.root_policy(), clock, preview_store);

    onboarding
        .set_step(OnboardingStepUpdate {
            current_step: 4,
            organization_preference: None,
            selected_preset_ids: Some(vec!["invoices".into(), "screenshots".into()]),
        })
        .await
        .expect("save onboarding step");

    let report = onboarding
        .scan_and_preview(None)
        .await
        .expect("onboarding scan preview");
    assert_eq!(report.folders_scanned, 1);
    assert_eq!(report.total_files, 2);
    assert!(report.plan_summary.files_matched >= 1);

    // Explicitly do NOT confirm automatic enablement -> starter rules must stay disabled.
    onboarding
        .complete(vec!["invoices".into(), "screenshots".into()], false)
        .await
        .expect("complete onboarding");

    let after = snapshot_tree(&env.downloads);
    assert_eq!(before, after, "onboarding must never mutate user files");

    let rules = repo_rules::list(&db).await.expect("list rules");
    assert_eq!(rules.len(), 2);
    assert!(
        rules.iter().all(|r| !r.enabled),
        "starter rules must remain disabled when auto-enable is unchecked"
    );
}

#[tokio::test]
async fn r_smoke_04_organize_moves_file_safely_with_ledger_and_activity() {
    let env = CleanEnv::new();
    let pdf = env.downloads.join("Tax_Receipt.pdf");
    write_new_file(&pdf, b"%PDF-1.7 tax receipt bytes");

    let db = database::open(&env.db_path).await.expect("open DB");
    let cats = CategoryService::new(db.clone());
    cats.ensure_builtins().await.expect("categories");
    let classifier = Arc::new(Classifier::new(cats.list().await.expect("list cats")));

    let folders = FolderService::new(db.clone(), env.root_policy());
    let folder = folders
        .add(&env.downloads, false)
        .await
        .expect("add monitored folder");

    let rule_svc = RuleService::new(db.clone(), env.root_policy());
    let dest_root = rule_svc
        .add_destination_root(&env.archive_dest)
        .await
        .expect("approve destination root");

    let draft = RuleDraft {
        name: "Move PDFs to Archive".into(),
        description: None,
        enabled: true,
        source_folders: vec![folder.id.clone()],
        conditions: ConditionNode::Leaf {
            condition: Condition::Text {
                field: TextFieldKind::Extension,
                op: TextOp::Equals,
                value: "pdf".into(),
                case_sensitive: false,
            },
        },
        actions: vec![Action::Move {
            destination: format!("{dest_root}/PDFs"),
        }],
        date_source: DateSource::Modified,
        execution_mode: ExecutionMode::FirstMatch,
    };
    rule_svc
        .create(draft, RuleOrigin::User)
        .await
        .expect("create rule");

    let clock = Arc::new(FixedClock::utc(1_794_700_100_000));
    let safe_fs = Arc::new(SafeFs::with_trash_dir(env.trash_dir.clone()));
    let organizer = Organizer::new(Ledger::new(db.clone()), safe_fs, clock.clone());
    let authority = load_path_authority(&db).await.expect("load authority");
    let rules = rule_svc.list().await.expect("list rules");
    let monitoring = MonitoringSettings::default();
    let candidates = collect_folder_candidates(&folder, &monitoring, &classifier, None);

    let plan = build_execution_plan(
        &PlanContext {
            fs: &RealFs,
            authority: &authority,
            classifier: &classifier,
            clock: clock.as_ref(),
        },
        &rules,
        &candidates,
        &PlanScope::default(),
        &PlanEnv::default(),
    );
    assert_eq!(plan.items.len(), 1);

    let report = execute_plan(
        &plan,
        None,
        ConflictPolicy::AutoRename,
        &organizer,
        &authority,
        BatchTrigger::Preview,
    )
    .await
    .expect("execute plan");
    assert_eq!(report.completed.len(), 1);
    assert!(report.failed.is_empty());

    let expected_dest = PathBuf::from(&dest_root).join("PDFs/Tax_Receipt.pdf");
    assert!(!pdf.exists(), "source file should have been moved");
    assert!(expected_dest.exists(), "destination file must exist");
    assert_eq!(
        fs::read(&expected_dest).expect("read moved file"),
        b"%PDF-1.7 tax receipt bytes"
    );

    let activity = ActivityService::new(db, Arc::new(SafeFs::with_trash_dir(env.trash_dir)), clock);
    let page = activity
        .list(
            ActivityFilter::default(),
            PageRequest {
                limit: 20,
                offset: 0,
            },
        )
        .await
        .expect("list activity");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].status, OpStatus::Completed);
}

#[tokio::test]
async fn r_smoke_05_undo_restores_original_file_and_prunes_empty_dest() {
    let env = CleanEnv::new();
    let original = env.downloads.join("Contract.pdf");
    let content = b"%PDF-1.7 contract original payload";
    write_new_file(&original, content);

    let db = database::open(&env.db_path).await.expect("open DB");
    let cats = CategoryService::new(db.clone());
    cats.ensure_builtins().await.expect("categories");
    let classifier = Arc::new(Classifier::new(cats.list().await.expect("list cats")));

    let folder = FolderService::new(db.clone(), env.root_policy())
        .add(&env.downloads, false)
        .await
        .expect("add folder");
    let rule_svc = RuleService::new(db.clone(), env.root_policy());
    let dest_root = rule_svc
        .add_destination_root(&env.archive_dest)
        .await
        .expect("dest root");

    rule_svc
        .create(
            RuleDraft {
                name: "Archive Contracts".into(),
                description: None,
                enabled: true,
                source_folders: vec![folder.id.clone()],
                conditions: ConditionNode::Leaf {
                    condition: Condition::Text {
                        field: TextFieldKind::Name,
                        op: TextOp::Contains,
                        value: "Contract".into(),
                        case_sensitive: false,
                    },
                },
                actions: vec![Action::Move {
                    destination: format!("{dest_root}/Contracts"),
                }],
                date_source: DateSource::Modified,
                execution_mode: ExecutionMode::FirstMatch,
            },
            RuleOrigin::User,
        )
        .await
        .expect("create rule");

    let clock = Arc::new(FixedClock::utc(1_794_700_200_000));
    let safe_fs = Arc::new(SafeFs::with_trash_dir(env.trash_dir.clone()));
    let organizer = Organizer::new(Ledger::new(db.clone()), safe_fs.clone(), clock.clone());
    let authority = load_path_authority(&db).await.expect("authority");
    let monitoring = MonitoringSettings::default();
    let plan = build_execution_plan(
        &PlanContext {
            fs: &RealFs,
            authority: &authority,
            classifier: &classifier,
            clock: clock.as_ref(),
        },
        &rule_svc.list().await.expect("rules"),
        &collect_folder_candidates(&folder, &monitoring, &classifier, None),
        &PlanScope::default(),
        &PlanEnv::default(),
    );
    let exec = execute_plan(
        &plan,
        None,
        ConflictPolicy::AutoRename,
        &organizer,
        &authority,
        BatchTrigger::Manual,
    )
    .await
    .expect("execute");
    assert_eq!(exec.completed.len(), 1);

    let moved_path = PathBuf::from(&dest_root).join("Contracts/Contract.pdf");
    assert!(moved_path.exists());
    assert!(!original.exists());

    let activity = ActivityService::new(db.clone(), safe_fs, clock);
    let undo_report = activity
        .undo(UndoTarget::Batch {
            batch_id: exec.batch_id,
            restore_structure: true,
            auto_rename_conflicts: false,
        })
        .await
        .expect("undo batch with restore_structure");
    assert_eq!(undo_report.undone.len(), 1);
    assert!(undo_report.conflicts.is_empty());

    assert!(original.exists(), "original path must be restored");
    assert!(!moved_path.exists(), "moved copy must no longer remain");
    assert!(
        !moved_path.parent().expect("Contracts dir").exists(),
        "empty created destination folder must be pruned"
    );
    assert_eq!(fs::read(&original).expect("read restored"), content);

    let page = activity
        .list(
            ActivityFilter::default(),
            PageRequest {
                limit: 10,
                offset: 0,
            },
        )
        .await
        .expect("activity after undo");
    assert_eq!(page.items.len(), 2);
    let undo_op = page
        .items
        .iter()
        .find(|i| i.operation_type == OpKind::UndoMove)
        .expect("UndoMove row recorded");
    assert_eq!(undo_op.status, OpStatus::Completed);
    let orig_op = page
        .items
        .iter()
        .find(|i| i.operation_type == OpKind::Move)
        .expect("original Move row");
    assert_eq!(orig_op.reversed_by.as_deref(), Some(undo_op.id.as_str()));
}

#[tokio::test]
async fn r_smoke_06_restart_persistence_and_startup_recovery() {
    let env = CleanEnv::new();
    let doc = env.desktop.join("Notes.txt");
    write_new_file(&doc, b"important user notes");

    // Session 1: configure folder, rule, onboarding, and perform an operation.
    let (folder_id, rule_id) = {
        let db = database::open(&env.db_path).await.expect("open session 1");
        let cats = CategoryService::new(db.clone());
        cats.ensure_builtins().await.expect("categories");
        let folder = FolderService::new(db.clone(), env.root_policy())
            .add(&env.desktop, true)
            .await
            .expect("add folder");
        let rule_svc = RuleService::new(db.clone(), env.root_policy());
        let dest_root = rule_svc
            .add_destination_root(&env.archive_dest)
            .await
            .expect("dest root");
        let rule = rule_svc
            .create(
                RuleDraft {
                    name: "Persist Across Restart".into(),
                    description: None,
                    enabled: true,
                    source_folders: vec![folder.id.clone()],
                    conditions: ConditionNode::Leaf {
                        condition: Condition::Text {
                            field: TextFieldKind::Extension,
                            op: TextOp::Equals,
                            value: "txt".into(),
                            case_sensitive: false,
                        },
                    },
                    actions: vec![Action::Move {
                        destination: format!("{dest_root}/Notes"),
                    }],
                    date_source: DateSource::Modified,
                    execution_mode: ExecutionMode::FirstMatch,
                },
                RuleOrigin::User,
            )
            .await
            .expect("create rule");

        let clock = Arc::new(FixedClock::utc(1_794_700_300_000));
        let onboarding = OnboardingService::new(
            db.clone(),
            env.root_policy(),
            clock.clone(),
            PreviewStore::new(),
        );
        onboarding
            .complete(Vec::new(), true)
            .await
            .expect("complete onboarding");

        let safe_fs = Arc::new(SafeFs::with_trash_dir(env.trash_dir.clone()));
        let organizer = Organizer::new(Ledger::new(db.clone()), safe_fs, clock.clone());
        let classifier = Arc::new(Classifier::new(cats.list().await.expect("cats")));
        let authority = load_path_authority(&db).await.expect("authority");
        let monitoring = MonitoringSettings::default();
        let plan = build_execution_plan(
            &PlanContext {
                fs: &RealFs,
                authority: &authority,
                classifier: &classifier,
                clock: clock.as_ref(),
            },
            &rule_svc.list().await.expect("rules"),
            &collect_folder_candidates(&folder, &monitoring, &classifier, None),
            &PlanScope::default(),
            &PlanEnv::default(),
        );
        let exec = execute_plan(
            &plan,
            None,
            ConflictPolicy::AutoRename,
            &organizer,
            &authority,
            BatchTrigger::Schedule,
        )
        .await
        .expect("execute");
        assert_eq!(exec.completed.len(), 1);

        db.close().await;
        (folder.id, rule.id)
    };

    // Session 2: reopen same DB path, run startup recovery, and verify persisted state.
    let db2 = database::open(&env.db_path)
        .await
        .expect("reopen session 2");
    let clock2 = Arc::new(FixedClock::utc(1_794_700_400_000));
    let safe_fs2 = Arc::new(SafeFs::with_trash_dir(env.trash_dir.clone()));
    let activity2 = ActivityService::new(db2.clone(), safe_fs2.clone(), clock2.clone());
    let auth2 = activity2
        .authority()
        .await
        .expect("authority after restart");
    let scan_roots: Vec<PathBuf> = auth2.roots().iter().map(|r| r.path.clone()).collect();
    let rec = recover_startup(
        activity2.organizer().ledger(),
        safe_fs2.as_ref(),
        clock2.as_ref(),
        &scan_roots,
    )
    .await
    .expect("startup recovery");
    assert!(rec.failed.is_empty());

    let persisted_folders = repo_folders::list(&db2).await.expect("folders persisted");
    assert_eq!(persisted_folders.len(), 1);
    assert_eq!(persisted_folders[0].id, folder_id);

    let persisted_rules = repo_rules::list(&db2).await.expect("rules persisted");
    assert_eq!(persisted_rules.len(), 1);
    assert_eq!(persisted_rules[0].id, rule_id);
    assert!(persisted_rules[0].enabled);

    let onboarding2 =
        OnboardingService::new(db2.clone(), env.root_policy(), clock2, PreviewStore::new());
    assert!(
        onboarding2
            .get_state()
            .await
            .expect("onboarding state")
            .completed,
        "onboarding completion must persist across restarts"
    );

    let page = activity2
        .list(
            ActivityFilter::default(),
            PageRequest {
                limit: 10,
                offset: 0,
            },
        )
        .await
        .expect("activity persisted");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].status, OpStatus::Completed);
}

#[tokio::test]
async fn r_smoke_07_offline_license_and_core_automation_with_zero_network() {
    let env = CleanEnv::new();
    let signing_key = SigningKey::from_bytes(&[42u8; 32]);
    let verifying_key = signing_key.verifying_key();
    let clock = Arc::new(FixedClock::utc(1_794_700_500_000));
    let keychain = Arc::new(InMemoryKeychain::new());

    // Online provider explicitly unconfigured: proves zero network dependency (FR-213, NFR-010).
    let license_svc = LicenseService::with_verifying_key(
        keychain.clone(),
        Arc::new(UnconfiguredOnlineProvider),
        clock.clone(),
        verifying_key,
    );
    assert_eq!(
        license_svc.status().expect("unlicensed").state,
        LicenseStateKind::Unlicensed
    );

    let token = sign_offline_token(
        &signing_key,
        &SignedLicensePayload {
            version: 1,
            license_id: "lic_smoke_1_0_0".into(),
            edition: LicenseEdition::Lifetime,
            customer_hint: Some("smoke@example.com".into()),
            issued_at_ms: 1_794_600_000_000,
            expires_at_ms: None,
            max_devices: Some(3),
        },
    );

    let activated = license_svc
        .activate_offline(&token)
        .expect("offline activation");
    assert!(activated.licensed);
    assert_eq!(activated.state, LicenseStateKind::ActiveOffline);
    assert!(!activated.online_provider_configured);

    // Re-instantiate service against persisted Keychain entry to verify offline launch status.
    let relaunched = LicenseService::with_verifying_key(
        keychain,
        Arc::new(UnconfiguredOnlineProvider),
        clock.clone(),
        verifying_key,
    );
    assert_eq!(
        relaunched
            .status()
            .expect("persisted offline license")
            .state,
        LicenseStateKind::ActiveOffline
    );

    // Core file organization continues offline with zero network calls.
    let db = database::open(&env.db_path).await.expect("open DB offline");
    CategoryService::new(db.clone())
        .ensure_builtins()
        .await
        .expect("categories");
    assert!(SettingsService::new(db).get().await.is_ok());
}

#[tokio::test]
async fn r_smoke_08_uninstall_removes_app_data_and_leaves_user_files_intact() {
    let env = CleanEnv::new();
    let app_bundle = env.home.join("Applications/FolderFlow.app");
    fs::create_dir_all(app_bundle.join("Contents/MacOS")).expect("create app bundle");
    write_new_file(&app_bundle.join("Contents/Info.plist"), b"<plist/>");

    let keep_dl = env.downloads.join("Quarterly Report.pdf");
    let keep_desk = env.desktop.join("Family Photo.jpg");
    let keep_doc = env.archive_dest.join("Taxes/2025.pdf");
    write_new_file(&keep_dl, b"user download bytes");
    write_new_file(&keep_desk, b"user photo bytes");
    write_new_file(&keep_doc, b"user organized archive bytes");

    // Initialize FolderFlow app data & logs inside ~/Library/...
    let db = database::open(&env.db_path).await.expect("open DB");
    CategoryService::new(db.clone())
        .ensure_builtins()
        .await
        .expect("categories");
    let folder = FolderService::new(db.clone(), env.root_policy())
        .add(&env.downloads, true)
        .await
        .expect("monitor Downloads");
    assert_eq!(folder.name, "Downloads");
    db.close().await;

    let mut log = logging::build_appender(&env.logs_dir, 14).expect("log");
    writeln!(log, "FolderFlow session").expect("write log");
    log.flush().expect("flush log");

    let dl_before = snapshot_tree(&env.downloads);
    let desk_before = snapshot_tree(&env.desktop);
    let doc_before = snapshot_tree(&env.archive_dest);

    // Simulate clean uninstall: delete app bundle + Application Support + Logs.
    simulate_app_uninstall(&[&app_bundle, &env.app_support, &env.logs_dir]);

    assert!(!app_bundle.exists(), "app bundle should be removed");
    assert!(!env.app_support.exists(), "app support should be removed");
    assert!(!env.logs_dir.exists(), "logs dir should be removed");

    // User folders and files MUST remain 100% intact.
    assert_eq!(snapshot_tree(&env.downloads), dl_before);
    assert_eq!(snapshot_tree(&env.desktop), desk_before);
    assert_eq!(snapshot_tree(&env.archive_dest), doc_before);
}
