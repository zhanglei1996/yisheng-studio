mod asr;
mod commands;
mod credentials;
mod db;
mod director;
mod domain;
mod error;
mod exporter;
mod localization;
mod media;
mod script;
mod timeline_map;
mod translation;
mod tts;
pub mod tts_provider;
mod visual_analysis;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use db::Database;
use domain::SegmentRecord;
use tauri::Manager;

pub struct AppState {
    database: Mutex<Database>,
    preview_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    tts_fit_snapshots: Mutex<HashMap<String, Vec<SegmentRecord>>>,
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
                preview_locks: Mutex::new(HashMap::new()),
                tts_fit_snapshots: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::project_list,
            commands::project_thumbnail,
            commands::project_create,
            commands::project_rename,
            commands::project_delete,
            commands::project_create_from_media,
            commands::project_readiness,
            commands::media_probe,
            commands::media_prepare,
            commands::preview_media,
            commands::preview_prepare,
            commands::job_enqueue,
            commands::job_list,
            commands::job_delete,
            commands::job_start,
            commands::job_pause,
            commands::job_resume,
            commands::job_cancel,
            commands::job_retry,
            commands::job_checkpoint,
            commands::segment_upsert,
            commands::segment_list,
            commands::segment_replace_project,
            commands::project_tts_settings_update,
            commands::segment_script_update,
            commands::director_plan,
            commands::glossary_list,
            commands::glossary_save,
            commands::glossary_delete,
            commands::asr_run,
            commands::translation_run,
            commands::translation_rebuild,
            commands::semantic_narration_run,
            commands::tts_run,
            commands::tts_catalog,
            commands::tts_audition,
            commands::tts_audition_cancel,
            commands::tts_fit_warnings,
            commands::tts_fit_undo,
            commands::export_preflight,
            commands::localization_analyze,
            commands::timeline_edit_accept,
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
