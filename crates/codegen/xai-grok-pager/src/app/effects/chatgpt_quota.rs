//! ChatGPT quota effect. Capture identity before crossing the async boundary.
use crate::app::{actions::TaskResult, provider_quota::Request};
use agent_client_protocol as acp;
use tokio::task::JoinSet;
use xai_acp_lib::AcpAgentTx;

use super::acp_send_bounded;

pub(super) fn spawn(tasks: &mut JoinSet<TaskResult>, tx: &AcpAgentTx, request: Request) {
    let tx = tx.clone();
    tasks.spawn(async move {
        let params = serde_json::json!({ "identity": request.identity });
        let req = acp::ExtRequest::new(
            "x.ai/chatgpt/quota",
            serde_json::value::to_raw_value(&params)
                .expect("quota params")
                .into(),
        );
        let result = match acp_send_bounded(req, &tx, "ChatGPT quota").await {
            Ok(response) => serde_json::from_str::<serde_json::Value>(response.0.get())
                .map_err(|_| "Invalid ChatGPT quota response".to_owned())
                .and_then(|value| {
                    if value.get("error").is_some_and(|error| !error.is_null()) {
                        return Err("ChatGPT quota unavailable".into());
                    }
                    serde_json::from_value(value.get("result").unwrap_or(&value).clone())
                        .map_err(|_| "Invalid ChatGPT quota response".to_owned())
                }),
            Err(_) => Err("ChatGPT quota request failed".into()),
        };
        TaskResult::ChatgptQuotaFetched { request, result }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn quota_rpc_timeout_completes_instead_of_remaining_in_flight() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut tasks = JoinSet::new();
        let request = Request {
            identity: Some("account".into()),
            generation: 1,
        };
        spawn(&mut tasks, &tx, request.clone());
        let pending = rx.recv().await.unwrap();
        tokio::time::advance(super::super::helpers::session_rpc_timeout()).await;
        let TaskResult::ChatgptQuotaFetched {
            request: completed,
            result,
        } = tasks.join_next().await.unwrap().unwrap()
        else {
            panic!("expected quota completion")
        };
        assert_eq!(completed, request);
        assert_eq!(result.unwrap_err(), "ChatGPT quota request failed");
        drop(pending);
    }

    #[tokio::test]
    async fn quota_rpc_disconnect_completes_with_safe_error() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);
        let mut tasks = JoinSet::new();
        spawn(
            &mut tasks,
            &tx,
            Request {
                identity: None,
                generation: 1,
            },
        );
        let TaskResult::ChatgptQuotaFetched { result, .. } =
            tasks.join_next().await.unwrap().unwrap()
        else {
            panic!("expected quota completion")
        };
        assert_eq!(result.unwrap_err(), "ChatGPT quota request failed");
    }
}
