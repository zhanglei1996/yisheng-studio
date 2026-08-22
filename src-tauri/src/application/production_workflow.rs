use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::{
    commands,
    error::AppError,
    project_commands,
    workflow::{
        ExecutionContext, FailureClassification, NodeFuture, NodeOutcome, ResourceClass,
        RetryClassification, RetryPolicy, WorkflowDefinition, WorkflowNode, WorkflowNodeSpec,
    },
    AppState,
};

pub(crate) const WORKFLOW_ID: &str = "video_localization";
pub(crate) const WORKFLOW_VERSION: u32 = 4;
pub(crate) const EXPORT_NODE_ID: &str = "export_publish";
pub(crate) const EXPORT_NODE_VERSION: u32 = 1;

#[derive(Clone, Copy)]
enum ProductionStep {
    MediaPrepare,
    Asr,
    TranscriptReview,
    Translation,
    ScriptReview,
    TtsSynthesis,
    AlignmentValidation,
    MixPreview,
    ExportPublish,
}

struct ProductionNode {
    app: AppHandle,
    legacy_job_id: String,
    step: ProductionStep,
    spec: WorkflowNodeSpec,
}

impl WorkflowNode for ProductionNode {
    fn spec(&self) -> WorkflowNodeSpec {
        self.spec.clone()
    }

    fn execute<'a>(&'a self, context: ExecutionContext) -> NodeFuture<'a> {
        Box::pin(async move {
            if context.cancellation.is_cancelled() {
                return NodeOutcome::Cancelled {
                    checkpoint: Some(format!("{}:cancelled", self.spec.id)),
                };
            }
            match self.execute_step(&context).await {
                Ok(outcome) => outcome,
                Err(error) => classify_error(error, &self.spec.id),
            }
        })
    }
}

impl ProductionNode {
    async fn execute_step(&self, context: &ExecutionContext) -> Result<NodeOutcome, AppError> {
        let state = self.app.state::<AppState>();
        match self.step {
            ProductionStep::MediaPrepare => {
                let artifacts = commands::media_prepare(
                    self.app.clone(),
                    state,
                    context.project_id.clone(),
                    self.legacy_job_id.clone(),
                )
                .await?;
                Ok(completed(
                    vec![format!("media:{}", artifacts.artifact_dir)],
                    "media:complete",
                ))
            }
            ProductionStep::Asr => {
                let segments = commands::asr_run(
                    self.app.clone(),
                    state,
                    context.project_id.clone(),
                    self.legacy_job_id.clone(),
                )
                .await?;
                Ok(completed(
                    vec![format!(
                        "segments:{}:{}",
                        context.project_id,
                        segments.len()
                    )],
                    "asr:complete",
                ))
            }
            ProductionStep::TranscriptReview => self.review_gate(
                context,
                "识别结果已保存，请确认字幕与术语后继续",
                "transcript-review:approved",
            ),
            ProductionStep::Translation => {
                let segments = commands::translation_run(
                    self.app.clone(),
                    state,
                    context.project_id.clone(),
                    self.legacy_job_id.clone(),
                )
                .await?;
                Ok(completed(
                    vec![format!(
                        "translation:{}:{}",
                        context.project_id,
                        segments.len()
                    )],
                    "translation:complete",
                ))
            }
            ProductionStep::ScriptReview => self.review_gate(
                context,
                "中文口播稿已保存，请确认演绎设置后继续",
                "script-review:approved",
            ),
            ProductionStep::TtsSynthesis => self.synthesize(context).await,
            ProductionStep::AlignmentValidation => {
                let readiness =
                    project_commands::project_readiness(state, context.project_id.clone())?;
                if readiness.blocking_count > 0 {
                    return Ok(NodeOutcome::WaitingForInput {
                        reason: readiness.next_action,
                        checkpoint: Some("alignment:needs-review".into()),
                    });
                }
                Ok(completed(Vec::new(), "alignment:validated"))
            }
            ProductionStep::MixPreview => {
                let artifact = state
                    .database
                    .lock()
                    .expect("database mutex poisoned")
                    .get_artifact(&format!("tts-mix-{}", context.project_id))?;
                let path = PathBuf::from(&artifact.path);
                if !path.is_file() || path.metadata().map_or(true, |metadata| metadata.len() == 0) {
                    return Err(AppError::Media("中文混音产物缺失或为空".into()));
                }
                Ok(completed(vec![artifact.id], "mix-preview:ready"))
            }
            ProductionStep::ExportPublish => Ok(NodeOutcome::WaitingForInput {
                reason: "项目已准备好，请选择导出目录和字幕模式".into(),
                checkpoint: Some("export:ready".into()),
            }),
        }
    }

    fn review_gate(
        &self,
        context: &ExecutionContext,
        reason: &str,
        checkpoint: &str,
    ) -> Result<NodeOutcome, AppError> {
        let project = self
            .app
            .state::<AppState>()
            .database
            .lock()
            .expect("database mutex poisoned")
            .get_project(&context.project_id)?;
        if project.workflow_mode == "review" && context.attempt == 1 {
            return Ok(NodeOutcome::WaitingForInput {
                reason: reason.into(),
                checkpoint: Some(format!("{}:waiting", self.spec.id)),
            });
        }
        Ok(completed(Vec::new(), checkpoint))
    }

    async fn synthesize(&self, context: &ExecutionContext) -> Result<NodeOutcome, AppError> {
        let state = self.app.state::<AppState>();
        let project = state
            .database
            .lock()
            .expect("database mutex poisoned")
            .get_project(&context.project_id)?;
        let unfinished_ids = state
            .database
            .lock()
            .expect("database mutex poisoned")
            .list_segments(&context.project_id)?
            .into_iter()
            .filter(|segment| {
                !crate::localization::is_non_speech_text(&segment.source_text)
                    && segment.tts_state != "ready"
            })
            .map(|segment| segment.id)
            .collect::<Vec<_>>();
        // A retry reconciles every unfinished segment against the on-disk cache.
        // This includes stale rows left by an interrupted run, not only rows whose
        // last provider response was explicitly marked failed.
        let requested =
            (context.attempt > 1 && !unfinished_ids.is_empty()).then_some(unfinished_ids);
        let mut result = if project.tts_sync_mode == "semantic" && requested.is_none() {
            commands::semantic_narration_run(
                self.app.clone(),
                state,
                context.project_id.clone(),
                self.legacy_job_id.clone(),
            )
            .await?
        } else {
            commands::tts_run(
                self.app.clone(),
                state,
                context.project_id.clone(),
                self.legacy_job_id.clone(),
                requested,
            )
            .await?
        };
        if project.workflow_mode == "quick" && !result.failed_segments.is_empty() {
            let retry_ids = result
                .failed_segments
                .iter()
                .map(|failure| failure.segment_id.clone())
                .collect();
            result = commands::tts_run(
                self.app.clone(),
                self.app.state::<AppState>(),
                context.project_id.clone(),
                self.legacy_job_id.clone(),
                Some(retry_ids),
            )
            .await?;
        }
        if !result.failed_segments.is_empty() {
            return Ok(NodeOutcome::WaitingForInput {
                reason: format!(
                    "{} 个配音单元生成失败，请检查服务商后继续",
                    result.failed_segments.len()
                ),
                checkpoint: Some("tts:failed-units".into()),
            });
        }
        if project.workflow_mode == "quick" && !result.warning_ids.is_empty() {
            let fit = commands::tts_fit_warnings(
                self.app.clone(),
                self.app.state::<AppState>(),
                context.project_id.clone(),
                self.legacy_job_id.clone(),
                Some(result.warning_ids.clone()),
            )
            .await?;
            if !fit.remaining_ids.is_empty() {
                return Ok(NodeOutcome::WaitingForInput {
                    reason: format!("仍有 {} 个时长问题需要人工确认", fit.remaining_ids.len()),
                    checkpoint: Some("tts:timing-review".into()),
                });
            }
        } else if !result.warning_ids.is_empty() {
            return Ok(NodeOutcome::WaitingForInput {
                reason: format!("{} 个配音单元需要时长确认", result.warning_ids.len()),
                checkpoint: Some("tts:timing-review".into()),
            });
        }
        Ok(completed(
            vec![format!("tts-mix-{}", context.project_id)],
            "tts:complete",
        ))
    }
}

pub(crate) fn definition(
    app: AppHandle,
    legacy_job_id: impl Into<String>,
) -> Result<WorkflowDefinition, AppError> {
    let legacy_job_id = legacy_job_id.into();
    let specs = [
        spec(
            "media_prepare",
            2,
            crate::domain::JobStage::MediaCheck,
            &[],
            ResourceClass::Media,
            2,
        ),
        spec(
            "asr",
            2,
            crate::domain::JobStage::Asr,
            &["media_prepare"],
            ResourceClass::Cpu,
            2,
        ),
        spec(
            "transcript_review",
            1,
            crate::domain::JobStage::Glossary,
            &["asr"],
            ResourceClass::Disk,
            1,
        ),
        spec(
            "translation",
            2,
            crate::domain::JobStage::Translation,
            &["transcript_review"],
            ResourceClass::Network,
            3,
        ),
        spec(
            "script_review",
            1,
            crate::domain::JobStage::ScriptDirector,
            &["translation"],
            ResourceClass::Disk,
            1,
        ),
        spec(
            "tts_synthesis",
            3,
            crate::domain::JobStage::Tts,
            &["script_review"],
            ResourceClass::Network,
            3,
        ),
        spec(
            "alignment_validation",
            1,
            crate::domain::JobStage::Tts,
            &["tts_synthesis"],
            ResourceClass::Cpu,
            2,
        ),
        spec(
            "mix_preview",
            1,
            crate::domain::JobStage::Export,
            &["alignment_validation"],
            ResourceClass::Media,
            2,
        ),
        spec(
            EXPORT_NODE_ID,
            EXPORT_NODE_VERSION,
            crate::domain::JobStage::Export,
            &["mix_preview"],
            ResourceClass::Disk,
            1,
        ),
    ];
    let steps = [
        ProductionStep::MediaPrepare,
        ProductionStep::Asr,
        ProductionStep::TranscriptReview,
        ProductionStep::Translation,
        ProductionStep::ScriptReview,
        ProductionStep::TtsSynthesis,
        ProductionStep::AlignmentValidation,
        ProductionStep::MixPreview,
        ProductionStep::ExportPublish,
    ];
    let nodes = specs
        .into_iter()
        .zip(steps)
        .map(|(spec, step)| {
            Box::new(ProductionNode {
                app: app.clone(),
                legacy_job_id: legacy_job_id.clone(),
                step,
                spec,
            }) as Box<dyn WorkflowNode>
        })
        .collect();
    WorkflowDefinition::new(WORKFLOW_ID, WORKFLOW_VERSION, nodes)
}

fn spec(
    id: &str,
    version: u32,
    stage: crate::domain::JobStage,
    dependencies: &[&str],
    resource_class: ResourceClass,
    max_attempts: u32,
) -> WorkflowNodeSpec {
    WorkflowNodeSpec {
        id: id.into(),
        version,
        stage,
        dependencies: dependencies.iter().map(|value| (*value).into()).collect(),
        resource_class,
        retry_policy: if max_attempts == 1 {
            RetryPolicy::never()
        } else {
            RetryPolicy::up_to(max_attempts)
        },
    }
}

fn completed(output_artifact_ids: Vec<String>, checkpoint: &str) -> NodeOutcome {
    NodeOutcome::Completed {
        output_artifact_ids,
        checkpoint: Some(checkpoint.into()),
    }
}

fn classify_error(error: AppError, node_id: &str) -> NodeOutcome {
    let checkpoint = Some(format!("{node_id}:error"));
    match error {
        AppError::Provider(message) | AppError::Credential(message) => NodeOutcome::Retryable {
            classification: RetryClassification::RateLimited,
            message,
            retry_after_ms: Some(1_000),
            checkpoint,
        },
        AppError::Database(error) => NodeOutcome::Retryable {
            classification: RetryClassification::Dependency,
            message: error.to_string(),
            retry_after_ms: Some(500),
            checkpoint,
        },
        AppError::Media(message) => NodeOutcome::Failed {
            classification: FailureClassification::Media,
            message,
            checkpoint,
        },
        AppError::NotFound(message) => NodeOutcome::Failed {
            classification: FailureClassification::Dependency,
            message,
            checkpoint,
        },
        AppError::Validation(message) => NodeOutcome::Failed {
            classification: FailureClassification::Validation,
            message,
            checkpoint,
        },
    }
}
