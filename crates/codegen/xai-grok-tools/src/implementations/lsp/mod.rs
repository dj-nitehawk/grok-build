//! Language server integration.
//!
//! Full client/manager stack requires feature `lsp` (`async-lsp`). Without it,
//! [`stub`] provides a stable public surface so workspace/agent still compile;
//! the LSP tool reports unavailable at runtime.

pub mod config;
mod types;

#[cfg(feature = "lsp")]
mod watched_files;
#[cfg(feature = "lsp")]
pub mod capabilities;
#[cfg(feature = "lsp")]
pub mod client;
#[cfg(feature = "lsp")]
pub mod diagnostics;
#[cfg(feature = "lsp")]
pub mod dispatch;
#[cfg(feature = "lsp")]
pub mod documents;
#[cfg(feature = "lsp")]
pub mod format;
#[cfg(feature = "lsp")]
pub mod manager;
#[cfg(feature = "lsp")]
pub mod pending;
#[cfg(feature = "lsp")]
pub mod pull;
#[cfg(feature = "lsp")]
pub mod refresh;
#[cfg(feature = "lsp")]
pub mod restart;
#[cfg(feature = "lsp")]
pub mod workspace_open;

#[cfg(feature = "lsp")]
#[cfg(test)]
mod tests;

#[cfg(not(feature = "lsp"))]
mod stub;

#[cfg(feature = "lsp")]
pub use dispatch::LspBackendAdapter;
#[cfg(feature = "lsp")]
pub use manager::{LspManager, drain_lsp_diagnostics};
#[cfg(feature = "lsp")]
pub use restart::restart_monitor;

#[cfg(not(feature = "lsp"))]
pub use stub::{LspBackendAdapter, LspManager, drain_lsp_diagnostics, restart_monitor};

pub use types::{
    DiagnosticEntry, DiagnosticSeverityLevel, DiagnosticsSummary, DiskChangeKind, FileDiagnosticEntry, LspBackend,
    LspConfig, LspOperation, LspToolInput, LspToolResult,
};

// ── Shared helpers (full stack only) ────────────────────────────────────

#[cfg(feature = "lsp")]
use std::path::Path;
#[cfg(feature = "lsp")]
use std::sync::Arc;

#[cfg(feature = "lsp")]
use async_lsp::lsp_types::{Position, TextDocumentIdentifier, TextDocumentPositionParams, Url};

/// How long a reader will wait for diagnostics to arrive after an edit before
/// reporting what it has.
///
/// This is the budget the whole after-edit diagnostics path is sized against:
/// anything scheduled to happen later than this — a pull retry, say — answers
/// after the reader has already given up. Kept here, next to the pieces that
/// have to agree on it, rather than as a number at the call site.
///
/// Always available (not feature-gated) so the diagnostics reminder can share
/// the same budget when feature `lsp` is off and only the stub backend is linked.
pub const DIAGNOSTICS_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

#[cfg(feature = "lsp")]
#[derive(Debug, thiserror::Error)]
pub enum LspError {
    #[error("failed to spawn LSP server: {0}")]
    SpawnFailed(String),
    #[error("LSP server '{0}' timed out after {1:?}")]
    Timeout(String, std::time::Duration),
    #[error("LSP initialization failed: {0}")]
    InitFailed(String),
    #[error("LSP request failed: {0}")]
    RequestFailed(String),
    #[error("invalid file path")]
    InvalidPath,
}

#[cfg(feature = "lsp")]
pub type DiagnosticsNotify = Arc<tokio::sync::Notify>;
#[cfg(feature = "lsp")]
pub type LspMainLoop = async_lsp::MainLoop<async_lsp::router::Router<()>>;

#[cfg(feature = "lsp")]
pub fn file_uri(path: &Path) -> Result<Url, LspError> {
    Url::from_file_path(path).map_err(|_| LspError::InvalidPath)
}

#[cfg(feature = "lsp")]
pub fn text_document_position(
    path: &Path,
    line: u32,
    column: u32,
) -> Result<TextDocumentPositionParams, LspError> {
    Ok(TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: file_uri(path)?,
        },
        position: Position {
            line,
            character: column,
        },
    })
}
