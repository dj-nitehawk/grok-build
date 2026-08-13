//! Diagnostics-server handle used by hub connect lifecycle.
//!
//! Real [`xai_grok_diag_server::DiagHandle`] when feature `diag-server` is on
//! (workspace-server bin). Unit stub when off so the TUI lib stays linked
//! without the diag crate; slim leaves `HubConfig::diag` as `None`.

#[cfg(feature = "diag-server")]
pub use xai_grok_diag_server::DiagHandle;

#[cfg(not(feature = "diag-server"))]
#[derive(Debug, Clone)]
pub struct DiagHandle;

#[cfg(not(feature = "diag-server"))]
impl DiagHandle {
    pub fn set_connected(&self) {}

    pub fn set_disconnected(&self) {}

    pub fn set_terminal_close(&self, _code: u16) {}
}
