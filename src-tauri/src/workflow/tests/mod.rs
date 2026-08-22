use std::{
    collections::VecDeque,
    future::Future,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use crate::{
    db::Database,
    domain::{JobStage, JobStatus},
    infrastructure::workflow_store::WorkflowSqliteStore,
};

use super::*;

struct FakeNode {
    spec: WorkflowNodeSpec,
    outcomes: Mutex<VecDeque<NodeOutcome>>,
    calls: Arc<AtomicUsize>,
    cancel_during_execution: bool,
}

impl WorkflowNode for FakeNode {
    fn spec(&self) -> WorkflowNodeSpec {
        self.spec.clone()
    }

    fn execute<'a>(&'a self, context: ExecutionContext) -> NodeFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.cancel_during_execution {
            context.cancellation.cancel();
        }
        let outcome = self
            .outcomes
            .lock()
            .expect("fake outcome mutex poisoned")
            .pop_front()
            .unwrap_or_else(|| completed(&format!("artifact:{}", self.spec.id)));
        Box::pin(async move { outcome })
    }
}

fn fake_node(
    id: &str,
    stage: JobStage,
    dependencies: &[&str],
    max_attempts: u32,
    outcomes: Vec<NodeOutcome>,
    calls: Arc<AtomicUsize>,
) -> Box<dyn WorkflowNode> {
    Box::new(FakeNode {
        spec: WorkflowNodeSpec {
            id: id.into(),
            version: 1,
            stage,
            dependencies: dependencies.iter().map(|value| (*value).into()).collect(),
            resource_class: ResourceClass::Cpu,
            retry_policy: RetryPolicy::up_to(max_attempts),
        },
        outcomes: Mutex::new(outcomes.into()),
        calls,
        cancel_during_execution: false,
    })
}

fn cancelling_node(id: &str, calls: Arc<AtomicUsize>) -> Box<dyn WorkflowNode> {
    Box::new(FakeNode {
        spec: WorkflowNodeSpec {
            id: id.into(),
            version: 1,
            stage: JobStage::Tts,
            dependencies: Vec::new(),
            resource_class: ResourceClass::Network,
            retry_policy: RetryPolicy::never(),
        },
        outcomes: Mutex::new(vec![completed("should-not-publish")].into()),
        calls,
        cancel_during_execution: true,
    })
}

fn completed(artifact: &str) -> NodeOutcome {
    NodeOutcome::Completed {
        output_artifact_ids: vec![artifact.into()],
        checkpoint: Some(format!("published:{artifact}")),
    }
}

fn temporary_database(project_id: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "yisheng-workflow-{}-{}.db",
        project_id,
        uuid::Uuid::new_v4()
    ));
    let database = Database::open(&path).unwrap();
    database
        .create_project(project_id, "Workflow test")
        .unwrap();
    drop(database);
    path
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn definition_rejects_forward_dependencies_and_zero_retry_budget() {
    let calls = Arc::new(AtomicUsize::new(0));
    let forward = WorkflowDefinition::new(
        "invalid",
        1,
        vec![fake_node(
            "translate",
            JobStage::Translation,
            &["asr"],
            1,
            vec![completed("translation")],
            calls.clone(),
        )],
    );
    assert!(forward.is_err());

    let zero_retry = WorkflowDefinition::new(
        "invalid-retry",
        1,
        vec![fake_node(
            "asr",
            JobStage::Asr,
            &[],
            0,
            vec![completed("asr")],
            calls,
        )],
    );
    assert!(zero_retry.is_err());
}

#[test]
fn completed_run_projects_a_legacy_job_and_is_idempotent() {
    let path = temporary_database("p-complete");
    let store = WorkflowSqliteStore::open(&path).unwrap();
    let first_calls = Arc::new(AtomicUsize::new(0));
    let second_calls = Arc::new(AtomicUsize::new(0));
    let definition = WorkflowDefinition::new(
        "localize",
        1,
        vec![
            fake_node(
                "media",
                JobStage::Proxy,
                &[],
                1,
                vec![completed("artifact:proxy")],
                first_calls.clone(),
            ),
            fake_node(
                "asr",
                JobStage::Asr,
                &["media"],
                1,
                vec![completed("artifact:segments")],
                second_calls.clone(),
            ),
        ],
    )
    .unwrap();
    let runner = WorkflowRunner::new(&store);
    let run = runner
        .create_run(&definition, "p-complete", Some("legacy-job".into()))
        .unwrap();

    let result =
        block_on(runner.run_until_blocked(&definition, &run.id, CancellationToken::default()))
            .unwrap();
    assert_eq!(result.state, WorkflowRunState::Succeeded);
    let projection = store.project_job_summary(&run.id).unwrap();
    assert_eq!(projection.id, "legacy-job");
    assert_eq!(projection.status, JobStatus::Succeeded);
    assert_eq!(projection.stage, JobStage::Asr);
    assert_eq!(projection.progress, 100);
    let node_runs = store.list_node_runs(&run.id).unwrap();
    assert_eq!(node_runs.len(), 2);
    assert_eq!(node_runs[1].input_artifact_ids, ["artifact:proxy"]);

    let repeated =
        block_on(runner.run_until_blocked(&definition, &run.id, CancellationToken::default()))
            .unwrap();
    assert_eq!(repeated.state, WorkflowRunState::Succeeded);
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    assert_eq!(second_calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.list_node_runs(&run.id).unwrap().len(), 2);
    assert!(store
        .list_events(&run.id)
        .unwrap()
        .iter()
        .any(|event| event.kind == "job_projection"));
}

#[test]
fn waiting_node_requires_an_explicit_resume() {
    let path = temporary_database("p-wait");
    let store = WorkflowSqliteStore::open(&path).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let definition = WorkflowDefinition::new(
        "review",
        1,
        vec![fake_node(
            "review",
            JobStage::Glossary,
            &[],
            1,
            vec![
                NodeOutcome::WaitingForInput {
                    reason: "等待术语审核".into(),
                    checkpoint: Some("review:pending".into()),
                },
                completed("artifact:review"),
            ],
            calls.clone(),
        )],
    )
    .unwrap();
    let runner = WorkflowRunner::new(&store);
    let run = runner.create_run(&definition, "p-wait", None).unwrap();

    let waiting =
        block_on(runner.run_until_blocked(&definition, &run.id, CancellationToken::default()))
            .unwrap();
    assert_eq!(waiting.state, WorkflowRunState::WaitingForInput);
    assert_eq!(
        store.project_job_summary(&run.id).unwrap().status,
        JobStatus::WaitingUser
    );
    let still_waiting =
        block_on(runner.run_until_blocked(&definition, &run.id, CancellationToken::default()))
            .unwrap();
    assert_eq!(still_waiting.state, WorkflowRunState::WaitingForInput);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    runner.resume(&run.id).unwrap();
    let completed =
        block_on(runner.run_until_blocked(&definition, &run.id, CancellationToken::default()))
            .unwrap();
    assert_eq!(completed.state, WorkflowRunState::Succeeded);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn terminal_failure_stops_the_run() {
    let path = temporary_database("p-failed");
    let store = WorkflowSqliteStore::open(&path).unwrap();
    let definition = WorkflowDefinition::new(
        "failure",
        1,
        vec![fake_node(
            "translate",
            JobStage::Translation,
            &[],
            1,
            vec![NodeOutcome::Failed {
                classification: FailureClassification::Validation,
                message: "模型返回缺少片段".into(),
                checkpoint: Some("translation:invalid".into()),
            }],
            Arc::new(AtomicUsize::new(0)),
        )],
    )
    .unwrap();
    let runner = WorkflowRunner::new(&store);
    let run = runner.create_run(&definition, "p-failed", None).unwrap();
    let result =
        block_on(runner.run_until_blocked(&definition, &run.id, CancellationToken::default()))
            .unwrap();
    assert_eq!(result.state, WorkflowRunState::Failed);
    let projection = store.project_job_summary(&run.id).unwrap();
    assert_eq!(projection.status, JobStatus::Failed);
    assert!(projection.error_message.unwrap().contains("缺少片段"));
}

#[test]
fn cancellation_token_discards_a_completed_node_outcome() {
    let path = temporary_database("p-cancel");
    let store = WorkflowSqliteStore::open(&path).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let definition =
        WorkflowDefinition::new("cancel", 1, vec![cancelling_node("tts", calls.clone())]).unwrap();
    let runner = WorkflowRunner::new(&store);
    let run = runner.create_run(&definition, "p-cancel", None).unwrap();
    let result =
        block_on(runner.run_until_blocked(&definition, &run.id, CancellationToken::default()))
            .unwrap();
    assert_eq!(result.state, WorkflowRunState::Cancelled);
    let node = store.list_node_runs(&run.id).unwrap().remove(0);
    assert_eq!(node.state, NodeRunState::Cancelled);
    assert!(node.output_artifact_ids.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn persisted_cancel_request_stops_a_queued_run_before_execution() {
    let path = temporary_database("p-cancel-request");
    let store = WorkflowSqliteStore::open(&path).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let definition = WorkflowDefinition::new(
        "cancel-request",
        1,
        vec![fake_node(
            "media",
            JobStage::Proxy,
            &[],
            1,
            vec![completed("artifact:proxy")],
            calls.clone(),
        )],
    )
    .unwrap();
    let runner = WorkflowRunner::new(&store);
    let run = runner
        .create_run(&definition, "p-cancel-request", None)
        .unwrap();
    runner.request_cancel(&run.id).unwrap();
    let result =
        block_on(runner.run_until_blocked(&definition, &run.id, CancellationToken::default()))
            .unwrap();
    assert_eq!(result.state, WorkflowRunState::Cancelled);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        store.project_job_summary(&run.id).unwrap().status,
        JobStatus::Cancelled
    );
}

#[test]
fn retry_resumes_after_reopen_without_republishing_completed_nodes() {
    let path = temporary_database("p-retry");
    let first_store = WorkflowSqliteStore::open(&path).unwrap();
    let first_a_calls = Arc::new(AtomicUsize::new(0));
    let first_b_calls = Arc::new(AtomicUsize::new(0));
    let first_definition = WorkflowDefinition::new(
        "resume",
        1,
        vec![
            fake_node(
                "media",
                JobStage::Proxy,
                &[],
                1,
                vec![completed("artifact:proxy")],
                first_a_calls.clone(),
            ),
            fake_node(
                "asr",
                JobStage::Asr,
                &["media"],
                2,
                vec![NodeOutcome::Retryable {
                    classification: RetryClassification::Transient,
                    message: "临时失败".into(),
                    retry_after_ms: Some(50),
                    checkpoint: Some("asr:retry".into()),
                }],
                first_b_calls.clone(),
            ),
        ],
    )
    .unwrap();
    let run_id = {
        let runner = WorkflowRunner::new(&first_store);
        let run = runner
            .create_run(&first_definition, "p-retry", None)
            .unwrap();
        let result = block_on(runner.run_until_blocked(
            &first_definition,
            &run.id,
            CancellationToken::default(),
        ))
        .unwrap();
        assert_eq!(result.state, WorkflowRunState::Retryable);
        run.id
    };
    assert_eq!(first_a_calls.load(Ordering::SeqCst), 1);
    assert_eq!(first_b_calls.load(Ordering::SeqCst), 1);
    drop(first_store);

    let reopened_store = WorkflowSqliteStore::open(&path).unwrap();
    let resumed_a_calls = Arc::new(AtomicUsize::new(0));
    let resumed_b_calls = Arc::new(AtomicUsize::new(0));
    let resumed_definition = WorkflowDefinition::new(
        "resume",
        1,
        vec![
            fake_node(
                "media",
                JobStage::Proxy,
                &[],
                1,
                vec![completed("duplicate")],
                resumed_a_calls.clone(),
            ),
            fake_node(
                "asr",
                JobStage::Asr,
                &["media"],
                2,
                vec![completed("artifact:segments")],
                resumed_b_calls.clone(),
            ),
        ],
    )
    .unwrap();
    let runner = WorkflowRunner::new(&reopened_store);
    runner.resume(&run_id).unwrap();
    let result = block_on(runner.run_until_blocked(
        &resumed_definition,
        &run_id,
        CancellationToken::default(),
    ))
    .unwrap();
    assert_eq!(result.state, WorkflowRunState::Succeeded);
    assert_eq!(resumed_a_calls.load(Ordering::SeqCst), 0);
    assert_eq!(resumed_b_calls.load(Ordering::SeqCst), 1);
    let nodes = reopened_store.list_node_runs(&run_id).unwrap();
    assert_eq!(
        nodes.iter().filter(|node| node.node_id == "media").count(),
        1
    );
    assert_eq!(nodes.iter().filter(|node| node.node_id == "asr").count(), 2);
    assert_eq!(nodes.last().unwrap().input_artifact_ids, ["artifact:proxy"]);
}

#[test]
fn reopening_marks_an_interrupted_node_retryable() {
    let path = temporary_database("p-interrupted");
    let store = WorkflowSqliteStore::open(&path).unwrap();
    let definition = WorkflowDefinition::new(
        "interrupted",
        1,
        vec![fake_node(
            "media",
            JobStage::Proxy,
            &[],
            2,
            vec![completed("artifact:proxy")],
            Arc::new(AtomicUsize::new(0)),
        )],
    )
    .unwrap();
    let runner = WorkflowRunner::new(&store);
    let run = runner
        .create_run(&definition, "p-interrupted", None)
        .unwrap();
    let spec = definition.nodes()[0].spec();
    store
        .update_run(
            &run.id,
            &RunUpdate {
                state: WorkflowRunState::Running,
                current_node_id: Some(spec.id.clone()),
                stage: spec.stage,
                progress: 0,
                checkpoint: Some("media:started".into()),
                error_message: None,
            },
        )
        .unwrap();
    store.begin_node(&run.id, &spec, 1, &[]).unwrap();
    drop(store);

    let reopened = WorkflowSqliteStore::open(&path).unwrap();
    assert_eq!(
        reopened.get_run(&run.id).unwrap().state,
        WorkflowRunState::Retryable
    );
    assert_eq!(
        reopened
            .latest_node_run(&run.id, "media")
            .unwrap()
            .unwrap()
            .state,
        NodeRunState::Retryable
    );
    assert_eq!(
        reopened.project_job_summary(&run.id).unwrap().status,
        JobStatus::Paused
    );
    assert!(reopened
        .list_events(&run.id)
        .unwrap()
        .iter()
        .any(|event| event.kind == "run_recovered"));
}

#[test]
fn external_export_completion_atomically_finishes_the_waiting_run() {
    let path = temporary_database("p-external-export");
    let store = WorkflowSqliteStore::open(&path).unwrap();
    let definition = WorkflowDefinition::new(
        "external-export",
        1,
        vec![fake_node(
            "export_publish",
            JobStage::Export,
            &[],
            1,
            vec![NodeOutcome::WaitingForInput {
                reason: "选择导出目录".into(),
                checkpoint: Some("export:ready".into()),
            }],
            Arc::new(AtomicUsize::new(0)),
        )],
    )
    .unwrap();
    let runner = WorkflowRunner::new(&store);
    let run = runner
        .create_run(
            &definition,
            "p-external-export",
            Some("job-external-export".into()),
        )
        .unwrap();
    let result =
        block_on(runner.run_until_blocked(&definition, &run.id, CancellationToken::default()))
            .unwrap();
    assert_eq!(result.state, WorkflowRunState::WaitingForInput);

    store
        .complete_external_node(
            "job-external-export",
            "export_publish",
            1,
            &["video.mp4".into(), "voice.wav".into()],
            "export:published",
        )
        .unwrap();
    store
        .complete_external_node(
            "job-external-export",
            "export_publish",
            1,
            &["video.mp4".into(), "voice.wav".into()],
            "export:published",
        )
        .unwrap();

    assert_eq!(
        store.get_run(&run.id).unwrap().state,
        WorkflowRunState::Succeeded
    );
    let node = store
        .latest_node_run(&run.id, "export_publish")
        .unwrap()
        .unwrap();
    assert_eq!(node.state, NodeRunState::Succeeded);
    assert_eq!(node.output_artifact_ids, ["video.mp4", "voice.wav"]);
    assert!(store
        .list_events(&run.id)
        .unwrap()
        .iter()
        .any(|event| event.kind == "external_node_completed"));
}
