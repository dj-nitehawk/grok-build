//! Stub `WebFetchClient` when feature `web-fetch` is off (no htmd/scraper).

use std::path::Path;

use super::config::WebFetchParams;
use super::error::WebFetchError;
use crate::types::output::WebFetchOutput;

/// Placeholder client when HTML fetch stack is not compiled in.
#[derive(Clone)]
pub struct WebFetchClient;

impl WebFetchClient {
    pub fn new(_params: &WebFetchParams) -> Result<Self, WebFetchError> {
        Err(WebFetchError::NotCompiledIn)
    }

    pub async fn fetch(
        &self,
        _raw_url: &str,
        _session_folder: Option<&Path>,
        _read_tool_name: Option<&str>,
        _execute_tool_name: Option<&str>,
    ) -> Result<WebFetchOutput, WebFetchError> {
        Err(WebFetchError::NotCompiledIn)
    }
}
