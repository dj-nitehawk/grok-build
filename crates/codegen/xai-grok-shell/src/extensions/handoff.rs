//! `x.ai/handoff` extension handler.
//!
//! Generates a task-scoped handoff note from the active session via
//! [`SessionCommand::Handoff`] and returns it to the client. The note is
//! produced without mutating the parent conversation; the client creates a
//! new empty session and seeds the first prompt.

use agent_client_protocol as acp;
use tokio::sync::oneshot;

use super::{ExtResult, parse_params};
use crate::agent::MvpAgent;
use crate::session::SessionCommand;

#[tracing::instrument(skip_all)]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct HandoffRequest {
        session_id: String,
        task: String,
    }

    let req: HandoffRequest = parse_params(args)?;
    tracing::info!("handling /handoff request");

    if req.task.trim().is_empty() {
        return Err(acp::Error::invalid_params().data("task is required"));
    }

    let sid: acp::SessionId = req.session_id.clone().into();
    let Some(session) = agent.get_session_handle(&sid) else {
        return Err(
            acp::Error::invalid_params().data(format!("session not found: {}", req.session_id))
        );
    };

    let (tx, rx) = oneshot::channel();
    let _ = session.cmd_tx.send(SessionCommand::Handoff {
        task: req.task,
        respond_to: tx,
    });
    let result = rx
        .await
        .map_err(|_| acp::Error::internal_error().data("session failed to respond"))?;
    match result {
        Ok(note) => super::to_ext_response(Ok(serde_json::json!({
            "note": note,
        }))),
        Err(e) => Err(acp::Error::internal_error().data(e)),
    }
}
