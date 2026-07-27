//! `Effect::Handoff` async spawn (fork `/handoff`).
//!
//! Kept out of the main effects match so upstream effect arms rebase cleanly.

use super::helpers::sanitize_user_error;
use crate::app::actions::TaskResult;
use crate::app::agent::AgentId;
use agent_client_protocol as acp;
use tokio::task::JoinSet;
use xai_acp_lib::{AcpAgentTx, acp_send};

/// Spawn the `x.ai/handoff` call; results land as
/// [`TaskResult::HandoffReady`] / [`TaskResult::HandoffFailed`].
pub(super) fn spawn_handoff(
    tasks: &mut JoinSet<TaskResult>,
    acp_tx: &AcpAgentTx,
    agent_id: AgentId,
    session_id: acp::SessionId,
    task: String,
) {
    let tx = acp_tx.clone();
    tasks.spawn(async move {
        let request = acp::ExtRequest::new(
            "x.ai/handoff",
            serde_json::value::to_raw_value(&serde_json::json!({
                "sessionId": session_id.0.to_string(),
                "task": task,
            }))
            .expect("serialize handoff params")
            .into(),
        );
        match acp_send(request, &tx).await {
            Ok(resp) => {
                let parsed: serde_json::Value =
                    serde_json::from_str(resp.0.get()).unwrap_or_default();
                // ExtMethodResult wraps payload under "result".
                let note = parsed
                    .get("result")
                    .and_then(|r| r.get("note"))
                    .and_then(|n| n.as_str())
                    .or_else(|| parsed.get("note").and_then(|n| n.as_str()))
                    .unwrap_or("")
                    .to_string();
                if note.trim().is_empty() {
                    TaskResult::HandoffFailed {
                        agent_id,
                        error: "empty handoff note".into(),
                    }
                } else {
                    TaskResult::HandoffReady {
                        agent_id,
                        note,
                        task,
                    }
                }
            }
            Err(e) => TaskResult::HandoffFailed {
                agent_id,
                error: sanitize_user_error(&format!("handoff failed: {e}")),
            },
        }
    });
}
