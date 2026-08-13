//! Stub workflow engine when feature `engine` is off (no rhai).
//!
//! Keeps [`WorkflowRunParams`] and [`run_workflow`] available so shell session
//! wiring typechecks. Launch/validate paths get a clear compile-out error.

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::host::WorkflowHostRequest;
use crate::journal::Journal;
use crate::run::WorkflowOutcome;

pub struct WorkflowRunParams {
    pub script: String,
    pub args: serde_json::Value,
    pub journal: Journal,
    pub host_tx: mpsc::UnboundedSender<WorkflowHostRequest>,
    pub cancel: CancellationToken,
    pub max_ops: u64,
}

impl WorkflowRunParams {
    pub const DEFAULT_MAX_OPS: u64 = 100_000_000;
}

pub fn run_workflow(_params: WorkflowRunParams) -> WorkflowOutcome {
    WorkflowOutcome::Failed {
        error: "workflow engine is not compiled into this build (missing feature `engine`)"
            .into(),
    }
}
