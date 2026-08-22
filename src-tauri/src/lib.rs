mod application;
mod asr;
mod commands;
mod credentials;
mod db;
mod director;
mod domain;
mod error;
mod exporter;
mod infrastructure;
mod localization;
mod media;
// Project-level recovery commands stay separate from the media-heavy command module.
mod project_commands;
mod script;
mod timeline_map;
mod translation;
mod tts;
pub mod tts_provider;
mod visual_analysis;
pub mod workflow;
mod workflow_commands;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use db::Database;
use domain::SegmentRecord;
use tauri::Manager;

pub struct AppState {
    database: Mutex<Database>,
    workflow_store: Arc<infrastructure::workflow_store::SharedWorkflowStore>,
    workflow_scheduler: Arc<workflow::ResourceScheduler>,
    workflow_cancellations: Mutex<HashMap<String, workflow::CancellationToken>>,
    preview_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    tts_fit_snapshots: Mutex<HashMap<String, Vec<SegmentRecord>>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let database_path = data_dir.join("app.db");
            let database = Database::open(&database_path)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let workflow_store =
                infrastructure::workflow_store::SharedWorkflowStore::open(&database_path)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
            app.manage(AppState {
                database: Mutex::new(database),
                workflow_store: Arc::new(workflow_store),
                workflow_scheduler: Arc::new(workflow::ResourceScheduler::production()),
                workflow_cancellations: Mutex::new(HashMap::new()),
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
            project_commands::project_readiness,
            project_commands::project_audio_mode_update,
            commands::media_probe,
            commands::preview_media,
            commands::preview_prepare,
            commands::job_list,
            commands::job_delete,
            workflow_commands::workflow_enqueue,
            workflow_commands::workflow_start,
            workflow_commands::workflow_continue,
            workflow_commands::workflow_retry,
            workflow_commands::workflow_pause,
            workflow_commands::workflow_cancel,
            commands::segment_upsert,
            commands::segment_list,
            commands::segment_replace_project,
            commands::project_tts_settings_update,
            commands::segment_script_update,
            commands::director_plan,
            commands::glossary_list,
            commands::glossary_save,
            commands::glossary_delete,
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
