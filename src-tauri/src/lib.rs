//! The desktop window of Icon Studio. The work is done by `uis-core` (the same functions the `uis`
//! command line calls); this crate only exposes them as Tauri commands (commands.rs).

mod commands;

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

pub const MAIN_LABEL: &str = "main";

/// Set once the window has been shown (the page asks for it after its first paint).
static SHOWN: AtomicBool = AtomicBool::new(false);

pub(crate) fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub(crate) fn show_first(app: &tauri::AppHandle) {
    if !SHOWN.swap(true, Ordering::SeqCst) {
        show_main(app);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // A second start shows the running window instead. Release builds only: the lock is the app
    // identifier, so an installed copy that is running would quietly end `tauri dev`.
    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)));
    builder
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::POSITION | StateFlags::SIZE | StateFlags::MAXIMIZED)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::settings_get,
            commands::settings_set,
            commands::settings_reset,
            commands::open_path,
            commands::window_ready,
            commands::project_list,
            commands::project_add,
            commands::project_pick,
            commands::project_remove,
            commands::project_set_pack,
            commands::project_probe,
            commands::hub_projects,
            commands::prefab_scan,
            commands::prefab_override,
            commands::pack_list,
            commands::pack_get,
            commands::capture_only,
            commands::capture_image,
            commands::recovery_status,
            commands::recovery_run,
            commands::readiness,
            commands::codex_copy_from_app,
            commands::prompt_preview,
            commands::prompt_rewrite,
            commands::generate_raw,
            commands::review,
            commands::images_today,
            commands::post_rerun,
            commands::export,
            commands::jobs_list,
            commands::run_start,
            commands::run_cancel,
            commands::job_get,
            commands::prefab_thumbs,
            commands::export_pick,
            commands::export_check,
        ])
        .setup(|app| {
            let home = uis_core::home::Home::resolve();
            let _ = home.ensure();
            // The page normally asks for the first show; if it never does, show the window anyway
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                show_first(&handle);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
