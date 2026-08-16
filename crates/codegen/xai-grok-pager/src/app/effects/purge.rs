//! `Effect::PurgeAndQuit` async spawn (fork `/purge`).
//!
//! Kept out of the main effects match so upstream effect arms rebase cleanly.

use super::helpers::sanitize_user_error;
use crate::app::actions::TaskResult;
use agent_client_protocol as acp;
use tokio::task::JoinSet;
use xai_acp_lib::{AcpAgentTx, acp_send};

/// Spawn the `x.ai/session/purge` call; results land as
/// [`TaskResult::PurgeComplete`] / [`TaskResult::PurgeFailed`].
pub(super) fn spawn_purge_and_quit(tasks: &mut JoinSet<TaskResult>, acp_tx: &AcpAgentTx) {
    let tx = acp_tx.clone();
    tasks.spawn(async move {
        let request = acp::ExtRequest::new(
            "x.ai/session/purge",
            serde_json::value::to_raw_value(&serde_json::json!({}))
                .expect("serialize empty purge params")
                .into(),
        );
        match acp_send(request, &tx).await {
            Ok(resp) => {
                let wrapper: serde_json::Value =
                    serde_json::from_str(resp.0.get()).unwrap_or_default();
                if let Some(err) = wrapper.get("error").filter(|v| !v.is_null()) {
                    let msg = err
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| err.to_string());
                    return TaskResult::PurgeFailed { error: msg };
                }
                let sessions = wrapper
                    .get("sessionsRemoved")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let logs = wrapper
                    .get("logsDirEntriesCleared")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let errors = wrapper
                    .get("errors")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let summary = if errors == 0 {
                    format!("Purged {sessions} session(s); cleared logs ({logs} entries)")
                } else {
                    format!(
                        "Purged {sessions} session(s); cleared logs ({logs} entries); {errors} warning(s)"
                    )
                };
                TaskResult::PurgeComplete { summary }
            }
            Err(e) => TaskResult::PurgeFailed {
                error: sanitize_user_error(&format!("couldn't purge history: {e}")),
            },
        }
    });
}
