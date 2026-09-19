//! Subscription usage adapter. No Grok auth or payment state is involved.
use super::{ExtResult, to_raw_response};
use agent_client_protocol as acp;

pub async fn handle(args: &acp::ExtRequest) -> ExtResult {
    #[derive(serde::Deserialize)]
    struct Params {
        identity: Option<String>,
    }
    let params: Params =
        serde_json::from_str(args.params.get()).map_err(|_| acp::Error::invalid_params())?;
    let quota = crate::agent::chatgpt::quota::fetch(params.identity.as_deref())
        .await
        .map_err(|error| acp::Error::internal_error().data(error))?;
    to_raw_response(&quota)
}
