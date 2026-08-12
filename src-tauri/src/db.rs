use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    domain::{
        JobStatus, JobSummary, ProjectStatus, ProjectSummary, ProviderProfile, SegmentRecord,
    },
    error::AppError,
};

pub struct Database {
    connection: Connection,
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
        self.connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS projects (
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
               revision INTEGER NOT NULL DEFAULT 1,
               status TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS provider_profiles (
               id TEXT PRIMARY KEY,
               kind TEXT NOT NULL,
               name TEXT NOT NULL,
               public_config_json TEXT NOT NULL,
               credential_ref TEXT
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
             );",
        )?;
        if !has_column(&self.connection, "jobs", "error_message")? {
            self.connection
                .execute("ALTER TABLE jobs ADD COLUMN error_message TEXT", [])?;
        }
        for (column, kind) in [
            ("duration_ms", "INTEGER"),
            ("width", "INTEGER"),
            ("height", "INTEGER"),
            ("artifact_dir", "TEXT"),
            ("workflow_mode", "TEXT NOT NULL DEFAULT 'quick'"),
            ("audio_mode", "TEXT NOT NULL DEFAULT 'duck'"),
            ("translation_provider_id", "TEXT"),
            ("tts_provider_id", "TEXT NOT NULL DEFAULT 'system'"),
        ] {
            if !has_column(&self.connection, "projects", column)? {
                self.connection.execute(
                    &format!("ALTER TABLE projects ADD COLUMN {column} {kind}"),
                    [],
                )?;
            }
        }
        self.connection.execute(
            "INSERT OR IGNORE INTO app_migrations(version) VALUES (2)",
            [],
        )?;
        Ok(())
    }

    pub fn create_project(&self, id: &str, name: &str) -> Result<ProjectSummary, AppError> {
        let clean = name.trim();
        if clean.is_empty() {
            return Err(AppError::Validation("project name cannot be empty".into()));
        }
        self.connection.execute(
            "INSERT INTO projects (id, name, status, progress) VALUES (?1, ?2, 'draft', 0)",
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
            segment_count: 0,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>, AppError> {
        let mut statement = self
            .connection
            .prepare("SELECT id, name, status, progress, source_path, source_fingerprint, duration_ms, width, height, artifact_dir, workflow_mode, audio_mode, translation_provider_id, tts_provider_id, created_at, updated_at, (SELECT COUNT(*) FROM segments WHERE segments.project_id = projects.id) FROM projects ORDER BY updated_at DESC")?;
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
                segment_count: row.get::<_, i64>(16)?.max(0) as u32,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
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

    pub fn get_project(&self, id: &str) -> Result<ProjectSummary, AppError> {
        self.connection.query_row(
            "SELECT id, name, status, progress, source_path, source_fingerprint, duration_ms, width, height, artifact_dir, workflow_mode, audio_mode, translation_provider_id, tts_provider_id, created_at, updated_at, (SELECT COUNT(*) FROM segments WHERE segments.project_id = projects.id) FROM projects WHERE id=?1",
            [id], |row| Ok(ProjectSummary {
                id: row.get(0)?, name: row.get(1)?, status: parse_project_status(&row.get::<_, String>(2)?),
                progress: row.get::<_, i64>(3)?.clamp(0,100) as u8, source_path: row.get(4)?, source_fingerprint: row.get(5)?, duration_ms: row.get(6)?,
                width: row.get::<_, Option<i64>>(7)?.map(|value| value.max(0) as u32), height: row.get::<_, Option<i64>>(8)?.map(|value| value.max(0) as u32),
                artifact_dir: row.get(9)?, workflow_mode: row.get(10)?, audio_mode: row.get(11)?, translation_provider_id: row.get(12)?, tts_provider_id: row.get(13)?,
                segment_count: row.get::<_, i64>(16)?.max(0) as u32, created_at: row.get(14)?, updated_at: row.get(15)?,
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
        self.connection.execute(
            "INSERT INTO segments (id, project_id, ordinal, start_ms, end_ms, source_text, subtitle_zh, spoken_zh, linked, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET ordinal=excluded.ordinal, start_ms=excluded.start_ms, end_ms=excluded.end_ms,
             source_text=excluded.source_text, subtitle_zh=excluded.subtitle_zh, spoken_zh=excluded.spoken_zh,
             linked=excluded.linked, status=excluded.status",
            params![segment.id, segment.project_id, segment.ordinal, segment.start_ms, segment.end_ms,
                segment.source_text, segment.subtitle_zh, segment.spoken_zh, segment.linked, segment.status],
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
            transaction.execute(
                "INSERT INTO segments (id, project_id, ordinal, start_ms, end_ms, source_text, subtitle_zh, spoken_zh, linked, status) VALUES (?1,?2,?3,?4,?5,?6,'','',1,'ready')",
                params![segment.id, segment.project_id, segment.ordinal, segment.start_ms, segment.end_ms, segment.source_text],
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
        transaction.execute("DELETE FROM segments WHERE project_id=?1", [project_id])?;
        for (ordinal, segment) in segments.iter().enumerate() {
            transaction.execute(
                "INSERT INTO segments (id, project_id, ordinal, start_ms, end_ms, source_text, subtitle_zh, spoken_zh, linked, status) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![segment.id, project_id, ordinal as i64, segment.start_ms, segment.end_ms, segment.source_text, segment.subtitle_zh, segment.spoken_zh, segment.linked, segment.status],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_segments(&self, project_id: &str) -> Result<Vec<SegmentRecord>, AppError> {
        let mut statement = self.connection.prepare("SELECT id, project_id, ordinal, start_ms, end_ms, source_text, subtitle_zh, spoken_zh, linked, status FROM segments WHERE project_id=?1 ORDER BY ordinal")?;
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
        let updated = self.connection.execute(
            "UPDATE segments SET subtitle_zh=?2, spoken_zh=?3, status='ready' WHERE id=?1",
            params![segment_id, subtitle_zh, spoken_zh],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(segment_id.into()));
        }
        Ok(())
    }

    pub fn clear_project_translations(&self, project_id: &str) -> Result<(), AppError> {
        self.connection.execute(
            "UPDATE segments SET subtitle_zh='', spoken_zh='', linked=1, status='asr_ready' WHERE project_id=?1",
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

    pub fn set_project_segments_status(
        &self,
        project_id: &str,
        from_status: &str,
        to_status: &str,
    ) -> Result<(), AppError> {
        self.connection.execute(
            "UPDATE segments SET status=?1 WHERE project_id=?2 AND status=?3",
            params![to_status, project_id, from_status],
        )?;
        Ok(())
    }

    pub fn update_segment_spoken(&self, segment_id: &str, spoken_zh: &str) -> Result<(), AppError> {
        let updated = self.connection.execute(
            "UPDATE segments SET spoken_zh=?2, linked=0, status='ready' WHERE id=?1",
            params![segment_id, spoken_zh],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(segment_id.into()));
        }
        Ok(())
    }

    pub fn list_providers(&self) -> Result<Vec<ProviderProfile>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, kind, name, public_config_json, credential_ref FROM provider_profiles ORDER BY name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ProviderProfile {
                id: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                public_config_json: row.get(3)?,
                credential_ref: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn get_provider(&self, id: &str) -> Result<ProviderProfile, AppError> {
        self.connection
            .query_row(
                "SELECT id, kind, name, public_config_json, credential_ref FROM provider_profiles WHERE id=?1",
                [id],
                |row| {
                    Ok(ProviderProfile {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        name: row.get(2)?,
                        public_config_json: row.get(3)?,
                        credential_ref: row.get(4)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(id.into()))
    }

    pub fn save_provider(&self, profile: &ProviderProfile) -> Result<(), AppError> {
        self.connection.execute(
            "INSERT INTO provider_profiles(id, kind, name, public_config_json, credential_ref) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, name=excluded.name, public_config_json=excluded.public_config_json, credential_ref=excluded.credential_ref",
            params![profile.id, profile.kind, profile.name, profile.public_config_json, profile.credential_ref],
        )?;
        Ok(())
    }

    pub fn remove_provider(&self, id: &str) -> Result<Option<String>, AppError> {
        let reference: Option<String> = self
            .connection
            .query_row(
                "SELECT credential_ref FROM provider_profiles WHERE id=?1",
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
            "UPDATE jobs SET stage=?2, progress=?3, checkpoint=?4, updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![id, stage, progress.min(100), checkpoint],
        )?;
        if updated == 0 {
            return Err(AppError::NotFound(id.into()));
        }
        self.connection.execute(
            "UPDATE projects SET progress=?2, status='processing', updated_at=CURRENT_TIMESTAMP
             WHERE id=(SELECT project_id FROM jobs WHERE id=?1)",
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
    use crate::domain::{JobStatus, SegmentRecord};

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
            })
            .unwrap();
        }
        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].segment_count, 2);
        assert_eq!(db.get_project("p1").unwrap().segment_count, 2);
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
}
