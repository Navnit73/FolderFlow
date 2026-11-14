//! Menu bar tray state, live icon/menu generation, and background keep-alive (`FR-021`, `FR-150..152`, `P7-T01`).

use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{ActivationPolicy, AppHandle, Manager, WindowEvent};
use tauri_specta::Event;

use crate::app_state::AppState;
use crate::engine::monitor::{MonitoringStatus, RuntimeState};
use crate::error::{AppError, AppResult};
use crate::events::TrayNavigateEvent;

pub const TRAY_ID: &str = "main-tray";

pub mod menu_ids {
    pub const STATUS: &str = "tray_status";
    pub const MODE: &str = "tray_mode";
    pub const PAUSE_ALL: &str = "tray_pause_all";
    pub const RESUME_ALL: &str = "tray_resume_all";
    pub const ORGANIZE_NOW: &str = "tray_organize_now";
    pub const PREVIEW_PENDING: &str = "tray_preview";
    pub const RECENT_ACTIVITY: &str = "tray_activity";
    pub const OPEN_APP: &str = "tray_open";
    pub const SETTINGS: &str = "tray_settings";
    pub const QUIT: &str = "tray_quit";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TrayVisualState {
    Active,
    Paused,
    Working,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuEntry {
    pub id: &'static str,
    pub label: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraySnapshot {
    pub visual_state: TrayVisualState,
    pub tooltip: String,
    pub status_label: String,
    pub mode_label: String,
    pub can_pause_all: bool,
    pub can_resume_all: bool,
    pub can_organize_now: bool,
}

impl TraySnapshot {
    /// Returns every menu entry required by `FR-150`.
    pub fn entries(&self) -> Vec<TrayMenuEntry> {
        vec![
            TrayMenuEntry {
                id: menu_ids::STATUS,
                label: self.status_label.clone(),
                enabled: false,
            },
            TrayMenuEntry {
                id: menu_ids::MODE,
                label: self.mode_label.clone(),
                enabled: false,
            },
            TrayMenuEntry {
                id: menu_ids::PAUSE_ALL,
                label: "Pause All".to_owned(),
                enabled: self.can_pause_all,
            },
            TrayMenuEntry {
                id: menu_ids::RESUME_ALL,
                label: "Resume Monitoring".to_owned(),
                enabled: self.can_resume_all,
            },
            TrayMenuEntry {
                id: menu_ids::ORGANIZE_NOW,
                label: "Organize Now".to_owned(),
                enabled: self.can_organize_now,
            },
            TrayMenuEntry {
                id: menu_ids::PREVIEW_PENDING,
                label: "Preview Pending".to_owned(),
                enabled: true,
            },
            TrayMenuEntry {
                id: menu_ids::RECENT_ACTIVITY,
                label: "Recent Activity".to_owned(),
                enabled: true,
            },
            TrayMenuEntry {
                id: menu_ids::OPEN_APP,
                label: "Open FolderFlow".to_owned(),
                enabled: true,
            },
            TrayMenuEntry {
                id: menu_ids::SETTINGS,
                label: "Settings…".to_owned(),
                enabled: true,
            },
            TrayMenuEntry {
                id: menu_ids::QUIT,
                label: "Quit FolderFlow".to_owned(),
                enabled: true,
            },
        ]
    }
}

/// Computes live menu bar icon state, labels, and action enablement from monitoring + pipeline state (`FR-150`, `FR-151`).
pub fn compute_tray_snapshot(status: &MonitoringStatus, pipeline_busy: bool) -> TraySnapshot {
    let total = status.folders.len();
    let has_error = status.folders.iter().any(|f| {
        matches!(
            f.state,
            RuntimeState::Error | RuntimeState::PermissionDenied | RuntimeState::Unavailable
        )
    });
    let has_scanning = status
        .folders
        .iter()
        .any(|f| matches!(f.state, RuntimeState::Scanning | RuntimeState::Starting));

    let visual_state = if status.paused {
        TrayVisualState::Paused
    } else if pipeline_busy || has_scanning {
        TrayVisualState::Working
    } else if has_error {
        TrayVisualState::Error
    } else {
        TrayVisualState::Active
    };

    let status_label = match visual_state {
        TrayVisualState::Paused => "Status: Monitoring Paused".to_owned(),
        TrayVisualState::Working => "Status: Organizing / Scanning…".to_owned(),
        TrayVisualState::Error => "Status: Needs Attention".to_owned(),
        TrayVisualState::Active => {
            if total == 0 {
                "Status: Ready (No Folders)".to_owned()
            } else if total == 1 {
                "Status: Monitoring 1 Folder".to_owned()
            } else {
                format!("Status: Monitoring {total} Folders")
            }
        }
    };

    let mode_label = if status.paused {
        "State: Paused".to_owned()
    } else if total == 0 {
        "State: Idle".to_owned()
    } else {
        "State: Active".to_owned()
    };

    let tooltip = format!(
        "FolderFlow — {}",
        status_label.trim_start_matches("Status: ")
    );

    TraySnapshot {
        visual_state,
        tooltip,
        status_label,
        mode_label,
        can_pause_all: !status.paused && total > 0,
        can_resume_all: status.paused && total > 0,
        can_organize_now: !status.paused && total > 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowCloseDisposition {
    HideToBackground,
    ExitApp,
}

/// Window closing keeps monitoring & background engine alive until an explicit Quit (`FR-021`, `FR-152`).
pub fn window_close_disposition(quit_requested: bool) -> WindowCloseDisposition {
    if quit_requested {
        WindowCloseDisposition::ExitApp
    } else {
        WindowCloseDisposition::HideToBackground
    }
}

/// Renders an 18×18 RGBA template glyph representing the live tray state (`FR-151`).
pub fn render_status_icon_rgba(state: TrayVisualState) -> Vec<u8> {
    const W: usize = 18;
    const H: usize = 18;
    let mut rgba = vec![0u8; W * H * 4];

    let mut set_px = |x: usize, y: usize, a: u8| {
        if x < W && y < H {
            let i = (y * W + x) * 4;
            rgba[i] = 0;
            rgba[i + 1] = 0;
            rgba[i + 2] = 0;
            rgba[i + 3] = a;
        }
    };

    // Crisp monochrome folder silhouette in black with alpha (macOS template image auto-tints for light/dark bar).
    for x in 2..=7 {
        set_px(x, 3, 255);
        set_px(x, 4, 255);
    }
    for y in 5..=14 {
        for x in 2..=15 {
            let on_border = x == 2 || x == 15 || y == 5 || y == 14;
            if on_border {
                set_px(x, y, 255);
            } else {
                set_px(x, y, 40);
            }
        }
    }

    // Center indicator overlay reflecting state: active (inner dot), paused (two bars), working (diagonal sweep), error (!).
    match state {
        TrayVisualState::Active => {
            for y in 8..=11 {
                for x in 7..=10 {
                    set_px(x, y, 255);
                }
            }
        }
        TrayVisualState::Paused => {
            for y in 8..=11 {
                set_px(7, y, 255);
                set_px(10, y, 255);
            }
        }
        TrayVisualState::Working => {
            set_px(6, 10, 255);
            set_px(8, 9, 255);
            set_px(10, 8, 255);
            set_px(12, 7, 255);
        }
        TrayVisualState::Error => {
            for y in 7..=10 {
                set_px(8, y, 255);
                set_px(9, y, 255);
            }
            set_px(8, 12, 255);
            set_px(9, 12, 255);
        }
    }
    rgba
}

fn build_menu(app: &AppHandle, snap: &TraySnapshot) -> AppResult<Menu<tauri::Wry>> {
    let entries = snap.entries();
    let item = |idx: usize| {
        let e = &entries[idx];
        MenuItem::with_id(app, e.id, &e.label, e.enabled, None::<&str>)
            .map_err(|err| AppError::Internal(format!("tray menu item {}: {err}", e.id)))
    };

    let status_item = item(0)?;
    let mode_item = item(1)?;
    let sep1 = PredefinedMenuItem::separator(app)
        .map_err(|err| AppError::Internal(format!("tray sep1: {err}")))?;
    let pause_item = item(2)?;
    let resume_item = item(3)?;
    let organize_item = item(4)?;
    let sep2 = PredefinedMenuItem::separator(app)
        .map_err(|err| AppError::Internal(format!("tray sep2: {err}")))?;
    let preview_item = item(5)?;
    let activity_item = item(6)?;
    let open_item = item(7)?;
    let settings_item = item(8)?;
    let sep3 = PredefinedMenuItem::separator(app)
        .map_err(|err| AppError::Internal(format!("tray sep3: {err}")))?;
    let quit_item = item(9)?;

    MenuBuilder::new(app)
        .items(&[
            &status_item,
            &mode_item,
            &sep1,
            &pause_item,
            &resume_item,
            &organize_item,
            &sep2,
            &preview_item,
            &activity_item,
            &open_item,
            &settings_item,
            &sep3,
            &quit_item,
        ])
        .build()
        .map_err(|err| AppError::Internal(format!("tray menu build: {err}")))
}

/// Shows the main window, restores standard Dock activation policy, and navigates optionally.
pub fn show_main_window(app: &AppHandle, navigate_to: Option<&str>) {
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
    if let Some(route) = navigate_to {
        let _ = TrayNavigateEvent {
            path: route.to_owned(),
        }
        .emit(app);
    }
}

/// Hides the main window while preserving background monitoring (`FR-021`).
pub fn hide_main_window(app: &AppHandle, show_menu_bar_icon: bool) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    if show_menu_bar_icon {
        let _ = app.set_activation_policy(ActivationPolicy::Accessory);
    }
}

/// Creates the menu bar tray icon and handles live updates + menu actions (`P7-T01`).
pub fn init_tray(
    app: &AppHandle,
    visible: bool,
    initial_status: &MonitoringStatus,
    busy: bool,
) -> AppResult<()> {
    let snap = compute_tray_snapshot(initial_status, busy);
    let menu = build_menu(app, &snap)?;
    let icon = Image::new_owned(render_status_icon_rgba(snap.visual_state), 18, 18);

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .icon(icon)
        .icon_as_template(true)
        .tooltip(&snap.tooltip)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref().to_owned();
            handle_menu_action(app, &id);
        })
        .on_tray_icon_event(|_tray, _event: TrayIconEvent| {})
        .build(app)
        .map_err(|e| AppError::Internal(format!("failed to create tray icon: {e}")))?;

    let _ = tray.set_visible(visible);
    Ok(())
}

/// Refreshes tray visibility, tooltip, icon glyph, and menu item state from `AppState`.
pub async fn refresh_tray_async(app: AppHandle) {
    let state = app.state::<AppState>();
    let status = state.monitor.status().await;
    let busy = state.pipeline.is_busy();
    let visible = state
        .settings
        .get()
        .await
        .map(|s| s.general.show_menu_bar_icon)
        .unwrap_or(true);
    let app_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        apply_tray_update(&app_main, visible, &status, busy);
    });
}

/// Updates an existing tray icon on the main thread cleanly.
pub fn apply_tray_update(app: &AppHandle, visible: bool, status: &MonitoringStatus, busy: bool) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let snap = compute_tray_snapshot(status, busy);
    if let Ok(menu) = build_menu(app, &snap) {
        let _ = tray.set_menu(Some(menu));
    }
    let icon = Image::new_owned(render_status_icon_rgba(snap.visual_state), 18, 18);
    let _ = tray.set_icon_with_as_template(Some(icon), true);
    let _ = tray.set_tooltip(Some(&snap.tooltip));
    let _ = tray.set_visible(visible);
}

fn handle_menu_action(app: &AppHandle, id: &str) {
    match id {
        menu_ids::PAUSE_ALL => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                state.pipeline.set_paused(true);
                state.monitor.pause_all().await;
                refresh_tray_async(app).await;
            });
        }
        menu_ids::RESUME_ALL => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                state.pipeline.set_paused(false);
                state.monitor.resume_all().await;
                refresh_tray_async(app).await;
            });
        }
        menu_ids::ORGANIZE_NOW => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                state.monitor.rescan_all().await;
                let _ = state.pipeline.drain_and_reconcile().await;
                refresh_tray_async(app).await;
            });
        }
        menu_ids::PREVIEW_PENDING => show_main_window(app, Some("/preview")),
        menu_ids::RECENT_ACTIVITY => show_main_window(app, Some("/activity")),
        menu_ids::OPEN_APP => show_main_window(app, None),
        menu_ids::SETTINGS => show_main_window(app, Some("/settings")),
        menu_ids::QUIT => {
            let app = app.clone();
            let state = app.state::<AppState>();
            state.quit_requested.store(true, Ordering::SeqCst);
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                state.monitor.shutdown().await;
                app.exit(0);
            });
        }
        _ => {}
    }
}

/// Handles `WindowEvent::CloseRequested`: hides main window and preserves monitoring (`FR-021`, `FR-152`).
pub fn handle_window_event(app: &AppHandle, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        let state = app.state::<AppState>();
        if window_close_disposition(state.quit_requested.load(Ordering::SeqCst))
            == WindowCloseDisposition::HideToBackground
        {
            api.prevent_close();
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_clone.state::<AppState>();
                let show_tray = state
                    .settings
                    .get()
                    .await
                    .map(|s| s.general.show_menu_bar_icon)
                    .unwrap_or(true);
                let app_for_main = app_clone.clone();
                let _ = app_clone.run_on_main_thread(move || {
                    hide_main_window(&app_for_main, show_tray);
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::mpsc;

    use super::*;
    use crate::categories::CategoryService;
    use crate::database::{self, repo_files};
    use crate::engine::classifier::Classifier;
    use crate::engine::monitor::{
        FolderRuntime, MonitorDeps, MonitorObserver, MonitorService, MonitorTiming, RawEvent,
        RuntimeState,
    };
    use crate::engine::paths::RootPolicy;
    use crate::folders::FolderService;
    use crate::settings::MonitoringSettings;

    fn folder_rt(state: RuntimeState) -> FolderRuntime {
        FolderRuntime {
            folder_id: "f1".into(),
            state,
            pending_events: 0,
            tracked_files: 3,
            last_error: None,
            last_event_at: None,
        }
    }

    #[test]
    fn r_tray_01_snapshot_reflects_active_paused_working_error_and_contains_all_required_items() {
        let active = compute_tray_snapshot(
            &MonitoringStatus {
                paused: false,
                folders: vec![folder_rt(RuntimeState::Watching)],
            },
            false,
        );
        assert_eq!(active.visual_state, TrayVisualState::Active);
        assert!(active.can_pause_all);
        assert!(!active.can_resume_all);
        assert!(active.can_organize_now);
        assert!(active.status_label.contains("1 Folder"));

        let entries = active.entries();
        let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
        for required in [
            "Pause All",
            "Resume Monitoring",
            "Organize Now",
            "Preview Pending",
            "Recent Activity",
            "Open FolderFlow",
            "Settings…",
            "Quit FolderFlow",
        ] {
            assert!(
                labels.contains(&required),
                "missing tray menu entry {required}"
            );
        }

        let paused = compute_tray_snapshot(
            &MonitoringStatus {
                paused: true,
                folders: vec![folder_rt(RuntimeState::Paused)],
            },
            false,
        );
        assert_eq!(paused.visual_state, TrayVisualState::Paused);
        assert!(!paused.can_pause_all);
        assert!(paused.can_resume_all);
        assert!(!paused.can_organize_now);

        let working = compute_tray_snapshot(
            &MonitoringStatus {
                paused: false,
                folders: vec![folder_rt(RuntimeState::Watching)],
            },
            true,
        );
        assert_eq!(working.visual_state, TrayVisualState::Working);

        let err = compute_tray_snapshot(
            &MonitoringStatus {
                paused: false,
                folders: vec![folder_rt(RuntimeState::PermissionDenied)],
            },
            false,
        );
        assert_eq!(err.visual_state, TrayVisualState::Error);

        // Icon renderer produces valid 18x18 RGBA buffers for every visual state.
        for state in [
            TrayVisualState::Active,
            TrayVisualState::Paused,
            TrayVisualState::Working,
            TrayVisualState::Error,
        ] {
            assert_eq!(render_status_icon_rgba(state).len(), 18 * 18 * 4);
        }
    }

    struct DummyObs;
    impl MonitorObserver for DummyObs {
        fn on_status(&self, _: &FolderRuntime) {}
        fn on_stable(&self, _: &str, _: &crate::engine::scanner::ScannedFile) {}
        fn on_removed(&self, _: &str, _: &Path) {}
    }

    #[tokio::test]
    async fn r_tray_02_window_close_hides_without_quitting_and_keeps_monitoring_running() {
        assert_eq!(
            window_close_disposition(false),
            WindowCloseDisposition::HideToBackground
        );
        assert_eq!(
            window_close_disposition(true),
            WindowCloseDisposition::ExitApp
        );

        // Verify background monitoring + rescan continue functioning cleanly until explicit quit.
        let dir = tempfile::tempdir().expect("tempdir");
        let db = database::open(&dir.path().join("app.db"))
            .await
            .expect("db");
        let cats = CategoryService::new(db.clone());
        cats.ensure_builtins().await.expect("builtins");
        let classifier = Arc::new(Classifier::new(cats.list().await.expect("cats")));
        let home = dir.path().join("home");
        let app_data = dir.path().join("appdata");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&app_data).expect("appdata");
        let folders = FolderService::new(
            db.clone(),
            RootPolicy {
                home: home.clone(),
                app_data_dir: app_data,
            },
        );
        let watched = home.join("Desktop");
        std::fs::create_dir_all(&watched).expect("watched");
        let folder = folders.add(&watched, false).await.expect("add");

        let mut deps = MonitorDeps::new(
            db.clone(),
            classifier,
            MonitoringSettings::default(),
            Arc::new(DummyObs),
        );
        deps.watcher_factory = Arc::new(
            |_root: &Path, _rec: bool, tx: mpsc::Sender<RawEvent>, _overflow: Arc<AtomicBool>| {
                Ok(Box::new(tx) as Box<dyn Send>)
            },
        );
        deps.timing = MonitorTiming {
            debounce: Duration::from_millis(10),
            stability_interval: Duration::from_millis(10),
            stability_checks: 1,
            reconcile_interval: Duration::from_secs(3600),
            health_interval: Duration::from_secs(3600),
            backoff_min: Duration::from_millis(20),
            backoff_max: Duration::from_millis(50),
            stop_timeout: Duration::from_millis(500),
            max_pending: 1000,
            max_tracked: 1000,
        };
        let monitor = MonitorService::new(deps);
        monitor.start(folder.clone()).await;

        // Simulated window close keeps engine alive -> pause/resume & organize now work.
        assert_eq!(
            window_close_disposition(false),
            WindowCloseDisposition::HideToBackground
        );
        monitor.pause_all().await;
        assert!(monitor.status().await.paused);
        monitor.resume_all().await;
        assert!(!monitor.status().await.paused);

        let f = watched.join("note.txt");
        crate::test_support::write_file(&f, b"after close");
        monitor.rescan_all().await;
        let watched_str = watched.to_string_lossy().into_owned();
        let mut seen = false;
        for _ in 0..60 {
            if repo_files::list_facts_for_folder(&db, &folder.id, &watched_str, "Desktop")
                .await
                .expect("facts")
                .iter()
                .any(|fact| fact.path.ends_with("note.txt"))
            {
                seen = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            seen,
            "background monitor must keep organizing/scanning after window close"
        );
        monitor.shutdown().await;
    }

    #[test]
    fn edge_07_tray_zero_folders_and_close_vs_quit_disposition() {
        let empty_active = compute_tray_snapshot(
            &MonitoringStatus {
                paused: false,
                folders: vec![],
            },
            false,
        );
        assert_eq!(empty_active.visual_state, TrayVisualState::Active);
        assert!(!empty_active.can_pause_all);
        assert!(!empty_active.can_resume_all);
        assert!(!empty_active.can_organize_now);

        let empty_paused = compute_tray_snapshot(
            &MonitoringStatus {
                paused: true,
                folders: vec![],
            },
            true,
        );
        assert_eq!(empty_paused.visual_state, TrayVisualState::Paused);
        assert!(!empty_paused.can_pause_all);
        assert!(!empty_paused.can_resume_all);
        assert!(!empty_paused.can_organize_now);

        assert_eq!(
            window_close_disposition(false),
            WindowCloseDisposition::HideToBackground
        );
        assert_eq!(
            window_close_disposition(true),
            WindowCloseDisposition::ExitApp
        );
    }
}
