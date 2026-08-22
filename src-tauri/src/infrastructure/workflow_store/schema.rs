use rusqlite::Connection;

use crate::error::AppError;

pub(super) fn migrate(connection: &Connection) -> Result<(), AppError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS workflow_runs (
           id TEXT PRIMARY KEY,
           workflow_id TEXT NOT NULL,
           workflow_version INTEGER NOT NULL,
           project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           legacy_job_id TEXT,
           status TEXT NOT NULL,
           current_node_id TEXT,
           stage TEXT NOT NULL,
           progress INTEGER NOT NULL DEFAULT 0,
           checkpoint TEXT,
           error_message TEXT,
           cancel_requested INTEGER NOT NULL DEFAULT 0,
           created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
           updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
           UNIQUE(legacy_job_id)
         );
         CREATE TABLE IF NOT EXISTS node_runs (
           id TEXT PRIMARY KEY,
           run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
           node_id TEXT NOT NULL,
           node_version INTEGER NOT NULL,
           attempt INTEGER NOT NULL,
           stage TEXT NOT NULL,
           resource_class TEXT NOT NULL,
           status TEXT NOT NULL,
           input_artifacts_json TEXT NOT NULL DEFAULT '[]',
           output_artifacts_json TEXT NOT NULL DEFAULT '[]',
           checkpoint TEXT,
           error_class TEXT,
           error_message TEXT,
           started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
           finished_at TEXT,
           UNIQUE(run_id, node_id, attempt)
         );
         CREATE TABLE IF NOT EXISTS run_events (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
           node_run_id TEXT REFERENCES node_runs(id) ON DELETE SET NULL,
           kind TEXT NOT NULL,
           payload_json TEXT NOT NULL DEFAULT '{}',
           created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
         );
         CREATE INDEX IF NOT EXISTS workflow_run_project_lookup
           ON workflow_runs(project_id, updated_at);
         CREATE INDEX IF NOT EXISTS node_run_lookup
           ON node_runs(run_id, node_id, attempt DESC);
         CREATE INDEX IF NOT EXISTS run_event_lookup
           ON run_events(run_id, id);
         INSERT OR IGNORE INTO app_migrations(version) VALUES (5);",
    )?;
    Ok(())
}
