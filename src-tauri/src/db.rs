use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    domain::{
        ArtifactRecord, GlossaryTerm, JobStatus, JobSummary, NarrationScene, NonSpeechEvent,
        ProjectStatus, ProjectSummary, ProviderProfile, SegmentRecord, SyncAnchor, TimelineEdit,
    },
    error::AppError,
    script::{Origin, ScriptDocumentV1},
};

pub struct Database {
    connection: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsSegmentSnapshot {
    pub id: String,
    pub ordinal: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub script_revision: u64,
    pub spoken_zh: String,
    pub script_doc_json: String,
    pub tts_overrides_json: String,
    pub tts_state: String,
    pub tts_settings_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsPublishSnapshot {
    pub project_id: String,
    pub project_tts_revision: u64,
    pub provider_id: String,
    pub voice_id: Option<String>,
    pub style: String,
    pub settings_json: String,
    pub director_enabled: bool,
    pub provider_revision: u64,
    pub segments: Vec<TtsSegmentSnapshot>,
}

#[derive(Debug, Clone)]
pub struct TtsSegmentPublication {
    pub segment_id: String,
    pub expected_script_revision: u64,
    pub state: String,
    pub settings_hash: Option<String>,
    pub duration_ms: Option<i64>,
    pub error_message: Option<String>,
    pub display_status: String,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| AppError::Validation(error.to_string()))?;
        }
        let connection = Connection::open(path)?;
        let database = Self { connection };
        database.migrate()?;
        database.recover_interrupted_jobs()?;
        Ok(database)
    }

    #[cfg(test)]
    pub fn memory() -> Result<Self, AppError> {
        let database = Self {
            connection: Connection::open_in_memory()?,
        };
        database.migrate()?;
        Ok(database)
    }

    fn migrate(&self) -> Result<(), AppError> {
        // WAL mode cannot be changed from inside a transaction. Everything
        // after these two PRAGMAs is one atomic, repeatable V3 migration.
        self.connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;",
        )?;
        self.connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<(), AppError> {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS projects (
               id TEXT PRIMARY KEY,
               name TEXT NOT NULL,
               status TEXT NOT NULL,
               progress INTEGER NOT NULL DEFAULT 0,
               source_path TEXT,
               source_fingerprint TEXT,
               duration_ms INTEGER,
               width INTEGER,
               height INTEGER,
               artifact_dir TEXT,
               workflow_mode TEXT NOT NULL DEFAULT 'quick',
               audio_mode TEXT NOT NULL DEFAULT 'duck',
               translation_provider_id TEXT,
               tts_provider_id TEXT NOT NULL DEFAULT 'system',
               tts_voice_id TEXT,
               tts_style TEXT NOT NULL DEFAULT 'auto',
               tts_settings_json TEXT NOT NULL DEFAULT '{}',
               tts_director_enabled INTEGER NOT NULL DEFAULT 1,
               tts_sync_mode TEXT NOT NULL DEFAULT 'strict',
               tts_settings_revision INTEGER NOT NULL DEFAULT 1,
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS segments (
               id TEXT PRIMARY KEY,
               project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
               ordinal INTEGER NOT NULL,
               start_ms INTEGER NOT NULL,
               end_ms INTEGER NOT NULL,
               source_text TEXT NOT NULL,
               subtitle_zh TEXT NOT NULL,
               spoken_zh TEXT NOT NULL,
               linked INTEGER NOT NULL DEFAULT 1,
               status TEXT NOT NULL DEFAULT 'ready',
               script_doc_json TEXT NOT NULL DEFAULT '',
               script_revision INTEGER NOT NULL DEFAULT 1,
               tts_overrides_json TEXT NOT NULL DEFAULT '{}',
               tts_state TEXT NOT NULL DEFAULT 'stale',
               tts_error_message TEXT,
               tts_settings_hash TEXT,
               tts_duration_ms INTEGER,
               CHECK (end_ms - start_ms >= 300),
               UNIQUE(project_id, ordinal)
             );
             CREATE TABLE IF NOT EXISTS jobs (
               id TEXT PRIMARY KEY,
               project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
               stage TEXT NOT NULL,
               progress INTEGER NOT NULL DEFAULT 0,
               status TEXT NOT NULL,
               checkpoint TEXT,
               error_message TEXT,
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE UNIQUE INDEX IF NOT EXISTS one_heavy_running_job
               ON jobs((1)) WHERE status = 'running';
             CREATE TABLE IF NOT EXISTS artifacts (
               id TEXT PRIMARY KEY,
               project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
               segment_id TEXT REFERENCES segments(id) ON DELETE CASCADE,
               kind TEXT NOT NULL,
               path TEXT NOT NULL,
               content_hash TEXT NOT NULL,
               dependency_hash TEXT NOT NULL,
               cache_key TEXT,
               revision INTEGER NOT NULL DEFAULT 1,
               status TEXT NOT NULL,
               metadata_json TEXT NOT NULL DEFAULT '{}',
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS provider_profiles (
               id TEXT PRIMARY KEY,
               kind TEXT NOT NULL,
               name TEXT NOT NULL,
               public_config_json TEXT NOT NULL,
               credential_ref TEXT,
               driver TEXT NOT NULL DEFAULT '',
               revision INTEGER NOT NULL DEFAULT 1,
               secret_bundle_ref TEXT,
               updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS glossary_terms (
               id TEXT PRIMARY KEY,
               project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
               source TEXT NOT NULL,
               target TEXT NOT NULL,
               policy TEXT NOT NULL,
               enabled INTEGER NOT NULL DEFAULT 1
             );
             CREATE TABLE IF NOT EXISTS app_migrations (
               version INTEGER PRIMARY KEY,
               applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS narration_scenes (
               id TEXT PRIMARY KEY,
               project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
               ordinal INTEGER NOT NULL,
               source_start_ms INTEGER NOT NULL,
               source_end_ms INTEGER NOT NULL,
               segment_ids_json TEXT NOT NULL DEFAULT '[]',
               subtitle_zh TEXT NOT NULL DEFAULT '',
               spoken_zh TEXT NOT NULL DEFAULT '',
               duration_budget_ms INTEGER NOT NULL,
               status TEXT NOT NULL DEFAULT 'draft',
               revision INTEGER NOT NULL DEFAULT 1,
               UNIQUE(project_id, ordinal),
               CHECK(source_end_ms > source_start_ms)
             );
             CREATE TABLE IF NOT EXISTS sync_anchors (
               id TEXT PRIMARY KEY,
               project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
               scene_id TEXT NOT NULL REFERENCES narration_scenes(id) ON DELETE CASCADE,
               source_time_ms INTEGER NOT NULL,
               phrase TEXT NOT NULL DEFAULT '',
               kind TEXT NOT NULL,
               priority TEXT NOT NULL,
               tolerance_ms INTEGER NOT NULL,
               confidence REAL NOT NULL DEFAULT 0,
               locked INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS timeline_edits (
               id TEXT PRIMARY KEY,
               project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
               source_start_ms INTEGER NOT NULL,
               source_end_ms INTEGER NOT NULL,
               operation TEXT NOT NULL,
               rate REAL,
               output_duration_ms INTEGER NOT NULL,
               origin TEXT NOT NULL,
               reason TEXT NOT NULL DEFAULT '',
               confidence REAL NOT NULL DEFAULT 0,
               accepted INTEGER NOT NULL DEFAULT 0,
               revision INTEGER NOT NULL DEFAULT 1,
               CHECK(source_end_ms > source_start_ms)
             );
             CREATE TABLE IF NOT EXISTS non_speech_events (
               id TEXT PRIMARY KEY,
               project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
               source_start_ms INTEGER NOT NULL,
               source_end_ms INTEGER NOT NULL,
               kind TEXT NOT NULL,
               label TEXT NOT NULL DEFAULT '',
               confidence REAL NOT NULL DEFAULT 0,
               CHECK(source_end_ms > source_start_ms)
             );",
            )?;
            add_column_if_missing(&self.connection, "jobs", "error_message", "TEXT")?;
            for (column, kind) in [
                ("duration_ms", "INTEGER"),
                ("width", "INTEGER"),
                ("height", "INTEGER"),
                ("artifact_dir", "TEXT"),
                ("workflow_mode", "TEXT NOT NULL DEFAULT 'quick'"),
                ("audio_mode", "TEXT NOT NULL DEFAULT 'duck'"),
                ("translation_provider_id", "TEXT"),
                ("tts_provider_id", "TEXT NOT NULL DEFAULT 'system'"),
                ("tts_voice_id", "TEXT"),
                ("tts_style", "TEXT NOT NULL DEFAULT 'auto'"),
                ("tts_settings_json", "TEXT NOT NULL DEFAULT '{}'"),
                ("tts_director_enabled", "INTEGER NOT NULL DEFAULT 1"),
                ("tts_sync_mode", "TEXT NOT NULL DEFAULT 'strict'"),
                ("tts_settings_revision", "INTEGER NOT NULL DEFAULT 1"),
            ] {
                add_column_if_missing(&self.connection, "projects", column, kind)?;
            }
            for (column, kind) in [
                ("script_doc_json", "TEXT NOT NULL DEFAULT ''"),
                ("script_revision", "INTEGER NOT NULL DEFAULT 1"),
                ("tts_overrides_json", "TEXT NOT NULL DEFAULT '{}'"),
                ("tts_state", "TEXT NOT NULL DEFAULT 'stale'"),
                ("tts_error_message", "TEXT"),
                ("tts_settings_hash", "TEXT"),
                ("tts_duration_ms", "INTEGER"),
            ] {
                add_column_if_missing(&self.connection, "segments", column, kind)?;
            }
            for (column, kind) in [
                ("driver", "TEXT NOT NULL DEFAULT ''"),
                ("revision", "INTEGER NOT NULL DEFAULT 1"),
                ("secret_bundle_ref", "TEXT"),
                // SQLite cannot add a column with CURRENT_TIMESTAMP as the
                // default, so legacy databases use an empty sentinel first.
                ("updated_at", "TEXT NOT NULL DEFAULT ''"),
            ] {
                add_column_if_missing(&self.connection, "provider_profiles", column, kind)?;
            }
            for (column, kind) in [
                ("cache_key", "TEXT"),
                ("metadata_json", "TEXT NOT NULL DEFAULT '{}'"),
                ("created_at", "TEXT NOT NULL DEFAULT ''"),
                ("updated_at", "TEXT NOT NULL DEFAULT ''"),
            ] {
                add_column_if_missing(&self.connection, "artifacts", column, kind)?;
            }
            self.connection.execute_batch(
                "CREATE INDEX IF NOT EXISTS artifact_cache_lookup
                   ON artifacts(project_id, kind, cache_key, status);
                 CREATE INDEX IF NOT EXISTS artifact_segment_lookup
                   ON artifacts(project_id, segment_id, kind);
                 CREATE INDEX IF NOT EXISTS narration_scene_project_lookup
                   ON narration_scenes(project_id, ordinal);
                 CREATE INDEX IF NOT EXISTS sync_anchor_project_lookup
                   ON sync_anchors(project_id, source_time_ms);
                 CREATE INDEX IF NOT EXISTS timeline_edit_project_lookup
                   ON timeline_edits(project_id, source_start_ms);
                 CREATE INDEX IF NOT EXISTS non_speech_project_lookup
                   ON non_speech_events(project_id, source_start_ms);
                 UPDATE provider_profiles
                    SET driver=CASE WHEN driver='' THEN kind ELSE driver END,
                        secret_bundle_ref=COALESCE(secret_bundle_ref, credential_ref),
                        updated_at=CASE WHEN updated_at='' THEN CURRENT_TIMESTAMP ELSE updated_at END;
                 UPDATE artifacts
                    SET created_at=CASE WHEN created_at='' THEN CURRENT_TIMESTAMP ELSE created_at END,
                        updated_at=CASE WHEN updated_at='' THEN CURRENT_TIMESTAMP ELSE updated_at END;",
            )?;

            // Backfill legacy spoken text into the canonical V1 document in
            // Rust so escaping and the exact AST shape stay serde-owned.
            let pending = {
                let mut statement = self
                    .connection
                    .prepare("SELECT id, spoken_zh FROM segments WHERE script_doc_json=''")?;
                let rows = statement.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            for (id, spoken_zh) in pending {
                if spoken_zh.trim().is_empty() {
                    continue;
                }
                let document = ScriptDocumentV1::from_plain_text(spoken_zh, Origin::Translation);
                let json = serde_json::to_string(&document)
                    .map_err(|error| AppError::Validation(error.to_string()))?;
                self.connection.execute(
                    "UPDATE segments SET script_doc_json=?2 WHERE id=?1 AND script_doc_json=''",
                    params![id, json],
                )?;
            }
            self.connection.execute_batch(
                "INSERT OR IGNORE INTO app_migrations(version) VALUES (2);
                 INSERT OR IGNORE INTO app_migrations(version) VALUES (3);
                 INSERT OR IGNORE INTO app_migrations(version) VALUES (4);",
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.connection.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(error) => {
                let _ = self.connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn replace_localization_analysis(
        &self,
        project_id: &str,
        scenes: &[NarrationScene],
        anchors: &[SyncAnchor],
        edits: &[TimelineEdit],
        events: &[NonSpeechEvent],
    ) -> Result<(), AppError> {
        self.connection
            .execute_batch("SAVEPOINT replace_localization_analysis")?;
        let result = (|| -> Result<(), AppError> {
            self.connection.execute(
                "DELETE FROM sync_anchors WHERE project_id=?1",
                params![project_id],
            )?;
            self.connection.execute(
                "DELETE FROM narration_scenes WHERE project_id=?1",
                params![project_id],
            )?;
            self.connection.execute(
                "DELETE FROM timeline_edits WHERE project_id=?1",
                params![project_id],
            )?;
            self.connection.execute(
                "DELETE FROM non_speech_events WHERE project_id=?1",
                params![project_id],
            )?;
            for scene in scenes {
                self.connection.execute(
                    "INSERT INTO narration_scenes
                     (id, project_id, ordinal, source_start_ms, source_end_ms, segment_ids_json,
                      subtitle_zh, spoken_zh, duration_budget_ms, status, revision)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![
                        scene.id,
                        project_id,
                        scene.ordinal,
                        scene.source_start_ms,
                        scene.source_end_ms,
                        serde_json::to_string(&scene.segment_ids)
                            .map_err(|error| AppError::Validation(error.to_string()))?,
                        scene.subtitle_zh,
                        scene.spoken_zh,
                        scene.duration_budget_ms,
                        scene.status,
                        scene.revision as i64,
                    ],
                )?;
            }
            for anchor in anchors {
                self.connection.execute(
                    "INSERT INTO sync_anchors
                     (id, project_id, scene_id, source_time_ms, phrase, kind, priority,
                      tolerance_ms, confidence, locked)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![
                        anchor.id,
                        project_id,
                        anchor.scene_id,
                        anchor.source_time_ms,
                        anchor.phrase,
                        anchor.kind,
                        anchor.priority,
                        anchor.tolerance_ms,
                        anchor.confidence,
                        anchor.locked,
                    ],
                )?;
            }
            for edit in edits {
                self.connection.execute(
                    "INSERT INTO timeline_edits
                     (id, project_id, source_start_ms, source_end_ms, operation, rate,
                      output_duration_ms, origin, reason, confidence, accepted, revision)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    params![
                        edit.id,
                        project_id,
                        edit.source_start_ms,
                        edit.source_end_ms,
                        edit.operation,
                        edit.rate,
                        edit.output_duration_ms,
                        edit.origin,
                        edit.reason,
                        edit.confidence,
                        edit.accepted,
                        edit.revision as i64,
                    ],
                )?;
            }
            for event in events {
                self.connection.execute(
                    "INSERT INTO non_speech_events
                     (id, project_id, source_start_ms, source_end_ms, kind, label, confidence)
                     VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![
                        event.id,
                        project_id,
                        event.source_start_ms,
                        event.source_end_ms,
                        event.kind,
                        event.label,
                        event.confidence,
                    ],
                )?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.connection
                    .execute_batch("RELEASE replace_localization_analysis")?;
                Ok(())
            }
            Err(error) => {
                let _ = self
                    .connection
                    .execute_batch("ROLLBACK TO replace_localization_analysis; RELEASE replace_localization_analysis");
                Err(error)
            }
        }
    }

    pub fn list_narration_scenes(&self, project_id: &str) -> Result<Vec<NarrationScene>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, ordinal, source_start_ms, source_end_ms, segment_ids_json,
                    subtitle_zh, spoken_zh, duration_budget_ms, status, revision
             FROM narration_scenes WHERE project_id=?1 ORDER BY ordinal",
        )?;
        let rows = statement.query_map(params![project_id], |row| {
            let ids_json: String = row.get(5)?;
            Ok(NarrationScene {
                id: row.get(0)?,
                project_id: row.get(1)?,
                ordinal: row.get(2)?,
                source_start_ms: row.get(3)?,
                source_end_ms: row.get(4)?,
                segment_ids: serde_json::from_str(&ids_json).unwrap_or_default(),
                subtitle_zh: row.get(6)?,
                spoken_zh: row.get(7)?,
                duration_budget_ms: row.get(8)?,
                status: row.get(9)?,
                revision: row.get::<_, i64>(10)?.max(1) as u64,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn list_sync_anchors(&self, project_id: &str) -> Result<Vec<SyncAnchor>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, scene_id, source_time_ms, phrase, kind, priority,
                    tolerance_ms, confidence, locked
             FROM sync_anchors WHERE project_id=?1 ORDER BY source_time_ms",
        )?;
        let rows = statement.query_map(params![project_id], |row| {
            Ok(SyncAnchor {
                id: row.get(0)?,
                project_id: row.get(1)?,
                scene_id: row.get(2)?,
                source_time_ms: row.get(3)?,
                phrase: row.get(4)?,
                kind: row.get(5)?,
                priority: row.get(6)?,
                tolerance_ms: row.get(7)?,
                confidence: row.get(8)?,
                locked: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn list_timeline_edits(&self, project_id: &str) -> Result<Vec<TimelineEdit>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, source_start_ms, source_end_ms, operation, rate,
                    output_duration_ms, origin, reason, confidence, accepted, revision
             FROM timeline_edits WHERE project_id=?1 ORDER BY source_start_ms",
        )?;
        let rows = statement.query_map(params![project_id], |row| {
            Ok(TimelineEdit {
                id: row.get(0)?,
                project_id: row.get(1)?,
                source_start_ms: row.get(2)?,
                source_end_ms: row.get(3)?,
                operation: row.get(4)?,
                rate: row.get(5)?,
                output_duration_ms: row.get(6)?,
                origin: row.get(7)?,
                reason: row.get(8)?,
                confidence: row.get(9)?,
                accepted: row.get(10)?,
                revision: row.get::<_, i64>(11)?.max(1) as u64,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn list_non_speech_events(
        &self,
        project_id: &str,
    ) -> Result<Vec<NonSpeechEvent>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, source_start_ms, source_end_ms, kind, label, confidence
             FROM non_speech_events WHERE project_id=?1 ORDER BY source_start_ms",
        )?;
        let rows = statement.query_map(params![project_id], |row| {
            Ok(NonSpeechEvent {
                id: row.get(0)?,
                project_id: row.get(1)?,
                source_start_ms: row.get(2)?,
                source_end_ms: row.get(3)?,
                kind: row.get(4)?,
                label: row.get(5)?,
                confidence: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn set_timeline_edit_accepted(
        &self,
        project_id: &str,
        edit_id: &str,
        accepted: bool,
    ) -> Result<TimelineEdit, AppError> {
        let changed = self.connection.execute(
            "UPDATE timeline_edits SET accepted=?3, revision=revision+1
             WHERE project_id=?1 AND id=?2",
            params![project_id, edit_id, accepted],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound(edit_id.into()));
        }
        self.list_timeline_edits(project_id)?
            .into_iter()
            .find(|edit| edit.id == edit_id)
            .ok_or_else(|| AppError::NotFound(edit_id.into()))
    }

    pub fn create_project(&self, id: &str, name: &str) -> Result<ProjectSummary, AppError> {
        let clean = name.trim();
        if clean.is_empty() {
            return Err(AppError::Validation("project name cannot be empty".into()));
        }
        self.connection.execute(
            "INSERT INTO projects (id, name, status, progress, tts_voice_id, tts_sync_mode)
             VALUES (?1, ?2, 'draft', 0, 'Tingting', 'balanced')",
            params![id, clean],
        )?;
        let now = current_timestamp(&self.connection)?;
        Ok(ProjectSummary {
            id: id.into(),
            name: clean.into(),
            status: ProjectStatus::Draft,
            progress: 0,
            source_path: None,
            source_fingerprint: None,
            duration_ms: None,
            width: None,
            height: None,
            artifact_dir: None,
            workflow_mode: "quick".into(),
            audio_mode: "duck".into(),
            translation_provider_id: None,
            tts_provider_id: "system".into(),
            tts_voice_id: Some("Tingting".into()),
            tts_style: "auto".into(),
            tts_settings_json: "{}".into(),
            tts_director_enabled: true,
            tts_sync_mode: "balanced".into(),
            tts_settings_revision: 1,
            segment_count: 0,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>, AppError> {
        let mut statement = self
            .connection
            .prepare("SELECT id, name, status, progress, source_path, source_fingerprint, duration_ms, width, height, artifact_dir, workflow_mode, audio_mode, translation_provider_id, tts_provider_id, tts_voice_id, tts_style, tts_settings_json, tts_director_enabled, tts_sync_mode, tts_settings_revision, created_at, updated_at, (SELECT COUNT(*) FROM segments WHERE segments.project_id = projects.id) FROM projects ORDER BY updated_at DESC")?;
        let rows = statement.query_map([], |row| {
            Ok(ProjectSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                status: parse_project_status(&row.get::<_, String>(2)?),
                progress: row.get::<_, i64>(3)?.clamp(0, 100) as u8,
                source_path: row.get(4)?,
                source_fingerprint: row.get(5)?,
                duration_ms: row.get(6)?,
                width: row
                    .get::<_, Option<i64>>(7)?
                    .map(|value| value.max(0) as u32),
                height: row
                    .get::<_, Option<i64>>(8)?
                    .map(|value| value.max(0) as u32),
                artifact_dir: row.get(9)?,
                workflow_mode: row.get(10)?,
                audio_mode: row.get(11)?,
                translation_provider_id: row.get(12)?,
                tts_provider_id: row.get(13)?,
                tts_voice_id: row.get(14)?,
                tts_style: row.get(15)?,
                tts_settings_json: row.get(16)?,
                tts_director_enabled: row.get(17)?,
                tts_sync_mode: row.get(18)?,
                tts_settings_revision: row.get::<_, i64>(19)?.max(1) as u64,
                segment_count: row.get::<_, i64>(22)?.max(0) as u32,
                created_at: row.get(20)?,
                updated_at: row.get(21)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn rename_project(&self, project_id: &str, name: &str) -> Result<ProjectSummary, AppError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 120 {
            return Err(AppError::Validation("项目名称需为 1–120 个字符".into()));
        }
        let updated = self.connection.execute(
            "UPDATE projects SET name=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![project_id, name],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(project_id.into()));
        }
        self.get_project(project_id)
    }

    pub fn delete_project(&self, project_id: &str) -> Result<(), AppError> {
        let running: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM jobs WHERE project_id=?1 AND status='running'",
            [project_id],
            |row| row.get(0),
        )?;
        if running > 0 {
            return Err(AppError::Validation(
                "项目仍有运行中的任务，请先取消任务再删除项目".into(),
            ));
        }
        let deleted = self
            .connection
            .execute("DELETE FROM projects WHERE id=?1", [project_id])?;
        if deleted == 0 {
            return Err(AppError::NotFound(project_id.into()));
        }
        Ok(())
    }

    pub fn attach_media(
        &self,
        project_id: &str,
        probe: &crate::domain::MediaProbe,
    ) -> Result<ProjectSummary, AppError> {
        let updated = self.connection.execute(
            "UPDATE projects SET name=?2, source_path=?3, source_fingerprint=?4, duration_ms=?5, width=?6, height=?7, updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![project_id, project_name(&probe.file_name), probe.source_path, probe.fingerprint, probe.duration_ms, probe.width, probe.height],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(project_id.into()));
        }
        self.get_project(project_id)
    }

    pub fn set_artifact_dir(&self, project_id: &str, artifact_dir: &str) -> Result<(), AppError> {
        let updated = self.connection.execute(
            "UPDATE projects SET artifact_dir=?2, progress=15, status='processing', updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![project_id, artifact_dir],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(project_id.into()));
        }
        Ok(())
    }

    pub fn configure_project(
        &self,
        project_id: &str,
        workflow_mode: &str,
        audio_mode: &str,
        translation_provider_id: Option<&str>,
    ) -> Result<(), AppError> {
        if !matches!(workflow_mode, "quick" | "review") {
            return Err(AppError::Validation("未知的处理模式".into()));
        }
        if !matches!(audio_mode, "duck" | "mute" | "separate") {
            return Err(AppError::Validation("未知的原声处理模式".into()));
        }
        let updated = self.connection.execute(
            "UPDATE projects SET workflow_mode=?2, audio_mode=?3, translation_provider_id=?4, updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![project_id, workflow_mode, audio_mode, translation_provider_id],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(project_id.into()));
        }
        Ok(())
    }

    pub fn set_project_translation_provider(
        &self,
        project_id: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let updated = self.connection.execute(
            "UPDATE projects SET translation_provider_id=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![project_id, provider_id],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(project_id.into()));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_project_tts_defaults(
        &self,
        project_id: &str,
        provider_id: &str,
        voice_id: Option<&str>,
        style: &str,
        settings_json: &str,
        director_enabled: bool,
        sync_mode: &str,
    ) -> Result<ProjectSummary, AppError> {
        validate_json_object(settings_json, "TTS 项目设置")?;
        if !matches!(sync_mode, "strict" | "balanced" | "narration" | "semantic") {
            return Err(AppError::Validation("未知的配音同步模式".into()));
        }
        let previous_provider_id = self
            .connection
            .query_row(
                "SELECT tts_provider_id FROM projects WHERE id=?1",
                [project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let updated = self.connection.execute(
            "UPDATE projects SET tts_provider_id=?2, tts_voice_id=?3, tts_style=?4,
             tts_settings_json=?5, tts_director_enabled=?6, tts_sync_mode=?7,
             tts_settings_revision=tts_settings_revision+1,
             updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![
                project_id,
                provider_id,
                voice_id,
                style,
                settings_json,
                director_enabled,
                sync_mode
            ],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(project_id.into()));
        }
        if previous_provider_id.as_deref() != Some(provider_id) {
            // A voice identifier belongs to a provider-specific namespace.
            // Preserve delivery/style overrides, but never carry an Aliyun
            // voice such as `Cherry` into an iFLYTEK request (or vice versa).
            self.connection.execute(
                "UPDATE segments
                 SET tts_overrides_json=json_remove(tts_overrides_json, '$.voiceId', '$.voice_id')
                 WHERE project_id=?1",
                [project_id],
            )?;
        }
        self.connection.execute(
            "UPDATE segments SET tts_state='stale', tts_error_message=NULL
             WHERE project_id=?1 AND tts_state != 'missing'",
            [project_id],
        )?;
        self.get_project(project_id)
    }

    pub fn get_project(&self, id: &str) -> Result<ProjectSummary, AppError> {
        self.connection.query_row(
            "SELECT id, name, status, progress, source_path, source_fingerprint, duration_ms, width, height, artifact_dir, workflow_mode, audio_mode, translation_provider_id, tts_provider_id, tts_voice_id, tts_style, tts_settings_json, tts_director_enabled, tts_sync_mode, tts_settings_revision, created_at, updated_at, (SELECT COUNT(*) FROM segments WHERE segments.project_id = projects.id) FROM projects WHERE id=?1",
            [id], |row| Ok(ProjectSummary {
                id: row.get(0)?, name: row.get(1)?, status: parse_project_status(&row.get::<_, String>(2)?),
                progress: row.get::<_, i64>(3)?.clamp(0,100) as u8, source_path: row.get(4)?, source_fingerprint: row.get(5)?, duration_ms: row.get(6)?,
                width: row.get::<_, Option<i64>>(7)?.map(|value| value.max(0) as u32), height: row.get::<_, Option<i64>>(8)?.map(|value| value.max(0) as u32),
                artifact_dir: row.get(9)?, workflow_mode: row.get(10)?, audio_mode: row.get(11)?, translation_provider_id: row.get(12)?, tts_provider_id: row.get(13)?,
                tts_voice_id: row.get(14)?, tts_style: row.get(15)?, tts_settings_json: row.get(16)?, tts_director_enabled: row.get(17)?,
                tts_sync_mode: row.get(18)?, tts_settings_revision: row.get::<_, i64>(19)?.max(1) as u64,
                segment_count: row.get::<_, i64>(22)?.max(0) as u32, created_at: row.get(20)?, updated_at: row.get(21)?,
            }),
        ).optional()?.ok_or_else(|| AppError::NotFound(id.into()))
    }

    pub fn upsert_segment(&self, segment: &SegmentRecord) -> Result<(), AppError> {
        if segment.end_ms - segment.start_ms < 300 {
            return Err(AppError::Validation(
                "segment must be at least 300ms".into(),
            ));
        }
        let overlap: Option<String> = self.connection.query_row(
            "SELECT id FROM segments WHERE project_id = ?1 AND id != ?2 AND start_ms < ?4 AND end_ms > ?3 LIMIT 1",
            params![segment.project_id, segment.id, segment.start_ms, segment.end_ms],
            |row| row.get(0),
        ).optional()?;
        if overlap.is_some() {
            return Err(AppError::Validation(
                "segment overlaps another segment".into(),
            ));
        }
        let script_doc_json = canonical_script_json(&segment.script_doc_json, &segment.spoken_zh)?;
        validate_json_object(&segment.tts_overrides_json, "片段 TTS 覆盖设置")?;
        self.connection.execute(
            "INSERT INTO segments (id, project_id, ordinal, start_ms, end_ms, source_text, subtitle_zh, spoken_zh, linked, status,
              script_doc_json, script_revision, tts_overrides_json, tts_state, tts_error_message, tts_settings_hash, tts_duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(id) DO UPDATE SET ordinal=excluded.ordinal, start_ms=excluded.start_ms, end_ms=excluded.end_ms,
             source_text=excluded.source_text, subtitle_zh=excluded.subtitle_zh, spoken_zh=excluded.spoken_zh,
             linked=excluded.linked, status=excluded.status, script_doc_json=excluded.script_doc_json,
             script_revision=excluded.script_revision, tts_overrides_json=excluded.tts_overrides_json,
             tts_state=excluded.tts_state, tts_error_message=excluded.tts_error_message,
             tts_settings_hash=excluded.tts_settings_hash, tts_duration_ms=excluded.tts_duration_ms",
            params![segment.id, segment.project_id, segment.ordinal, segment.start_ms, segment.end_ms,
                segment.source_text, segment.subtitle_zh, segment.spoken_zh, segment.linked, segment.status,
                script_doc_json, segment.script_revision.max(1), segment.tts_overrides_json,
                segment.tts_state, segment.tts_error_message, segment.tts_settings_hash, segment.tts_duration_ms],
        )?;
        Ok(())
    }

    pub fn replace_asr_segments(
        &mut self,
        project_id: &str,
        segments: &[SegmentRecord],
    ) -> Result<(), AppError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM segments WHERE project_id=?1", [project_id])?;
        for segment in segments {
            let script_doc_json =
                canonical_script_json(&segment.script_doc_json, &segment.spoken_zh)?;
            transaction.execute(
                "INSERT INTO segments (id, project_id, ordinal, start_ms, end_ms, source_text, subtitle_zh, spoken_zh, linked, status, script_doc_json, script_revision, tts_overrides_json, tts_state) VALUES (?1,?2,?3,?4,?5,?6,'','',1,'ready',?7,1,'{}','missing')",
                params![segment.id, segment.project_id, segment.ordinal, segment.start_ms, segment.end_ms, segment.source_text, script_doc_json],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn replace_project_segments(
        &mut self,
        project_id: &str,
        segments: &[SegmentRecord],
    ) -> Result<(), AppError> {
        let mut previous_end = 0;
        for segment in segments {
            if segment.project_id != project_id {
                return Err(AppError::Validation("片段不属于当前项目".into()));
            }
            if segment.end_ms - segment.start_ms < 300 {
                return Err(AppError::Validation("片段最短为 300ms".into()));
            }
            if segment.start_ms < previous_end {
                return Err(AppError::Validation("片段时间不能重叠".into()));
            }
            previous_end = segment.end_ms;
        }
        let transaction = self.connection.transaction()?;
        let existing = {
            let mut statement = transaction.prepare(
                "SELECT id, start_ms, end_ms, spoken_zh, script_doc_json, script_revision,
                 tts_overrides_json, tts_state, tts_error_message, tts_settings_hash,
                 tts_duration_ms FROM segments WHERE project_id=?1",
            )?;
            let rows = statement.query_map([project_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?.max(1) as u64,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, Option<i64>>(10)?,
                    ),
                ))
            })?;
            rows.collect::<Result<std::collections::HashMap<_, _>, _>>()?
        };
        // Move existing ordinals out of the non-negative editor range first.
        // This permits reordering two retained IDs without transiently
        // violating UNIQUE(project_id, ordinal).
        transaction.execute(
            "UPDATE segments SET ordinal=-ordinal-1 WHERE project_id=?1",
            [project_id],
        )?;
        for (ordinal, segment) in segments.iter().enumerate() {
            let script_doc_json =
                canonical_script_json(&segment.script_doc_json, &segment.spoken_zh)?;
            validate_json_object(&segment.tts_overrides_json, "片段 TTS 覆盖设置")?;
            let (script_revision, tts_state, tts_error_message, tts_settings_hash, tts_duration_ms) =
                if let Some((
                    old_start,
                    old_end,
                    old_spoken,
                    old_script,
                    old_revision,
                    old_overrides,
                    old_tts_state,
                    old_error,
                    old_hash,
                    old_duration,
                )) = existing.get(&segment.id)
                {
                    let audio_changed = *old_start != segment.start_ms
                        || *old_end != segment.end_ms
                        || old_spoken != &segment.spoken_zh
                        || old_script != &script_doc_json
                        || old_overrides != &segment.tts_overrides_json;
                    if audio_changed {
                        (
                            old_revision.saturating_add(1).max(segment.script_revision),
                            "stale".to_string(),
                            None,
                            None,
                            None,
                        )
                    } else {
                        (
                            (*old_revision).max(segment.script_revision),
                            old_tts_state.clone(),
                            old_error.clone(),
                            old_hash.clone(),
                            *old_duration,
                        )
                    }
                } else {
                    (
                        segment.script_revision.max(1),
                        if segment.spoken_zh.trim().is_empty() {
                            "missing"
                        } else {
                            "stale"
                        }
                        .to_string(),
                        None,
                        None,
                        None,
                    )
                };
            transaction.execute(
                "INSERT INTO segments (id, project_id, ordinal, start_ms, end_ms, source_text, subtitle_zh, spoken_zh, linked, status, script_doc_json, script_revision, tts_overrides_json, tts_state, tts_error_message, tts_settings_hash, tts_duration_ms)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
                 ON CONFLICT(id) DO UPDATE SET ordinal=excluded.ordinal, start_ms=excluded.start_ms, end_ms=excluded.end_ms,
                   source_text=excluded.source_text, subtitle_zh=excluded.subtitle_zh, spoken_zh=excluded.spoken_zh,
                   linked=excluded.linked, status=excluded.status, script_doc_json=excluded.script_doc_json,
                   script_revision=excluded.script_revision, tts_overrides_json=excluded.tts_overrides_json,
                   tts_state=excluded.tts_state, tts_error_message=excluded.tts_error_message,
                   tts_settings_hash=excluded.tts_settings_hash, tts_duration_ms=excluded.tts_duration_ms",
                params![segment.id, project_id, ordinal as i64, segment.start_ms, segment.end_ms, segment.source_text,
                  segment.subtitle_zh, segment.spoken_zh, segment.linked, segment.status, script_doc_json,
                  script_revision, segment.tts_overrides_json, tts_state,
                  tts_error_message, tts_settings_hash, tts_duration_ms],
            )?;
        }
        // Difference-based replacement preserves rows and their artifact FKs.
        // IDs absent from the submitted snapshot are removed deliberately.
        if segments.is_empty() {
            transaction.execute("DELETE FROM segments WHERE project_id=?1", [project_id])?;
        } else {
            let placeholders = std::iter::repeat_n("?", segments.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql =
                format!("DELETE FROM segments WHERE project_id=?1 AND id NOT IN ({placeholders})");
            let mut values = Vec::<rusqlite::types::Value>::with_capacity(segments.len() + 1);
            values.push(project_id.to_string().into());
            values.extend(segments.iter().map(|segment| segment.id.clone().into()));
            transaction.execute(&sql, rusqlite::params_from_iter(values))?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_segments(&self, project_id: &str) -> Result<Vec<SegmentRecord>, AppError> {
        let mut statement = self.connection.prepare("SELECT id, project_id, ordinal, start_ms, end_ms, source_text, subtitle_zh, spoken_zh, linked, status, script_doc_json, script_revision, tts_overrides_json, tts_state, tts_error_message, tts_settings_hash, tts_duration_ms FROM segments WHERE project_id=?1 ORDER BY ordinal")?;
        let rows = statement.query_map([project_id], |row| {
            Ok(SegmentRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                ordinal: row.get(2)?,
                start_ms: row.get(3)?,
                end_ms: row.get(4)?,
                source_text: row.get(5)?,
                subtitle_zh: row.get(6)?,
                spoken_zh: row.get(7)?,
                linked: row.get(8)?,
                status: row.get(9)?,
                script_doc_json: row.get(10)?,
                script_revision: row.get::<_, i64>(11)?.max(1) as u64,
                tts_overrides_json: row.get(12)?,
                tts_state: row.get(13)?,
                tts_error_message: row.get(14)?,
                tts_settings_hash: row.get(15)?,
                tts_duration_ms: row.get(16)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn update_segment_translation(
        &self,
        segment_id: &str,
        subtitle_zh: &str,
        spoken_zh: &str,
    ) -> Result<(), AppError> {
        let script_doc_json = canonical_script_json("", spoken_zh)?;
        let updated = self.connection.execute(
            "UPDATE segments SET subtitle_zh=?2, spoken_zh=?3, script_doc_json=?4,
             script_revision=script_revision+1, tts_state='stale', tts_error_message=NULL,
             status='ready' WHERE id=?1",
            params![segment_id, subtitle_zh, spoken_zh, script_doc_json],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(segment_id.into()));
        }
        Ok(())
    }

    pub fn clear_project_translations(&self, project_id: &str) -> Result<(), AppError> {
        self.connection.execute(
            "UPDATE segments SET subtitle_zh='', spoken_zh='', script_doc_json='',
             script_revision=script_revision+1, tts_state='missing', tts_error_message=NULL,
             linked=1, status='asr_ready' WHERE project_id=?1",
            [project_id],
        )?;
        Ok(())
    }

    pub fn set_segment_status(&self, segment_id: &str, status: &str) -> Result<(), AppError> {
        let updated = self.connection.execute(
            "UPDATE segments SET status=?2 WHERE id=?1",
            params![segment_id, status],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(segment_id.into()));
        }
        Ok(())
    }

    pub fn update_segment_spoken(&self, segment_id: &str, spoken_zh: &str) -> Result<(), AppError> {
        let script_doc_json = canonical_script_json("", spoken_zh)?;
        let updated = self.connection.execute(
            "UPDATE segments SET spoken_zh=?2, script_doc_json=?3,
             script_revision=script_revision+1, tts_state='stale', tts_error_message=NULL,
             linked=0, status='ready' WHERE id=?1",
            params![segment_id, spoken_zh, script_doc_json],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(segment_id.into()));
        }
        Ok(())
    }

    pub fn restore_segment_spoken_snapshot(&self, segment: &SegmentRecord) -> Result<(), AppError> {
        let updated = self.connection.execute(
            "UPDATE segments SET spoken_zh=?2, script_doc_json=?3, script_revision=?4,
             tts_state='stale', tts_error_message=NULL, tts_settings_hash=NULL,
             tts_duration_ms=NULL, linked=?5, status=?6 WHERE id=?1",
            params![
                segment.id,
                segment.spoken_zh,
                segment.script_doc_json,
                segment.script_revision,
                segment.linked as i64,
                segment.status,
            ],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(segment.id.clone()));
        }
        Ok(())
    }

    pub fn update_segment_script(
        &self,
        segment_id: &str,
        script_doc_json: &str,
        expected_revision: u64,
        tts_overrides_json: &str,
    ) -> Result<SegmentRecord, AppError> {
        let document: ScriptDocumentV1 = serde_json::from_str(script_doc_json)
            .map_err(|_| AppError::Validation("口播稿格式无效".into()))?;
        document.validate()?;
        validate_json_object(tts_overrides_json, "片段 TTS 覆盖设置")?;
        let canonical = serde_json::to_string(&document)
            .map_err(|error| AppError::Validation(error.to_string()))?;
        let updated = self.connection.execute(
            "UPDATE segments SET script_doc_json=?2, spoken_zh=?3,
             tts_overrides_json=?4, script_revision=script_revision+1,
             tts_state='stale', tts_error_message=NULL
             WHERE id=?1 AND script_revision=?5",
            params![
                segment_id,
                canonical,
                document.plain_text(),
                tts_overrides_json,
                expected_revision
            ],
        )?;
        if updated == 0 {
            let exists: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM segments WHERE id=?1)",
                [segment_id],
                |row| row.get(0),
            )?;
            return if exists {
                Err(AppError::Validation("口播稿已被更新，请刷新后重试".into()))
            } else {
                Err(AppError::NotFound(segment_id.into()))
            };
        }
        self.get_segment(segment_id)
    }

    pub fn get_segment(&self, segment_id: &str) -> Result<SegmentRecord, AppError> {
        let project_id: String = self
            .connection
            .query_row(
                "SELECT project_id FROM segments WHERE id=?1",
                [segment_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(segment_id.into()))?;
        self.list_segments(&project_id)?
            .into_iter()
            .find(|segment| segment.id == segment_id)
            .ok_or_else(|| AppError::NotFound(segment_id.into()))
    }

    #[allow(dead_code)]
    pub fn set_segment_tts_state(
        &self,
        segment_id: &str,
        state: &str,
        settings_hash: Option<&str>,
        duration_ms: Option<i64>,
        error_message: Option<&str>,
    ) -> Result<(), AppError> {
        if !matches!(
            state,
            "missing" | "stale" | "queued" | "synthesizing" | "ready" | "failed"
        ) {
            return Err(AppError::Validation("未知的片段 TTS 状态".into()));
        }
        let updated = self.connection.execute(
            "UPDATE segments SET tts_state=?2, tts_settings_hash=?3,
             tts_duration_ms=?4, tts_error_message=?5 WHERE id=?1",
            params![segment_id, state, settings_hash, duration_ms, error_message],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(segment_id.into()));
        }
        Ok(())
    }

    pub fn capture_tts_publish_snapshot(
        &self,
        project_id: &str,
        provider_revision: u64,
    ) -> Result<TtsPublishSnapshot, AppError> {
        let project = self.get_project(project_id)?;
        Ok(TtsPublishSnapshot {
            project_id: project.id.clone(),
            project_tts_revision: project.tts_settings_revision,
            provider_id: project.tts_provider_id,
            voice_id: project.tts_voice_id,
            style: project.tts_style,
            settings_json: project.tts_settings_json,
            director_enabled: project.tts_director_enabled,
            provider_revision,
            segments: self
                .list_segments(project_id)?
                .into_iter()
                .map(|segment| TtsSegmentSnapshot {
                    id: segment.id,
                    ordinal: segment.ordinal,
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                    script_revision: segment.script_revision,
                    spoken_zh: segment.spoken_zh,
                    script_doc_json: segment.script_doc_json,
                    tts_overrides_json: segment.tts_overrides_json,
                    tts_state: segment.tts_state,
                    tts_settings_hash: segment.tts_settings_hash,
                })
                .collect(),
        })
    }

    pub fn validate_tts_publish_snapshot(
        &self,
        job_id: &str,
        snapshot: &TtsPublishSnapshot,
    ) -> Result<(), AppError> {
        let job = self.get_job(job_id)?;
        if job.project_id != snapshot.project_id || job.status != JobStatus::Running {
            return Err(AppError::Validation(
                "配音任务已取消或不再运行，未发布本次结果".into(),
            ));
        }
        let project = self.get_project(&snapshot.project_id)?;
        if project.tts_settings_revision != snapshot.project_tts_revision
            || project.tts_provider_id != snapshot.provider_id
            || project.tts_voice_id != snapshot.voice_id
            || project.tts_style != snapshot.style
            || project.tts_settings_json != snapshot.settings_json
            || project.tts_director_enabled != snapshot.director_enabled
        {
            return Err(AppError::Validation(
                "配音设置已变化，未发布过期的合成结果".into(),
            ));
        }
        if snapshot.provider_id != "system" {
            let provider = self.get_provider(&snapshot.provider_id)?;
            if provider.revision != snapshot.provider_revision {
                return Err(AppError::Validation(
                    "语音服务配置已变化，未发布过期的合成结果".into(),
                ));
            }
        }
        let current = self
            .list_segments(&snapshot.project_id)?
            .into_iter()
            .map(|segment| TtsSegmentSnapshot {
                id: segment.id,
                ordinal: segment.ordinal,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                script_revision: segment.script_revision,
                spoken_zh: segment.spoken_zh,
                script_doc_json: segment.script_doc_json,
                tts_overrides_json: segment.tts_overrides_json,
                tts_state: segment.tts_state,
                tts_settings_hash: segment.tts_settings_hash,
            })
            .collect::<Vec<_>>();
        if current != snapshot.segments {
            return Err(AppError::Validation(
                "口播稿或时间轴已变化，未发布过期的合成结果".into(),
            ));
        }
        Ok(())
    }

    pub fn commit_tts_publication(
        &self,
        job_id: &str,
        snapshot: &TtsPublishSnapshot,
        segment_updates: &[TtsSegmentPublication],
        artifacts: &[ArtifactRecord],
    ) -> Result<(), AppError> {
        for update in segment_updates {
            if !matches!(update.state.as_str(), "ready" | "stale" | "failed") {
                return Err(AppError::Validation("未知的片段 TTS 发布状态".into()));
            }
        }
        for artifact in artifacts {
            validate_json_object(&artifact.metadata_json, "产物元数据")?;
        }
        self.validate_tts_publish_snapshot(job_id, snapshot)?;
        self.connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<(), AppError> {
            // Re-check the mutable CAS inputs inside the transaction. The app-level
            // database mutex keeps the preceding file publish and this commit one
            // indivisible publication boundary for all commands in this process.
            self.validate_tts_publish_snapshot(job_id, snapshot)?;
            for update in segment_updates {
                let changed = self.connection.execute(
                    "UPDATE segments SET tts_state=?2, tts_settings_hash=?3,
                     tts_duration_ms=?4, tts_error_message=?5, status=?6
                     WHERE id=?1 AND project_id=?7 AND script_revision=?8",
                    params![
                        update.segment_id,
                        update.state,
                        update.settings_hash,
                        update.duration_ms,
                        update.error_message,
                        update.display_status,
                        snapshot.project_id,
                        update.expected_script_revision
                    ],
                )?;
                if changed != 1 {
                    return Err(AppError::Validation(
                        "口播稿已变化，未发布过期的合成结果".into(),
                    ));
                }
            }
            for artifact in artifacts {
                self.connection.execute(
                    "INSERT INTO artifacts(id, project_id, segment_id, kind, path, content_hash,
                      dependency_hash, cache_key, revision, status, metadata_json, created_at, updated_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)
                     ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,
                      segment_id=excluded.segment_id, kind=excluded.kind, path=excluded.path,
                      content_hash=excluded.content_hash, dependency_hash=excluded.dependency_hash,
                      cache_key=excluded.cache_key, revision=artifacts.revision+1,
                      status=excluded.status, metadata_json=excluded.metadata_json,
                      updated_at=CURRENT_TIMESTAMP",
                    params![
                        artifact.id,
                        artifact.project_id,
                        artifact.segment_id,
                        artifact.kind,
                        artifact.path,
                        artifact.content_hash,
                        artifact.dependency_hash,
                        artifact.cache_key,
                        artifact.revision.max(1),
                        artifact.status,
                        artifact.metadata_json
                    ],
                )?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.connection.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(error) => {
                let _ = self.connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn list_providers(&self) -> Result<Vec<ProviderProfile>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, kind, name, public_config_json, credential_ref, driver,
             revision, secret_bundle_ref, updated_at FROM provider_profiles ORDER BY name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ProviderProfile {
                id: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                public_config_json: row.get(3)?,
                credential_ref: row.get(4)?,
                driver: row.get(5)?,
                revision: row.get::<_, i64>(6)?.max(1) as u64,
                secret_bundle_ref: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn get_provider(&self, id: &str) -> Result<ProviderProfile, AppError> {
        self.connection
            .query_row(
                "SELECT id, kind, name, public_config_json, credential_ref, driver,
                 revision, secret_bundle_ref, updated_at FROM provider_profiles WHERE id=?1",
                [id],
                |row| {
                    Ok(ProviderProfile {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        name: row.get(2)?,
                        public_config_json: row.get(3)?,
                        credential_ref: row.get(4)?,
                        driver: row.get(5)?,
                        revision: row.get::<_, i64>(6)?.max(1) as u64,
                        secret_bundle_ref: row.get(7)?,
                        updated_at: row.get(8)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(id.into()))
    }

    pub fn save_provider(&self, profile: &ProviderProfile) -> Result<(), AppError> {
        validate_json_object(&profile.public_config_json, "服务商公开配置")?;
        let driver = if profile.driver.trim().is_empty() {
            profile.kind.as_str()
        } else {
            profile.driver.as_str()
        };
        let bundle_ref = profile
            .secret_bundle_ref
            .as_ref()
            .or(profile.credential_ref.as_ref());
        self.connection.execute(
            "INSERT INTO provider_profiles(id, kind, name, public_config_json, credential_ref,
              driver, revision, secret_bundle_ref, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, CURRENT_TIMESTAMP)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, name=excluded.name,
              public_config_json=excluded.public_config_json, credential_ref=excluded.credential_ref,
              driver=excluded.driver, revision=provider_profiles.revision+1,
              secret_bundle_ref=excluded.secret_bundle_ref, updated_at=CURRENT_TIMESTAMP",
            params![profile.id, profile.kind, profile.name, profile.public_config_json,
              profile.credential_ref, driver, profile.revision.max(1), bundle_ref],
        )?;
        Ok(())
    }

    pub fn remove_provider(&self, id: &str) -> Result<Option<String>, AppError> {
        let reference: Option<String> = self
            .connection
            .query_row(
                "SELECT COALESCE(secret_bundle_ref, credential_ref) FROM provider_profiles WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let updated = self
            .connection
            .execute("DELETE FROM provider_profiles WHERE id=?1", [id])?;
        if updated == 0 {
            return Err(AppError::NotFound(id.into()));
        }
        Ok(reference)
    }

    #[allow(dead_code)]
    pub fn upsert_artifact(&self, artifact: &ArtifactRecord) -> Result<ArtifactRecord, AppError> {
        validate_json_object(&artifact.metadata_json, "产物元数据")?;
        self.connection.execute(
            "INSERT INTO artifacts(id, project_id, segment_id, kind, path, content_hash,
              dependency_hash, cache_key, revision, status, metadata_json, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)
             ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,
              segment_id=excluded.segment_id, kind=excluded.kind, path=excluded.path,
              content_hash=excluded.content_hash, dependency_hash=excluded.dependency_hash,
              cache_key=excluded.cache_key, revision=artifacts.revision+1,
              status=excluded.status, metadata_json=excluded.metadata_json,
              updated_at=CURRENT_TIMESTAMP",
            params![
                artifact.id,
                artifact.project_id,
                artifact.segment_id,
                artifact.kind,
                artifact.path,
                artifact.content_hash,
                artifact.dependency_hash,
                artifact.cache_key,
                artifact.revision.max(1),
                artifact.status,
                artifact.metadata_json
            ],
        )?;
        self.get_artifact(&artifact.id)
    }

    pub fn get_artifact(&self, id: &str) -> Result<ArtifactRecord, AppError> {
        self.connection
            .query_row(
                "SELECT id, project_id, segment_id, kind, path, content_hash,
             dependency_hash, cache_key, revision, status, metadata_json, created_at, updated_at
             FROM artifacts WHERE id=?1",
                [id],
                artifact_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(id.into()))
    }

    #[allow(dead_code)]
    pub fn list_artifacts(
        &self,
        project_id: &str,
        segment_id: Option<&str>,
        kind: Option<&str>,
    ) -> Result<Vec<ArtifactRecord>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, segment_id, kind, path, content_hash,
             dependency_hash, cache_key, revision, status, metadata_json, created_at, updated_at
             FROM artifacts WHERE project_id=?1
               AND (?2 IS NULL OR segment_id=?2)
               AND (?3 IS NULL OR kind=?3)
             ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map(params![project_id, segment_id, kind], artifact_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    #[allow(dead_code)]
    pub fn find_cached_artifact(
        &self,
        project_id: &str,
        kind: &str,
        cache_key: &str,
    ) -> Result<Option<ArtifactRecord>, AppError> {
        self.connection
            .query_row(
                "SELECT id, project_id, segment_id, kind, path, content_hash,
             dependency_hash, cache_key, revision, status, metadata_json, created_at, updated_at
             FROM artifacts WHERE project_id=?1 AND kind=?2 AND cache_key=?3 AND status='ready'
             ORDER BY updated_at DESC LIMIT 1",
                params![project_id, kind, cache_key],
                artifact_from_row,
            )
            .optional()
            .map_err(AppError::from)
    }

    #[allow(dead_code)]
    pub fn set_artifact_status(&self, id: &str, status: &str) -> Result<(), AppError> {
        let updated = self.connection.execute(
            "UPDATE artifacts SET status=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![id, status],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(id.into()));
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn delete_artifact(&self, id: &str) -> Result<(), AppError> {
        let updated = self
            .connection
            .execute("DELETE FROM artifacts WHERE id=?1", [id])?;
        if updated == 0 {
            return Err(AppError::NotFound(id.into()));
        }
        Ok(())
    }

    pub fn list_glossary_terms(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<GlossaryTerm>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, source, target, policy, enabled FROM glossary_terms
             WHERE project_id IS NULL OR (?1 IS NOT NULL AND project_id=?1)
             ORDER BY CASE WHEN project_id=?1 THEN 0 ELSE 1 END, source COLLATE NOCASE",
        )?;
        let rows = statement.query_map([project_id], |row| {
            Ok(GlossaryTerm {
                id: row.get(0)?,
                project_id: row.get(1)?,
                source: row.get(2)?,
                target: row.get(3)?,
                policy: row.get(4)?,
                enabled: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn save_glossary_term(&self, term: &GlossaryTerm) -> Result<(), AppError> {
        if term.source.trim().is_empty() || term.target.trim().is_empty() {
            return Err(AppError::Validation("术语原文和译文不能为空".into()));
        }
        self.connection.execute(
            "INSERT INTO glossary_terms(id, project_id, source, target, policy, enabled)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,
              source=excluded.source, target=excluded.target, policy=excluded.policy,
              enabled=excluded.enabled",
            params![
                term.id,
                term.project_id,
                term.source.trim(),
                term.target.trim(),
                term.policy,
                term.enabled
            ],
        )?;
        Ok(())
    }

    pub fn delete_glossary_term(&self, id: &str) -> Result<(), AppError> {
        let updated = self
            .connection
            .execute("DELETE FROM glossary_terms WHERE id=?1", [id])?;
        if updated == 0 {
            return Err(AppError::NotFound(id.into()));
        }
        Ok(())
    }

    pub fn enqueue_job(&self, job: &JobSummary) -> Result<(), AppError> {
        self.connection.execute(
            "INSERT INTO jobs (id, project_id, stage, progress, status, checkpoint) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![job.id, job.project_id, job.stage, job.progress, job_status(&job.status), job.checkpoint],
        )?;
        self.connection.execute(
            "UPDATE projects SET status='processing', updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            [&job.project_id],
        )?;
        Ok(())
    }

    pub fn list_jobs(&self) -> Result<Vec<JobSummary>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, stage, progress, status, checkpoint, error_message, created_at, updated_at
             FROM jobs ORDER BY CASE status WHEN 'running' THEN 0 WHEN 'waiting_user' THEN 1 WHEN 'queued' THEN 2 ELSE 3 END, created_at ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(JobSummary {
                id: row.get(0)?,
                project_id: row.get(1)?,
                stage: row.get(2)?,
                progress: row.get::<_, i64>(3)?.clamp(0, 100) as u8,
                status: parse_job_status(&row.get::<_, String>(4)?),
                checkpoint: row.get(5)?,
                error_message: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn delete_job(&self, id: &str) -> Result<(), AppError> {
        let current = self.get_job(id)?;
        if current.status == JobStatus::Running {
            return Err(AppError::Validation(
                "运行中的任务不能直接删除，请先取消任务".into(),
            ));
        }
        let deleted = self
            .connection
            .execute("DELETE FROM jobs WHERE id=?1", [id])?;
        if deleted == 0 {
            return Err(AppError::NotFound(id.into()));
        }
        Ok(())
    }

    pub fn start_job(&self, id: &str) -> Result<(), AppError> {
        let running: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM jobs WHERE status='running'",
            [],
            |row| row.get(0),
        )?;
        if running > 0 {
            return Err(AppError::Validation(
                "another heavy job is already running".into(),
            ));
        }
        let updated = self.connection.execute(
            "UPDATE jobs SET status='running', updated_at=CURRENT_TIMESTAMP WHERE id=?1 AND status IN ('queued','paused','failed')",
            [id],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(id.into()));
        }
        self.sync_project_from_job(id, JobStatus::Running)?;
        Ok(())
    }

    /// Makes a TTS job runnable regardless of whether it is being resumed,
    /// retried after a provider warning, or regenerated after a completed
    /// export. This keeps the command boundary from depending on UI-specific
    /// transition choreography.
    pub fn prepare_tts_job(&self, id: &str) -> Result<(), AppError> {
        let current = self.get_job(id)?;
        match current.status {
            JobStatus::Succeeded => self.reopen_job(id, "tts", 63, "tts:started")?,
            JobStatus::Paused | JobStatus::Failed | JobStatus::WaitingUser => {
                self.transition_job(id, JobStatus::Queued)?;
                self.connection.execute(
                    "UPDATE jobs SET stage='tts', progress=63, checkpoint='tts:started', error_message=NULL, updated_at=CURRENT_TIMESTAMP WHERE id=?1 AND status='queued'",
                    [id],
                )?;
            }
            JobStatus::Queued => {}
            JobStatus::Running => {
                return Err(AppError::Validation("中文配音任务已在运行".into()));
            }
            JobStatus::Cancelled => {
                return Err(AppError::Validation(
                    "已取消的任务不能重新生成配音，请新建任务".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn handoff_running_job(
        &self,
        id: &str,
        stage: &str,
        progress: u8,
        checkpoint: &str,
    ) -> Result<(), AppError> {
        let updated = self.connection.execute(
            "UPDATE jobs SET status='queued', stage=?2, progress=?3, checkpoint=?4, error_message=NULL, updated_at=CURRENT_TIMESTAMP WHERE id=?1 AND status='running'",
            params![id, stage, progress, checkpoint],
        )?;
        if updated == 0 {
            return Err(AppError::Validation("任务阶段交接时状态已变化".into()));
        }
        Ok(())
    }

    pub fn reopen_job(
        &self,
        id: &str,
        stage: &str,
        progress: u8,
        checkpoint: &str,
    ) -> Result<(), AppError> {
        let updated = self.connection.execute(
            "UPDATE jobs SET status='queued', stage=?2, progress=?3, checkpoint=?4, error_message=NULL, updated_at=CURRENT_TIMESTAMP WHERE id=?1 AND status='succeeded'",
            params![id, stage, progress.min(100), checkpoint],
        )?;
        if updated == 0 {
            return Err(AppError::Validation(
                "只有已完成任务可以重新进入处理".into(),
            ));
        }
        self.sync_project_from_job(id, JobStatus::Queued)?;
        Ok(())
    }

    pub fn checkpoint_job(
        &self,
        id: &str,
        stage: &str,
        progress: u8,
        checkpoint: &str,
    ) -> Result<(), AppError> {
        let updated = self.connection.execute(
            "UPDATE jobs SET stage=?2, progress=MAX(progress, ?3), checkpoint=?4, updated_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND status='running'",
            params![id, stage, progress.min(100), checkpoint],
        )?;
        if updated == 0 {
            let exists: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM jobs WHERE id=?1)",
                [id],
                |row| row.get(0),
            )?;
            return if exists {
                Err(AppError::Validation("只有运行中的任务可以更新进度".into()))
            } else {
                Err(AppError::NotFound(id.into()))
            };
        }
        self.connection.execute(
            "UPDATE projects SET progress=MAX(progress, ?2), status='processing', updated_at=CURRENT_TIMESTAMP
             WHERE id=(SELECT project_id FROM jobs WHERE id=?1 AND status='running')",
            params![id, progress.min(100)],
        )?;
        Ok(())
    }

    pub fn transition_job(&self, id: &str, target: JobStatus) -> Result<JobSummary, AppError> {
        let allowed = match target {
            JobStatus::Paused => "running",
            JobStatus::Queued => "paused,failed,waiting_user",
            JobStatus::Cancelled => "queued,running,paused,waiting_user,failed",
            JobStatus::Succeeded => "running,waiting_user",
            JobStatus::Failed => "running",
            JobStatus::WaitingUser => "running",
            JobStatus::Running => {
                return Err(AppError::Validation("use job_start to run a job".into()))
            }
        };
        let current: Option<String> = self
            .connection
            .query_row("SELECT status FROM jobs WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        let current = current.ok_or_else(|| AppError::NotFound(id.into()))?;
        if !allowed.split(',').any(|status| status == current) {
            return Err(AppError::Validation(format!(
                "cannot transition job from {current} to {}",
                job_status(&target)
            )));
        }
        self.connection.execute(
            "UPDATE jobs SET status=?2, error_message=CASE WHEN ?2='failed' THEN COALESCE(error_message, '任务失败，可安全重试') ELSE NULL END, updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![id, job_status(&target)],
        )?;
        self.sync_project_from_job(id, target)?;
        self.get_job(id)
    }

    pub fn requeue_completed_job(&self, id: &str) -> Result<JobSummary, AppError> {
        let updated = self.connection.execute(
            "UPDATE jobs SET status='queued', progress=0, checkpoint=NULL, error_message=NULL, updated_at=CURRENT_TIMESTAMP WHERE id=?1 AND status='succeeded'",
            [id],
        )?;
        if updated == 0 {
            return Err(AppError::Validation(
                "只有已完成任务可以重新加入队列".into(),
            ));
        }
        self.get_job(id)
    }

    pub fn fail_job(&self, id: &str, message: &str) -> Result<JobSummary, AppError> {
        let current: Option<String> = self
            .connection
            .query_row("SELECT status FROM jobs WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        if current.as_deref() != Some("running") {
            return Err(AppError::Validation("only a running job can fail".into()));
        }
        self.connection.execute(
            "UPDATE jobs SET status='failed', error_message=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![id, message],
        )?;
        self.sync_project_from_job(id, JobStatus::Failed)?;
        self.get_job(id)
    }

    pub fn get_job(&self, id: &str) -> Result<JobSummary, AppError> {
        self.connection.query_row(
            "SELECT id, project_id, stage, progress, status, checkpoint, error_message, created_at, updated_at FROM jobs WHERE id=?1",
            [id],
            |row| Ok(JobSummary {
                id: row.get(0)?, project_id: row.get(1)?, stage: row.get(2)?,
                progress: row.get::<_, i64>(3)?.clamp(0, 100) as u8,
                status: parse_job_status(&row.get::<_, String>(4)?), checkpoint: row.get(5)?,
                error_message: row.get(6)?, created_at: row.get(7)?, updated_at: row.get(8)?,
            }),
        ).optional()?.ok_or_else(|| AppError::NotFound(id.into()))
    }

    fn sync_project_from_job(&self, id: &str, status: JobStatus) -> Result<(), AppError> {
        let project_status = match status {
            JobStatus::Running | JobStatus::Queued | JobStatus::Paused => "processing",
            JobStatus::WaitingUser => "waiting_user",
            JobStatus::Succeeded => "ready",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "draft",
        };
        self.connection.execute(
            "UPDATE projects SET status=?2, progress=CASE WHEN ?2='ready' THEN 100 ELSE progress END, updated_at=CURRENT_TIMESTAMP
             WHERE id=(SELECT project_id FROM jobs WHERE id=?1)",
            params![id, project_status],
        )?;
        Ok(())
    }

    fn recover_interrupted_jobs(&self) -> Result<(), AppError> {
        self.connection.execute(
            "UPDATE jobs SET status='paused', updated_at=CURRENT_TIMESTAMP WHERE status='running'",
            [],
        )?;
        Ok(())
    }
}

fn current_timestamp(connection: &Connection) -> Result<String, AppError> {
    connection
        .query_row("SELECT CURRENT_TIMESTAMP", [], |row| row.get(0))
        .map_err(AppError::from)
}

fn project_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(file_name)
        .to_string()
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, AppError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    kind: &str,
) -> Result<(), AppError> {
    if !has_column(connection, table, column)? {
        connection.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"),
            [],
        )?;
    }
    Ok(())
}

fn canonical_script_json(raw: &str, spoken_zh: &str) -> Result<String, AppError> {
    let document = if raw.trim().is_empty() {
        ScriptDocumentV1::from_plain_text(spoken_zh, Origin::Translation)
    } else {
        serde_json::from_str::<ScriptDocumentV1>(raw)
            .map_err(|_| AppError::Validation("口播稿格式无效".into()))?
    };
    if document.plain_text().trim().is_empty() && spoken_zh.trim().is_empty() {
        return Ok(String::new());
    }
    document.validate()?;
    serde_json::to_string(&document).map_err(|error| AppError::Validation(error.to_string()))
}

fn validate_json_object(raw: &str, label: &str) -> Result<(), AppError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|_| AppError::Validation(format!("{label}必须是有效 JSON")))?;
    if !value.is_object() {
        return Err(AppError::Validation(format!("{label}必须是 JSON 对象")));
    }
    Ok(())
}

fn artifact_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArtifactRecord> {
    Ok(ArtifactRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        segment_id: row.get(2)?,
        kind: row.get(3)?,
        path: row.get(4)?,
        content_hash: row.get(5)?,
        dependency_hash: row.get(6)?,
        cache_key: row.get(7)?,
        revision: row.get::<_, i64>(8)?.max(1) as u64,
        status: row.get(9)?,
        metadata_json: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn parse_project_status(value: &str) -> ProjectStatus {
    match value {
        "processing" => ProjectStatus::Processing,
        "waiting_user" => ProjectStatus::WaitingUser,
        "ready" => ProjectStatus::Ready,
        "failed" => ProjectStatus::Failed,
        _ => ProjectStatus::Draft,
    }
}

fn job_status(value: &JobStatus) -> &'static str {
    match value {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::WaitingUser => "waiting_user",
        JobStatus::Paused => "paused",
        JobStatus::Succeeded => "succeeded",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
    }
}

fn parse_job_status(value: &str) -> JobStatus {
    match value {
        "running" => JobStatus::Running,
        "waiting_user" => JobStatus::WaitingUser,
        "paused" => JobStatus::Paused,
        "succeeded" => JobStatus::Succeeded,
        "failed" => JobStatus::Failed,
        "cancelled" => JobStatus::Cancelled,
        _ => JobStatus::Queued,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ArtifactRecord, JobStatus, NarrationScene, NonSpeechEvent, ProviderProfile, SegmentRecord,
        SyncAnchor, TimelineEdit,
    };

    fn segment(id: &str, ordinal: i64, start_ms: i64, end_ms: i64) -> SegmentRecord {
        SegmentRecord {
            id: id.into(),
            project_id: "p1".into(),
            ordinal,
            start_ms,
            end_ms,
            source_text: "source".into(),
            subtitle_zh: "字幕".into(),
            spoken_zh: "配音".into(),
            linked: true,
            status: "ready".into(),
            script_doc_json: String::new(),
            script_revision: 1,
            tts_overrides_json: "{}".into(),
            tts_state: "stale".into(),
            tts_error_message: None,
            tts_settings_hash: None,
            tts_duration_ms: None,
        }
    }

    #[test]
    fn project_and_non_overlapping_segments_round_trip() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        for (id, ordinal, start, end) in [("s1", 0, 0, 1000), ("s2", 1, 1000, 2000)] {
            db.upsert_segment(&SegmentRecord {
                id: id.into(),
                project_id: "p1".into(),
                ordinal,
                start_ms: start,
                end_ms: end,
                source_text: "source".into(),
                subtitle_zh: "字幕".into(),
                spoken_zh: "配音".into(),
                linked: true,
                status: "ready".into(),
                script_doc_json: String::new(),
                script_revision: 1,
                tts_overrides_json: "{}".into(),
                tts_state: "stale".into(),
                tts_error_message: None,
                tts_settings_hash: None,
                tts_duration_ms: None,
            })
            .unwrap();
        }
        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].segment_count, 2);
        assert_eq!(db.get_project("p1").unwrap().segment_count, 2);
    }

    #[test]
    fn localization_analysis_round_trips_with_accepted_timeline_edit() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        let scene = NarrationScene {
            id: "scene-1".into(),
            project_id: "p1".into(),
            ordinal: 0,
            source_start_ms: 0,
            source_end_ms: 10_000,
            segment_ids: vec!["s1".into()],
            subtitle_zh: "字幕".into(),
            spoken_zh: "口播".into(),
            duration_budget_ms: 9_500,
            status: "ready".into(),
            revision: 1,
        };
        let anchor = SyncAnchor {
            id: "anchor-1".into(),
            project_id: "p1".into(),
            scene_id: scene.id.clone(),
            source_time_ms: 1_000,
            phrase: "打开页面".into(),
            kind: "action".into(),
            priority: "exact".into(),
            tolerance_ms: 500,
            confidence: 0.9,
            locked: false,
        };
        let edit = TimelineEdit {
            id: "edit-1".into(),
            project_id: "p1".into(),
            source_start_ms: 4_000,
            source_end_ms: 6_000,
            operation: "cut".into(),
            rate: None,
            output_duration_ms: 0,
            origin: "automatic".into(),
            reason: "静止画面".into(),
            confidence: 0.92,
            accepted: false,
            revision: 1,
        };
        let event = NonSpeechEvent {
            id: "event-1".into(),
            project_id: "p1".into(),
            source_start_ms: 7_000,
            source_end_ms: 8_000,
            kind: "music".into(),
            label: "音乐".into(),
            confidence: 1.0,
        };
        db.replace_localization_analysis(
            "p1",
            std::slice::from_ref(&scene),
            std::slice::from_ref(&anchor),
            std::slice::from_ref(&edit),
            std::slice::from_ref(&event),
        )
        .unwrap();
        assert_eq!(db.list_narration_scenes("p1").unwrap(), vec![scene]);
        assert_eq!(db.list_sync_anchors("p1").unwrap(), vec![anchor]);
        assert_eq!(db.list_non_speech_events("p1").unwrap(), vec![event]);
        let accepted = db.set_timeline_edit_accepted("p1", "edit-1", true).unwrap();
        assert!(accepted.accepted);
        assert_eq!(accepted.revision, 2);
    }

    #[test]
    fn listing_a_large_project_stays_within_the_editor_navigation_budget() {
        let db = Database::memory().unwrap();
        db.create_project("large", "Large course").unwrap();
        for ordinal in 0..136 {
            let start_ms = ordinal * 1_000;
            db.upsert_segment(&SegmentRecord {
                id: format!("segment-{ordinal}"),
                project_id: "large".into(),
                ordinal,
                start_ms,
                end_ms: start_ms + 900,
                source_text: "source".into(),
                subtitle_zh: "字幕".into(),
                spoken_zh: "配音".into(),
                linked: true,
                status: "ready".into(),
                script_doc_json: String::new(),
                script_revision: 1,
                tts_overrides_json: "{}".into(),
                tts_state: "stale".into(),
                tts_error_message: None,
                tts_settings_hash: None,
                tts_duration_ms: None,
            })
            .unwrap();
        }
        let started = std::time::Instant::now();
        let segments = db.list_segments("large").unwrap();
        assert_eq!(segments.len(), 136);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(100),
            "136 segment read exceeded the 100ms navigation budget"
        );
    }

    #[test]
    fn overlapping_segment_is_rejected() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        let segment = |id: &str, ordinal, start_ms, end_ms| SegmentRecord {
            id: id.into(),
            project_id: "p1".into(),
            ordinal,
            start_ms,
            end_ms,
            source_text: "s".into(),
            subtitle_zh: "z".into(),
            spoken_zh: "v".into(),
            linked: true,
            status: "ready".into(),
            script_doc_json: String::new(),
            script_revision: 1,
            tts_overrides_json: "{}".into(),
            tts_state: "stale".into(),
            tts_error_message: None,
            tts_settings_hash: None,
            tts_duration_ms: None,
        };
        db.upsert_segment(&segment("s1", 0, 0, 1000)).unwrap();
        assert!(db.upsert_segment(&segment("s2", 1, 900, 1500)).is_err());
    }

    #[test]
    fn only_one_heavy_job_can_run() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        for id in ["j1", "j2"] {
            db.enqueue_job(&JobSummary {
                id: id.into(),
                project_id: "p1".into(),
                stage: "media_check".into(),
                progress: 0,
                status: JobStatus::Queued,
                checkpoint: None,
                error_message: None,
                created_at: String::new(),
                updated_at: String::new(),
            })
            .unwrap();
        }
        db.start_job("j1").unwrap();
        assert!(db.start_job("j2").is_err());
        db.checkpoint_job("j1", "proxy", 20, "artifact:proxy")
            .unwrap();
    }

    #[test]
    fn job_transitions_are_persisted_and_listed() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        db.enqueue_job(&JobSummary {
            id: "j1".into(),
            project_id: "p1".into(),
            stage: "media_check".into(),
            progress: 0,
            status: JobStatus::Queued,
            checkpoint: None,
            error_message: None,
            created_at: String::new(),
            updated_at: String::new(),
        })
        .unwrap();
        db.start_job("j1").unwrap();
        db.checkpoint_job("j1", "asr", 35, "word:420").unwrap();
        db.transition_job("j1", JobStatus::Paused).unwrap();
        let job = db.list_jobs().unwrap().remove(0);
        assert_eq!(job.status, JobStatus::Paused);
        assert_eq!(job.progress, 35);
        assert_eq!(job.checkpoint.as_deref(), Some("word:420"));
        db.transition_job("j1", JobStatus::Queued).unwrap();
        db.start_job("j1").unwrap();
    }

    #[test]
    fn running_job_checkpoints_never_move_progress_backwards() {
        let db = Database::memory().unwrap();
        db.create_project("p-progress", "Progress").unwrap();
        db.enqueue_job(&JobSummary {
            id: "j-progress".into(),
            project_id: "p-progress".into(),
            stage: "translation".into(),
            progress: 0,
            status: JobStatus::Queued,
            checkpoint: Some("queued".into()),
            error_message: None,
            created_at: String::new(),
            updated_at: String::new(),
        })
        .unwrap();
        db.start_job("j-progress").unwrap();
        db.checkpoint_job("j-progress", "script_director", 65, "director:complete")
            .unwrap();
        db.checkpoint_job("j-progress", "tts", 57, "tts:started")
            .unwrap();

        assert_eq!(db.get_job("j-progress").unwrap().progress, 65);
        assert_eq!(db.get_project("p-progress").unwrap().progress, 65);
        assert_eq!(db.get_job("j-progress").unwrap().stage, "tts");
    }

    #[test]
    fn project_rename_and_safe_deletion_manage_dependent_jobs() {
        let database = Database::memory().unwrap();
        database.create_project("p-delete", "源文件").unwrap();
        let renamed = database
            .rename_project("p-delete", "第一版中文配音")
            .unwrap();
        assert_eq!(renamed.name, "第一版中文配音");

        let job = JobSummary {
            id: "j-delete".into(),
            project_id: "p-delete".into(),
            stage: "asr".into(),
            progress: 0,
            status: JobStatus::Queued,
            checkpoint: None,
            error_message: None,
            created_at: String::new(),
            updated_at: String::new(),
        };
        database.enqueue_job(&job).unwrap();
        database.start_job("j-delete").unwrap();
        assert!(database
            .delete_job("j-delete")
            .unwrap_err()
            .to_string()
            .contains("运行中"));
        assert!(database
            .delete_project("p-delete")
            .unwrap_err()
            .to_string()
            .contains("运行中"));

        database
            .transition_job("j-delete", JobStatus::Cancelled)
            .unwrap();
        database.delete_project("p-delete").unwrap();
        assert!(database.get_project("p-delete").is_err());
        assert!(
            database.get_job("j-delete").is_err(),
            "project deletion must cascade to jobs"
        );
    }

    #[test]
    fn tts_job_can_be_prepared_again_after_success() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        db.enqueue_job(&JobSummary {
            id: "j1".into(),
            project_id: "p1".into(),
            stage: "tts".into(),
            progress: 80,
            status: JobStatus::Queued,
            checkpoint: None,
            error_message: None,
            created_at: String::new(),
            updated_at: String::new(),
        })
        .unwrap();
        db.start_job("j1").unwrap();
        db.transition_job("j1", JobStatus::Succeeded).unwrap();

        db.prepare_tts_job("j1").unwrap();
        assert_eq!(db.get_job("j1").unwrap().status, JobStatus::Queued);
        db.start_job("j1").unwrap();
        assert_eq!(db.get_job("j1").unwrap().status, JobStatus::Running);
    }

    #[test]
    fn interrupted_job_recovers_as_paused() {
        let directory = std::env::temp_dir().join(format!("yisheng-db-{}", uuid::Uuid::new_v4()));
        let path = directory.join("app.db");
        {
            let db = Database::open(&path).unwrap();
            db.create_project("p1", "Course").unwrap();
            db.enqueue_job(&JobSummary {
                id: "j1".into(),
                project_id: "p1".into(),
                stage: "asr".into(),
                progress: 41,
                status: JobStatus::Queued,
                checkpoint: Some("word:900".into()),
                error_message: None,
                created_at: String::new(),
                updated_at: String::new(),
            })
            .unwrap();
            db.start_job("j1").unwrap();
        }
        let recovered = Database::open(&path).unwrap().get_job("j1").unwrap();
        assert_eq!(recovered.status, JobStatus::Paused);
        assert_eq!(recovered.checkpoint.as_deref(), Some("word:900"));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn v3_schema_and_script_backfill_are_idempotent() {
        let directory = std::env::temp_dir().join(format!("yisheng-v3-{}", uuid::Uuid::new_v4()));
        let path = directory.join("app.db");
        std::fs::create_dir_all(&directory).unwrap();
        {
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(
                "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, status TEXT NOT NULL, progress INTEGER NOT NULL DEFAULT 0, tts_provider_id TEXT NOT NULL DEFAULT 'system', created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                 CREATE TABLE segments (id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, ordinal INTEGER NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, source_text TEXT NOT NULL, subtitle_zh TEXT NOT NULL, spoken_zh TEXT NOT NULL, linked INTEGER NOT NULL DEFAULT 1, status TEXT NOT NULL DEFAULT 'ready', UNIQUE(project_id, ordinal));
                 INSERT INTO projects(id,name,status) VALUES ('p1','Legacy','draft');
                 INSERT INTO segments(id,project_id,ordinal,start_ms,end_ms,source_text,subtitle_zh,spoken_zh) VALUES ('s1','p1',0,0,1000,'hello','你好','你好');"
            ).unwrap();
        }
        let db = Database::open(&path).unwrap();
        let migrated = db.get_segment("s1").unwrap();
        let document: ScriptDocumentV1 = serde_json::from_str(&migrated.script_doc_json).unwrap();
        assert_eq!(document.plain_text(), "你好");
        drop(db);
        let reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.get_segment("s1").unwrap().script_revision, 1);
        let versions: i64 = reopened
            .connection
            .query_row(
                "SELECT COUNT(*) FROM app_migrations WHERE version=3",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(versions, 1);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn script_revision_and_project_settings_make_audio_stale() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        db.upsert_segment(&segment("s1", 0, 0, 1_000)).unwrap();
        let original = db.get_segment("s1").unwrap();
        let document = ScriptDocumentV1::from_plain_text("新的口播稿", Origin::Manual);
        let updated = db
            .update_segment_script(
                "s1",
                &serde_json::to_string(&document).unwrap(),
                original.script_revision,
                "{}",
            )
            .unwrap();
        assert_eq!(updated.spoken_zh, "新的口播稿");
        assert_eq!(updated.tts_state, "stale");
        assert_eq!(updated.script_revision, original.script_revision + 1);
        assert!(db
            .update_segment_script(
                "s1",
                &serde_json::to_string(&document).unwrap(),
                original.script_revision,
                "{}"
            )
            .is_err());
        db.set_segment_tts_state("s1", "ready", Some("hash-1"), Some(980), None)
            .unwrap();
        let project = db
            .set_project_tts_defaults(
                "p1",
                "aliyun",
                Some("Cherry"),
                "professional",
                "{}",
                true,
                "balanced",
            )
            .unwrap();
        assert_eq!(project.tts_settings_revision, 2);
        assert_eq!(db.get_segment("s1").unwrap().tts_state, "stale");
    }

    #[test]
    fn changing_tts_provider_clears_only_provider_specific_voice_overrides() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        db.upsert_segment(&segment("s1", 0, 0, 1_000)).unwrap();
        let original = db.get_segment("s1").unwrap();
        let document = ScriptDocumentV1::from_plain_text("讲解稿", Origin::Manual);
        db.update_segment_script(
            "s1",
            &serde_json::to_string(&document).unwrap(),
            original.script_revision,
            r#"{"voiceId":"Cherry","style":"professional","speed":1.02,"directorEnabled":true}"#,
        )
        .unwrap();

        db.set_project_tts_defaults(
            "p1",
            "iflytek-super-tts",
            Some("x6_lingxiaoxuan_pro"),
            "professional",
            "{}",
            true,
            "semantic",
        )
        .unwrap();

        let overrides: serde_json::Value =
            serde_json::from_str(&db.get_segment("s1").unwrap().tts_overrides_json).unwrap();
        assert!(overrides.get("voiceId").is_none());
        assert_eq!(overrides["style"], "professional");
        assert_eq!(overrides["speed"], 1.02);
        assert_eq!(overrides["directorEnabled"], true);
    }

    #[test]
    fn artifact_cache_and_provider_bundle_fields_round_trip() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        db.upsert_segment(&segment("s1", 0, 0, 1_000)).unwrap();
        db.save_provider(&ProviderProfile {
            id: "ali".into(),
            kind: "tts".into(),
            name: "阿里".into(),
            public_config_json: "{}".into(),
            credential_ref: None,
            driver: "aliyun_tts".into(),
            revision: 1,
            secret_bundle_ref: Some("provider:bundle".into()),
            updated_at: String::new(),
        })
        .unwrap();
        let profile = db.get_provider("ali").unwrap();
        assert_eq!(profile.driver, "aliyun_tts");
        assert_eq!(
            profile.secret_bundle_ref.as_deref(),
            Some("provider:bundle")
        );
        let artifact = ArtifactRecord {
            id: "a1".into(),
            project_id: "p1".into(),
            segment_id: Some("s1".into()),
            kind: "tts".into(),
            path: "/tmp/a.wav".into(),
            content_hash: "content".into(),
            dependency_hash: "deps".into(),
            cache_key: Some("cache-1".into()),
            revision: 1,
            status: "ready".into(),
            metadata_json: "{}".into(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        db.upsert_artifact(&artifact).unwrap();
        assert_eq!(
            db.find_cached_artifact("p1", "tts", "cache-1")
                .unwrap()
                .unwrap()
                .id,
            "a1"
        );
        db.set_artifact_status("a1", "stale").unwrap();
        assert!(db
            .find_cached_artifact("p1", "tts", "cache-1")
            .unwrap()
            .is_none());
    }

    fn running_tts_job(db: &Database) {
        db.enqueue_job(&JobSummary {
            id: "j-tts".into(),
            project_id: "p1".into(),
            stage: "tts".into(),
            progress: 57,
            status: JobStatus::Queued,
            checkpoint: None,
            error_message: None,
            created_at: String::new(),
            updated_at: String::new(),
        })
        .unwrap();
        db.start_job("j-tts").unwrap();
    }

    #[test]
    fn cancelled_tts_job_cannot_checkpoint_or_publish() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        db.upsert_segment(&segment("s1", 0, 0, 1_000)).unwrap();
        running_tts_job(&db);
        let snapshot = db.capture_tts_publish_snapshot("p1", 1).unwrap();
        db.transition_job("j-tts", JobStatus::Cancelled).unwrap();

        assert!(db.checkpoint_job("j-tts", "tts", 70, "tts:late").is_err());
        assert!(db
            .commit_tts_publication(
                "j-tts",
                &snapshot,
                &[TtsSegmentPublication {
                    segment_id: "s1".into(),
                    expected_script_revision: 1,
                    state: "ready".into(),
                    settings_hash: Some("new-hash".into()),
                    duration_ms: Some(900),
                    error_message: None,
                    display_status: "ready".into(),
                }],
                &[],
            )
            .is_err());
        assert_eq!(db.get_segment("s1").unwrap().tts_state, "stale");
    }

    #[test]
    fn changed_script_rejects_atomic_tts_publication() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        db.upsert_segment(&segment("s1", 0, 0, 1_000)).unwrap();
        running_tts_job(&db);
        let snapshot = db.capture_tts_publish_snapshot("p1", 1).unwrap();
        db.update_segment_spoken("s1", "生成期间修改的口播稿")
            .unwrap();

        let artifact = ArtifactRecord {
            id: "tts-s1-new".into(),
            project_id: "p1".into(),
            segment_id: Some("s1".into()),
            kind: "tts_aligned".into(),
            path: "/tmp/new.wav".into(),
            content_hash: "content".into(),
            dependency_hash: "new-hash".into(),
            cache_key: Some("new-hash".into()),
            revision: 1,
            status: "ready".into(),
            metadata_json: "{}".into(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        assert!(db
            .commit_tts_publication(
                "j-tts",
                &snapshot,
                &[TtsSegmentPublication {
                    segment_id: "s1".into(),
                    expected_script_revision: 1,
                    state: "ready".into(),
                    settings_hash: Some("new-hash".into()),
                    duration_ms: Some(900),
                    error_message: None,
                    display_status: "ready".into(),
                }],
                &[artifact],
            )
            .is_err());
        assert_eq!(db.get_segment("s1").unwrap().tts_state, "stale");
        assert!(db.get_artifact("tts-s1-new").is_err());
    }

    #[test]
    fn tts_segment_and_artifacts_commit_together() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        db.upsert_segment(&segment("s1", 0, 0, 1_000)).unwrap();
        running_tts_job(&db);
        let snapshot = db.capture_tts_publish_snapshot("p1", 1).unwrap();
        let artifact = ArtifactRecord {
            id: "tts-s1-good".into(),
            project_id: "p1".into(),
            segment_id: Some("s1".into()),
            kind: "tts_aligned".into(),
            path: "/tmp/good.wav".into(),
            content_hash: "content".into(),
            dependency_hash: "good-hash".into(),
            cache_key: Some("good-hash".into()),
            revision: 1,
            status: "ready".into(),
            metadata_json: "{}".into(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        db.commit_tts_publication(
            "j-tts",
            &snapshot,
            &[TtsSegmentPublication {
                segment_id: "s1".into(),
                expected_script_revision: 1,
                state: "ready".into(),
                settings_hash: Some("good-hash".into()),
                duration_ms: Some(900),
                error_message: None,
                display_status: "ready".into(),
            }],
            &[artifact],
        )
        .unwrap();
        let saved = db.get_segment("s1").unwrap();
        assert_eq!(saved.tts_state, "ready");
        assert_eq!(saved.tts_settings_hash.as_deref(), Some("good-hash"));
        assert_eq!(db.get_artifact("tts-s1-good").unwrap().status, "ready");
    }

    #[test]
    fn failed_tts_segment_commits_its_diagnostic_without_an_artifact() {
        let db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        db.upsert_segment(&segment("s1", 0, 0, 1_000)).unwrap();
        running_tts_job(&db);
        let snapshot = db.capture_tts_publish_snapshot("p1", 1).unwrap();

        db.commit_tts_publication(
            "j-tts",
            &snapshot,
            &[TtsSegmentPublication {
                segment_id: "s1".into(),
                expected_script_revision: 1,
                state: "failed".into(),
                settings_hash: None,
                duration_ms: None,
                error_message: Some("云端语音服务限流".into()),
                display_status: "warning".into(),
            }],
            &[],
        )
        .unwrap();

        let saved = db.get_segment("s1").unwrap();
        assert_eq!(saved.tts_state, "failed");
        assert_eq!(saved.status, "warning");
        assert_eq!(saved.tts_error_message.as_deref(), Some("云端语音服务限流"));
        assert!(saved.tts_settings_hash.is_none());
        assert!(saved.tts_duration_ms.is_none());
        assert!(db
            .list_artifacts("p1", Some("s1"), Some("tts_aligned"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn difference_replace_preserves_existing_segment_artifacts() {
        let mut db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        let original = segment("s1", 0, 0, 1_000);
        db.upsert_segment(&original).unwrap();
        db.upsert_artifact(&ArtifactRecord {
            id: "a1".into(),
            project_id: "p1".into(),
            segment_id: Some("s1".into()),
            kind: "tts".into(),
            path: "/tmp/a.wav".into(),
            content_hash: "h".into(),
            dependency_hash: "d".into(),
            cache_key: Some("k".into()),
            revision: 1,
            status: "ready".into(),
            metadata_json: "{}".into(),
            created_at: String::new(),
            updated_at: String::new(),
        })
        .unwrap();
        let mut edited = original;
        edited.subtitle_zh = "新字幕".into();
        db.replace_project_segments("p1", &[edited]).unwrap();
        assert_eq!(
            db.get_artifact("a1").unwrap().segment_id.as_deref(),
            Some("s1")
        );
    }

    #[test]
    fn snapshot_replace_cannot_resurrect_stale_audio_or_lower_revision() {
        let mut db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        let original = segment("s1", 0, 0, 1_000);
        db.upsert_segment(&original).unwrap();
        db.set_segment_tts_state("s1", "ready", Some("current-hash"), Some(980), None)
            .unwrap();

        let current = db.get_segment("s1").unwrap();
        let mut forged = current.clone();
        forged.spoken_zh = "攻击者提交的新稿".into();
        forged.script_doc_json = serde_json::to_string(&ScriptDocumentV1::from_plain_text(
            "攻击者提交的新稿",
            Origin::Manual,
        ))
        .unwrap();
        forged.script_revision = 1;
        forged.tts_state = "ready".into();
        forged.tts_settings_hash = Some("current-hash".into());
        forged.tts_duration_ms = Some(980);

        db.replace_project_segments("p1", &[forged]).unwrap();
        let saved = db.get_segment("s1").unwrap();
        assert_eq!(saved.tts_state, "stale");
        assert!(saved.tts_settings_hash.is_none());
        assert!(saved.tts_duration_ms.is_none());
        assert!(saved.script_revision > current.script_revision);
    }

    #[test]
    fn difference_replace_can_swap_retained_segment_ordinals() {
        let mut db = Database::memory().unwrap();
        db.create_project("p1", "Course").unwrap();
        let first = segment("s1", 0, 0, 1_000);
        let second = segment("s2", 1, 1_000, 2_000);
        db.upsert_segment(&first).unwrap();
        db.upsert_segment(&second).unwrap();
        let mut reordered_second = second;
        reordered_second.start_ms = 0;
        reordered_second.end_ms = 1_000;
        let mut reordered_first = first;
        reordered_first.start_ms = 1_000;
        reordered_first.end_ms = 2_000;
        db.replace_project_segments("p1", &[reordered_second, reordered_first])
            .unwrap();
        let ids = db
            .list_segments("p1")
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, ["s2", "s1"]);
    }
}
