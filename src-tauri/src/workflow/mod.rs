use std::{
    collections::HashSet,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::{
    domain::{JobStage, JobSummary},
    error::AppError,
};
use serde::{Deserialize, Serialize};

pub type NodeFuture<'a> = Pin<Box<dyn Future<Output = NodeOutcome> + Send + 'a>>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceClass {
    Cpu,
    Media,
    Network,
    Disk,
}

impl ResourceClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Media => "media",
            Self::Network => "network",
            Self::Disk => "disk",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetryClassification {
    Transient,
    RateLimited,
    Dependency,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureClassification {
    Validation,
    Dependency,
    ProviderTerminal,
    Media,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
}

impl RetryPolicy {
    pub const fn never() -> Self {
        Self { max_attempts: 1 }
    }

    pub const fn up_to(max_attempts: u32) -> Self {
        Self { max_attempts }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowNodeSpec {
    pub id: String,
    pub version: u32,
    pub stage: JobStage,
    pub dependencies: Vec<String>,
    pub resource_class: ResourceClass,
    pub retry_policy: RetryPolicy,
}

pub trait WorkflowNode: Send + Sync {
    fn spec(&self) -> WorkflowNodeSpec;
    fn execute<'a>(&'a self, context: ExecutionContext) -> NodeFuture<'a>;
}

pub struct WorkflowDefinition {
    pub id: String,
    pub version: u32,
    nodes: Vec<Box<dyn WorkflowNode>>,
}

impl WorkflowDefinition {
    pub fn new(
        id: impl Into<String>,
        version: u32,
        nodes: Vec<Box<dyn WorkflowNode>>,
    ) -> Result<Self, AppError> {
        let id = id.into();
        if id.trim().is_empty() || version == 0 || nodes.is_empty() {
            return Err(AppError::Validation(
                "工作流 ID、版本和节点列表不能为空".into(),
            ));
        }
        let mut seen = HashSet::new();
        for node in &nodes {
            let spec = node.spec();
            if spec.id.trim().is_empty() || spec.version == 0 || spec.retry_policy.max_attempts == 0
            {
                return Err(AppError::Validation("工作流节点定义无效".into()));
            }
            if !seen.insert(spec.id.clone()) {
                return Err(AppError::Validation(format!(
                    "工作流节点 ID 重复：{}",
                    spec.id
                )));
            }
            let unique_dependencies = spec.dependencies.iter().collect::<HashSet<_>>();
            if unique_dependencies.len() != spec.dependencies.len() {
                return Err(AppError::Validation(format!(
                    "节点 {} 包含重复依赖",
                    spec.id
                )));
            }
            if let Some(dependency) = spec
                .dependencies
                .iter()
                .find(|dependency| !seen.contains(dependency.as_str()))
            {
                return Err(AppError::Validation(format!(
                    "节点 {} 的依赖 {} 不存在、位于其后或形成环",
                    spec.id, dependency
                )));
            }
        }
        Ok(Self { id, version, nodes })
    }

    pub fn nodes(&self) -> &[Box<dyn WorkflowNode>] {
        &self.nodes
    }
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub run_id: String,
    pub project_id: String,
    pub node_id: String,
    pub attempt: u32,
    pub input_artifact_ids: Vec<String>,
    pub cancellation: CancellationToken,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeOutcome {
    Completed {
        output_artifact_ids: Vec<String>,
        checkpoint: Option<String>,
    },
    WaitingForInput {
        reason: String,
        checkpoint: Option<String>,
    },
    Retryable {
        classification: RetryClassification,
        message: String,
        retry_after_ms: Option<u64>,
        checkpoint: Option<String>,
    },
    Failed {
        classification: FailureClassification,
        message: String,
        checkpoint: Option<String>,
    },
    Cancelled {
        checkpoint: Option<String>,
    },
}

impl NodeOutcome {
    fn checkpoint(&self) -> Option<String> {
        match self {
            Self::Completed { checkpoint, .. }
            | Self::WaitingForInput { checkpoint, .. }
            | Self::Retryable { checkpoint, .. }
            | Self::Failed { checkpoint, .. }
            | Self::Cancelled { checkpoint } => checkpoint.clone(),
        }
    }

    fn message(&self) -> Option<String> {
        match self {
            Self::WaitingForInput { reason, .. } => Some(reason.clone()),
            Self::Retryable { message, .. } | Self::Failed { message, .. } => Some(message.clone()),
            Self::Completed { .. } | Self::Cancelled { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunState {
    Queued,
    Running,
    WaitingForInput,
    Retryable,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeRunState {
    Running,
    WaitingForInput,
    Retryable,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct WorkflowRunRecord {
    pub id: String,
    pub workflow_id: String,
    pub workflow_version: u32,
    pub project_id: String,
    pub legacy_job_id: Option<String>,
    pub state: WorkflowRunState,
    pub current_node_id: Option<String>,
    pub stage: JobStage,
    pub progress: u8,
    pub checkpoint: Option<String>,
    pub error_message: Option<String>,
    pub cancel_requested: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NodeRunRecord {
    pub id: String,
    pub run_id: String,
    pub node_id: String,
    pub node_version: u32,
    pub attempt: u32,
    pub state: NodeRunState,
    pub input_artifact_ids: Vec<String>,
    pub output_artifact_ids: Vec<String>,
    pub checkpoint: Option<String>,
    pub error_class: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunEventRecord {
    pub id: i64,
    pub run_id: String,
    pub node_run_id: Option<String>,
    pub kind: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct CreateWorkflowRun {
    pub id: String,
    pub workflow_id: String,
    pub workflow_version: u32,
    pub project_id: String,
    pub legacy_job_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunUpdate {
    pub state: WorkflowRunState,
    pub current_node_id: Option<String>,
    pub stage: JobStage,
    pub progress: u8,
    pub checkpoint: Option<String>,
    pub error_message: Option<String>,
}

pub trait WorkflowStore {
    fn create_run(
        &self,
        request: &CreateWorkflowRun,
        initial_stage: JobStage,
    ) -> Result<WorkflowRunRecord, AppError>;
    fn get_run(&self, run_id: &str) -> Result<WorkflowRunRecord, AppError>;
    fn find_run_by_legacy_job_id(
        &self,
        legacy_job_id: &str,
    ) -> Result<Option<WorkflowRunRecord>, AppError>;
    fn latest_node_run(
        &self,
        run_id: &str,
        node_id: &str,
    ) -> Result<Option<NodeRunRecord>, AppError>;
    fn begin_node(
        &self,
        run_id: &str,
        spec: &WorkflowNodeSpec,
        attempt: u32,
        input_artifact_ids: &[String],
    ) -> Result<NodeRunRecord, AppError>;
    fn finish_node(&self, node_run_id: &str, outcome: &NodeOutcome) -> Result<(), AppError>;
    fn update_run(&self, run_id: &str, update: &RunUpdate) -> Result<(), AppError>;
    fn resume_run(&self, run_id: &str) -> Result<(), AppError>;
    fn request_cancel(&self, run_id: &str) -> Result<(), AppError>;
    fn record_event(
        &self,
        run_id: &str,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<(), AppError>;
    fn complete_external_node(
        &self,
        legacy_job_id: &str,
        node_id: &str,
        node_version: u32,
        output_artifact_ids: &[String],
        checkpoint: &str,
    ) -> Result<(), AppError>;
    fn project_job_summary(&self, run_id: &str) -> Result<JobSummary, AppError>;
    fn list_node_runs(&self, run_id: &str) -> Result<Vec<NodeRunRecord>, AppError>;
    fn list_events(&self, run_id: &str) -> Result<Vec<RunEventRecord>, AppError>;
}

mod runner;
pub use runner::{RunnerResult, WorkflowRunner};
mod scheduler;
pub use scheduler::ResourceScheduler;

#[cfg(test)]
mod tests;
