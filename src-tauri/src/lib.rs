mod asr;
mod commands;
mod credentials;
mod db;
mod domain;
mod error;
mod exporter;
mod media;
mod translation;
mod tts;

use std::sync::Mutex;

use db::Database;
use tauri::Manager;

pub struct AppState {
    database: Mutex<Database>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let database = Database::open(&data_dir.join("app.db"))
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            app.manage(AppState {
                database: Mutex::new(database),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::project_list,
            commands::project_create,
            commands::project_create_from_media,
            commands::media_probe,
            commands::media_prepare,
            commands::preview_media,
            commands::job_enqueue,
            commands::job_list,
            commands::job_start,
            commands::job_pause,
            commands::job_resume,
            commands::job_cancel,
            commands::job_retry,
            commands::job_checkpoint,
            commands::segment_upsert,
            commands::segment_list,
            commands::segment_replace_project,
            commands::asr_run,
            commands::translation_run,
            commands::translation_rebuild,
            commands::tts_run,
            commands::tts_fit_warnings,
            commands::export_start,
            commands::path_reveal,
            commands::segment_invalidation,
            commands::credential_save,
            commands::credential_delete,
            commands::provider_list,
            commands::provider_save,
            commands::provider_test,
            commands::provider_delete,
            commands::runtime_catalog,
            commands::diagnostics_create,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Yisheng Studio");
}
