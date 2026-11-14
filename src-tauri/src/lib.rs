//! FolderFlow application crate.
//!
//! `lib.rs` only wires modules, state, plugins and commands together.
//! Logic lives in the modules (docs/ARCHITECTURE.md §3–4).

mod app_state;
pub mod categories;
pub mod commands;
pub mod config_io;
pub mod database;
pub mod duplicates;
pub mod engine;
pub mod error;
pub mod events;
pub mod folders;
pub mod logging;
pub mod notifications;
pub mod onboarding;
pub mod rules;
pub mod security;
pub mod settings;
pub mod system;
pub mod utils;

#[cfg(test)]
pub(crate) mod test_support;

use specta_typescript::Typescript;
use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Builder, ErrorHandlingMode};

/// All commands exposed to the frontend. Adding a command here (and
/// regenerating bindings with `pnpm bindings`) is the only way to expose it.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::app::app_info,
            commands::settings::settings_get,
            commands::settings::settings_update,
            commands::settings::login_item_status,
            commands::settings::open_login_items_settings,
            commands::license::license_status,
            commands::license::license_activate,
            commands::license::license_activate_offline,
            commands::license::license_pick_and_activate_offline_file,
            commands::license::license_deactivate,
            commands::folders::folders_list,
            commands::folders::folders_add_with_picker,
            commands::folders::folders_add_known,
            commands::folders::folders_update,
            commands::folders::folders_remove,
            commands::folders::folders_action,
            commands::folders::folders_pick_subfolder,
            commands::folders::folders_summary,
            commands::folders::folders_errors,
            commands::folders::folders_access,
            commands::folders::open_privacy_settings,
            commands::monitoring::monitoring_status,
            commands::monitoring::monitoring_set_paused,
            commands::categories::categories_list,
            commands::categories::categories_create,
            commands::categories::categories_update,
            commands::categories::categories_delete,
            commands::categories::categories_set_enabled,
            commands::categories::categories_reorder,
            commands::categories::categories_pick_destination_folder,
            commands::rules::rules_list,
            commands::rules::rules_get,
            commands::rules::rules_create,
            commands::rules::rules_create_from_preset,
            commands::rules::rules_smart_catalog,
            commands::rules::rules_create_smart_preset,
            commands::rules::rules_update,
            commands::rules::rules_delete,
            commands::rules::rules_reorder,
            commands::rules::rules_set_enabled,
            commands::rules::rules_duplicate,
            commands::rules::rules_validate,
            commands::rules::rules_presets,
            commands::rules::template_preview,
            commands::rules::rules_test_folder,
            commands::rules::rules_pick_destination_folder,
            commands::activity::activity_list,
            commands::activity::activity_undo,
            commands::activity::activity_retry,
            commands::activity::activity_export_csv,
            commands::activity::activity_clear,
            commands::activity::reveal_in_finder,
            commands::activity::review_list,
            commands::activity::review_resolve,
            commands::preview::plan_create,
            commands::preview::plan_items,
            commands::preview::plan_execute,
            commands::preview::plan_discard,
            commands::duplicates::dup_scan,
            commands::duplicates::dup_groups,
            commands::duplicates::dup_group_details,
            commands::duplicates::dup_action,
            commands::duplicates::dup_export,
            commands::duplicates::dup_reveal_file,
            commands::duplicates::dup_pick_review_folder,
            commands::dashboard::dashboard_stats,
            commands::dashboard::db_maintenance,
            commands::exclusions::exclusions_list,
            commands::exclusions::exclusions_create,
            commands::exclusions::exclusions_delete,
            commands::config_io::config_export,
            commands::config_io::config_pick_import_file,
            commands::config_io::config_import_preview,
            commands::config_io::config_import_apply,
            commands::notifications::notifications_list,
            commands::notifications::notifications_mark_read,
            commands::notifications::notifications_clear,
            commands::notifications::notifications_test,
            commands::schedules::schedules_list,
            commands::schedules::schedules_upsert,
            commands::schedules::schedules_delete,
            commands::schedules::schedules_set_enabled,
            commands::schedules::schedules_run_now,
            commands::onboarding::onboarding_state,
            commands::onboarding::onboarding_set_step,
            commands::onboarding::onboarding_scan_preview,
            commands::onboarding::onboarding_complete,
            commands::onboarding::onboarding_reset,
        ])
        .events(collect_events![
            events::FolderStatusEvent,
            events::FolderActivityEvent,
            events::DupProgressEvent,
            events::AppNotificationEvent,
            events::TrayNavigateEvent
        ])
        .error_handling(ErrorHandlingMode::Result)
}

/// TypeScript exporter configuration for `src/bindings.ts`.
pub fn bindings_exporter() -> Typescript {
    Typescript::default().header(
        "// AUTO-GENERATED by tauri-specta from src-tauri. DO NOT EDIT.\n// Regenerate with: pnpm bindings\n",
    )
}

pub fn run() {
    let builder = specta_builder();

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let app_handle = app.handle().clone();
            let state = tauri::async_runtime::block_on(app_state::AppState::init(&app_handle))?;
            let current = tauri::async_runtime::block_on(state.settings.get())?;
            let mon_status = tauri::async_runtime::block_on(state.monitor.status());
            let busy = state.pipeline.is_busy();
            app.manage(state);

            system::tray::init_tray(
                &app_handle,
                current.general.show_menu_bar_icon,
                &mon_status,
                busy,
            )?;
            if current.general.start_minimized {
                system::tray::hide_main_window(&app_handle, current.general.show_menu_bar_icon);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            system::tray::handle_window_event(window.app_handle(), event);
        })
        .build(tauri::generate_context!());

    match result {
        Ok(app) => {
            let (sys_tx, mut sys_rx) =
                tokio::sync::mpsc::unbounded_channel::<system::coordinator::WorkspaceSystemEvent>();
            let workspace_guard = system::coordinator::start_workspace_observers(move |ev| {
                let _ = sys_tx.send(ev);
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = sys_rx.recv().await {
                    let state = handle.state::<app_state::AppState>();
                    system::coordinator::handle_workspace_event(
                        ev,
                        &state.folders,
                        &state.monitor,
                        &state.scheduler,
                    )
                    .await;
                    system::tray::refresh_tray_async(handle.clone()).await;
                }
            });

            app.run(move |app_handle, event| {
                let _keep = &workspace_guard;
                match event {
                    tauri::RunEvent::Reopen {
                        has_visible_windows: false,
                        ..
                    } => {
                        system::tray::show_main_window(app_handle, None);
                    }
                    tauri::RunEvent::Exit => {
                        let state = app_handle.state::<app_state::AppState>();
                        state
                            .quit_requested
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                        tauri::async_runtime::block_on(state.monitor.shutdown());
                    }
                    _ => {}
                }
            });
        }
        Err(e) => {
            // Logging may not be initialised if startup failed early.
            eprintln!("FolderFlow failed to start: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// R-CMD-01: `src/bindings.ts` must match what the Rust code generates.
    /// Run `pnpm bindings` (UPDATE_BINDINGS=1) to regenerate.
    #[test]
    fn r_cmd_01_bindings_are_up_to_date() {
        let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/bindings.ts");
        if std::env::var("UPDATE_BINDINGS").as_deref() == Ok("1") {
            super::specta_builder()
                .export(super::bindings_exporter(), &committed)
                .expect("export bindings");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let generated_path = dir.path().join("bindings.ts");
        super::specta_builder()
            .export(super::bindings_exporter(), &generated_path)
            .expect("export bindings");
        let generated = std::fs::read_to_string(&generated_path).expect("read generated");
        let current = std::fs::read_to_string(&committed).unwrap_or_default();
        assert!(
            generated == current,
            "src/bindings.ts is out of date with the Rust commands/types. Run `pnpm bindings`."
        );
    }
}
