//! Compile-out stub when feature `lsp` is off (no `async-lsp` link).
//!
//! Public types/traits match the full module surface used by workspace, agent,
//! and tool registry so mid-stack call sites stay stable across main syncs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::config::LspServerConfig;
use super::types::{
    DiagnosticsSummary, DiskChangeKind, FileDiagnosticEntry, LspBackend, LspToolInput,
    LspToolResult,
};

pub struct LspManager {
    pub servers: BTreeMap<String, LspServerConfig>,
    pub workspace_root: PathBuf,
    pub tools_enabled: bool,
    pub notification_handle: crate::notification::ToolNotificationHandle,
    pub process_scope: Option<crate::util::ProcessScope>,
}

impl LspManager {
    pub fn new(
        servers: BTreeMap<String, LspServerConfig>,
        workspace_root: PathBuf,
        tools_enabled: bool,
        notification_handle: crate::notification::ToolNotificationHandle,
    ) -> Self {
        Self {
            servers,
            workspace_root,
            tools_enabled,
            notification_handle,
            process_scope: None,
        }
    }

    pub fn with_process_scope(mut self, scope: Option<crate::util::ProcessScope>) -> Self {
        self.process_scope = scope;
        self
    }
}

pub struct LspBackendAdapter {
    _mgr: Arc<tokio::sync::Mutex<LspManager>>,
}

impl LspBackendAdapter {
    pub fn new(lsp_manager: Arc<tokio::sync::Mutex<LspManager>>) -> Self {
        Self { _mgr: lsp_manager }
    }
}

#[async_trait::async_trait]
impl LspBackend for LspBackendAdapter {
    fn ensure_started_background(&self) {}

    async fn ensure_ready(&self) -> Result<(), String> {
        Err(not_compiled().into())
    }

    fn is_ready(&self) -> bool {
        false
    }

    async fn dispatch(&self, _input: &LspToolInput) -> LspToolResult {
        LspToolResult {
            text: not_compiled().into(),
            is_error: true,
        }
    }

    async fn drain_diagnostics(&self, _timeout: std::time::Duration) -> Option<DiagnosticsSummary> {
        None
    }

    async fn notify_file_changed(&self, _path: &Path, _content: &str) {}

    async fn notify_file_event(
        &self,
        _path: &Path,
        _content: Option<&str>,
        _kind: DiskChangeKind,
    ) {
    }

    async fn read_diagnostics(&self, _paths: &[PathBuf]) -> Vec<FileDiagnosticEntry> {
        Vec::new()
    }
}

pub async fn drain_lsp_diagnostics(
    _mgr: Arc<tokio::sync::Mutex<LspManager>>,
    _timeout: std::time::Duration,
) -> Option<DiagnosticsSummary> {
    None
}

pub async fn restart_monitor(
    _mgr: std::sync::Weak<tokio::sync::Mutex<LspManager>>,
    _name: String,
) {
}

fn not_compiled() -> &'static str {
    "LSP support is not compiled into this build (missing feature `lsp`)"
}
