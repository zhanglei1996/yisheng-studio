use std::{collections::HashMap, time::Instant};

use serde_json::json;

use uuid::Uuid;

use super::{
    AppError, CancellationToken, CreateWorkflowRun, ExecutionContext, NodeOutcome, NodeRunState,
    ResourceScheduler, RunUpdate, WorkflowDefinition, WorkflowNodeSpec, WorkflowRunRecord,
    WorkflowRunState, WorkflowStore,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerResult {
    pub state: WorkflowRunState,
    pub node_id: Option<String>,
}

pub struct WorkflowRunner<'a, S: WorkflowStore + ?Sized> {
    store: &'a S,
    scheduler: Option<&'a ResourceScheduler>,
}

impl<'a, S: WorkflowStore + ?Sized> WorkflowRunner<'a, S> {
    pub fn new(store: &'a S) -> Self {
        Self {
            store,
            scheduler: None,
        }
    }

    pub fn with_scheduler(store: &'a S, scheduler: &'a ResourceScheduler) -> Self {
        Self {
            store,
            scheduler: Some(scheduler),
        }
    }

    pub fn create_run(
        &self,
        definition: &WorkflowDefinition,
        project_id: impl Into<String>,
        legacy_job_id: Option<String>,
    ) -> Result<WorkflowRunRecord, AppError> {
        let first = definition.nodes()[0].spec();
        self.store.create_run(
            &CreateWorkflowRun {
                id: Uuid::new_v4().to_string(),
                workflow_id: definition.id.clone(),
                workflow_version: definition.version,
                project_id: project_id.into(),
                legacy_job_id,
            },
            first.stage,
        )
    }

    pub fn resume(&self, run_id: &str) -> Result<(), AppError> {
        self.store.resume_run(run_id)
    }

    pub fn request_cancel(&self, run_id: &str) -> Result<(), AppError> {
        self.store.request_cancel(run_id)
    }

    pub async fn run_until_blocked(
        &self,
        definition: &WorkflowDefinition,
        run_id: &str,
        cancellation: CancellationToken,
    ) -> Result<RunnerResult, AppError> {
        let run = self.store.get_run(run_id)?;
        self.validate_definition(definition, &run)?;
        if run.cancel_requested || cancellation.is_cancelled() {
            self.store.update_run(
                run_id,
                &RunUpdate {
                    state: WorkflowRunState::Cancelled,
                    current_node_id: run.current_node_id.clone(),
                    stage: run.stage,
                    progress: run.progress,
                    checkpoint: run.checkpoint,
                    error_message: None,
                },
            )?;
            return Ok(RunnerResult {
                state: WorkflowRunState::Cancelled,
                node_id: run.current_node_id,
            });
        }
        if is_terminal(run.state) || is_blocked(run.state) {
            return Ok(RunnerResult {
                state: run.state,
                node_id: run.current_node_id,
            });
        }

        let mut outputs = HashMap::<String, Vec<String>>::new();
        let total = definition.nodes().len();
        for (index, node) in definition.nodes().iter().enumerate() {
            let spec = node.spec();
            let latest = self.store.latest_node_run(run_id, &spec.id)?;
            if let Some(completed) = latest
                .as_ref()
                .filter(|record| record.state == NodeRunState::Succeeded)
            {
                if completed.node_version != spec.version {
                    return Err(AppError::Validation(format!(
                        "节点 {} 的已完成版本与定义不匹配",
                        spec.id
                    )));
                }
                outputs.insert(spec.id.clone(), completed.output_artifact_ids.clone());
                continue;
            }

            let current = self.store.get_run(run_id)?;
            if cancellation.is_cancelled() || current.cancel_requested {
                self.finish_run(
                    run_id,
                    &spec,
                    progress(index, total),
                    WorkflowRunState::Cancelled,
                    current.checkpoint,
                    None,
                )?;
                return Ok(result(WorkflowRunState::Cancelled, spec.id));
            }
            let attempt = latest.as_ref().map_or(1, |record| record.attempt + 1);
            if latest
                .as_ref()
                .is_some_and(|record| record.state == NodeRunState::Retryable)
                && attempt > spec.retry_policy.max_attempts
            {
                self.finish_run(
                    run_id,
                    &spec,
                    progress(index, total),
                    WorkflowRunState::Failed,
                    latest.and_then(|record| record.checkpoint),
                    Some("节点已耗尽重试预算".into()),
                )?;
                return Ok(result(WorkflowRunState::Failed, spec.id));
            }
            let input_artifact_ids = spec
                .dependencies
                .iter()
                .flat_map(|dependency| outputs.get(dependency).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            self.store.update_run(
                run_id,
                &RunUpdate {
                    state: WorkflowRunState::Running,
                    current_node_id: Some(spec.id.clone()),
                    stage: spec.stage,
                    progress: progress(index, total),
                    checkpoint: latest.as_ref().and_then(|record| record.checkpoint.clone()),
                    error_message: None,
                },
            )?;
            let resource_wait_started = Instant::now();
            let _resource_permit = match self.scheduler {
                Some(scheduler) => Some(scheduler.acquire(spec.resource_class).await?),
                None => None,
            };
            self.store.record_event(
                run_id,
                "resource_acquired",
                &json!({
                    "nodeId": spec.id,
                    "resourceClass": spec.resource_class.as_str(),
                    "waitMs": resource_wait_started.elapsed().as_millis(),
                }),
            )?;
            let node_run = self
                .store
                .begin_node(run_id, &spec, attempt, &input_artifact_ids)?;
            let context = ExecutionContext {
                run_id: run_id.to_string(),
                project_id: current.project_id,
                node_id: spec.id.clone(),
                attempt,
                input_artifact_ids,
                cancellation: cancellation.clone(),
            };
            let execution_started = Instant::now();
            let mut outcome = node.execute(context).await;
            if cancellation.is_cancelled() && matches!(outcome, NodeOutcome::Completed { .. }) {
                outcome = NodeOutcome::Cancelled {
                    checkpoint: outcome.checkpoint(),
                };
            }
            self.store.finish_node(&node_run.id, &outcome)?;
            self.store.record_event(
                run_id,
                "node_observed",
                &json!({
                    "nodeId": spec.id,
                    "attempt": attempt,
                    "durationMs": execution_started.elapsed().as_millis(),
                    "outcome": outcome_kind(&outcome),
                }),
            )?;
            match outcome {
                NodeOutcome::Completed {
                    output_artifact_ids,
                    checkpoint,
                } => {
                    outputs.insert(spec.id.clone(), output_artifact_ids);
                    if index + 1 == total {
                        self.finish_run(
                            run_id,
                            &spec,
                            100,
                            WorkflowRunState::Succeeded,
                            checkpoint,
                            None,
                        )?;
                    } else {
                        let next = definition.nodes()[index + 1].spec();
                        self.store.update_run(
                            run_id,
                            &RunUpdate {
                                state: WorkflowRunState::Running,
                                current_node_id: Some(next.id),
                                stage: next.stage,
                                progress: progress(index + 1, total),
                                checkpoint,
                                error_message: None,
                            },
                        )?;
                    }
                }
                NodeOutcome::WaitingForInput { .. } => {
                    return self.blocked_result(
                        run_id,
                        &spec,
                        progress(index, total),
                        WorkflowRunState::WaitingForInput,
                        &outcome,
                    );
                }
                NodeOutcome::Retryable { .. } => {
                    return self.blocked_result(
                        run_id,
                        &spec,
                        progress(index, total),
                        WorkflowRunState::Retryable,
                        &outcome,
                    );
                }
                NodeOutcome::Failed { .. } => {
                    return self.blocked_result(
                        run_id,
                        &spec,
                        progress(index, total),
                        WorkflowRunState::Failed,
                        &outcome,
                    );
                }
                NodeOutcome::Cancelled { .. } => {
                    self.finish_run(
                        run_id,
                        &spec,
                        progress(index, total),
                        WorkflowRunState::Cancelled,
                        outcome.checkpoint(),
                        None,
                    )?;
                    return Ok(result(WorkflowRunState::Cancelled, spec.id));
                }
            }
        }
        Ok(result(
            WorkflowRunState::Succeeded,
            definition
                .nodes()
                .last()
                .expect("non-empty definition")
                .spec()
                .id,
        ))
    }

    fn validate_definition(
        &self,
        definition: &WorkflowDefinition,
        run: &WorkflowRunRecord,
    ) -> Result<(), AppError> {
        if run.workflow_id != definition.id || run.workflow_version != definition.version {
            return Err(AppError::Validation(
                "工作流运行记录与当前定义版本不匹配".into(),
            ));
        }
        Ok(())
    }

    fn blocked_result(
        &self,
        run_id: &str,
        spec: &WorkflowNodeSpec,
        progress: u8,
        state: WorkflowRunState,
        outcome: &NodeOutcome,
    ) -> Result<RunnerResult, AppError> {
        self.finish_run(
            run_id,
            spec,
            progress,
            state,
            outcome.checkpoint(),
            outcome.message(),
        )?;
        Ok(result(state, spec.id.clone()))
    }

    fn finish_run(
        &self,
        run_id: &str,
        spec: &WorkflowNodeSpec,
        progress: u8,
        state: WorkflowRunState,
        checkpoint: Option<String>,
        error_message: Option<String>,
    ) -> Result<(), AppError> {
        self.store.update_run(
            run_id,
            &RunUpdate {
                state,
                current_node_id: Some(spec.id.clone()),
                stage: spec.stage,
                progress,
                checkpoint,
                error_message,
            },
        )
    }
}

fn is_terminal(state: WorkflowRunState) -> bool {
    matches!(
        state,
        WorkflowRunState::Succeeded | WorkflowRunState::Failed | WorkflowRunState::Cancelled
    )
}

fn is_blocked(state: WorkflowRunState) -> bool {
    matches!(
        state,
        WorkflowRunState::WaitingForInput | WorkflowRunState::Retryable
    )
}

fn result(state: WorkflowRunState, node_id: String) -> RunnerResult {
    RunnerResult {
        state,
        node_id: Some(node_id),
    }
}

fn progress(completed: usize, total: usize) -> u8 {
    ((completed.saturating_mul(100) / total.max(1)).min(100)) as u8
}

fn outcome_kind(outcome: &NodeOutcome) -> &'static str {
    match outcome {
        NodeOutcome::Completed { .. } => "completed",
        NodeOutcome::WaitingForInput { .. } => "waiting_for_input",
        NodeOutcome::Retryable { .. } => "retryable",
        NodeOutcome::Failed { .. } => "failed",
        NodeOutcome::Cancelled { .. } => "cancelled",
    }
}
