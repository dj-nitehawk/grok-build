//! Tool-server and harness SDK.
//!
//! Single crate hosting both the tool-server runtime and the
//! harness-side dispatch surface. The shared substrate —
//! [`HubConnectionPool`], [`HubConnection`], the inbound demux, the
//! refcount-managed bound-session set, and the transparent reconnect /
//! replay state machine — lives here so both ends speak through one
//! frame multiplex on top of one WebSocket per `(url, principal)`.
//!
//! The server entry point is [`ToolServer`]: build it via
//! [`ToolServerBuilder`], wire one or more [`ToolServerHandler`]
//! implementations, and call [`ToolServer::run`] to drive the inbound
//! loop. The harness entry point is [`ToolHarness`]: build it via
//! [`ToolHarnessBuilder`], optionally seed it with in-process
//! [`xai_tool_runtime::Tool`] implementations, and call
//! [`ToolHarness::call`] to dispatch a tool call. Authorisation
//! credentials (`AuthCredential`) plus the target URL determine
//! which pool entry the consumer attaches to; multiple
//! [`ToolServer`] / [`ToolHarness`] instances against the same
//! `(url, principal)` share a single connection and refcount their
//! session bindings.

#![forbid(unsafe_code)]

pub(crate) mod admission;
pub mod auth;
pub(crate) mod cancel;
pub mod connection;
pub(crate) mod connection_borrow;
pub mod demux;
pub mod discovery;
#[cfg(feature = "telemetry-donate")]
pub(crate) mod donate_pump;
pub mod error;
pub mod handshake;
pub mod harness;
#[cfg(feature = "telemetry-donate")]
pub mod log_donate;
#[cfg(feature = "metrics")]
pub mod metric_donate;
/// No-op metric donation types when feature `metrics` is off.
#[cfg(not(feature = "metrics"))]
mod metric_donate_stub {
    use crate::server::ToolServer;

    pub struct MetricDonationPump;

    impl MetricDonationPump {
        pub async fn drain(&self) {}
    }

    impl ToolServer {
        pub fn metric_donation_reporter(
            &self,
            _service_name: impl Into<String>,
        ) -> MetricDonationPump {
            MetricDonationPump
        }
    }
}
pub mod metrics;
pub mod notification;
pub mod observability;
pub mod pool;
pub mod refcount;
pub mod server;
#[cfg(feature = "telemetry-donate")]
pub mod trace_donate;
/// Shape-stable no-ops when OTLP donation is compiled out.
#[cfg(not(feature = "telemetry-donate"))]
mod donate_stub;

pub mod oidc_provider;

pub use auth::{AuthCredential, AuthIdentity, AuthProvider, PrincipalKey, SharedAuthProvider};
pub use connection::{CLOSE_CODE_SANDBOX_TERMINATED, ConnKey, HubConnection, ReconnectEvent};
pub use error::ClientError;
pub use harness::{
    CancelOnDrop, LocalRegistry, ModelOutputExtractor, SessionBindReport, ToolHarness,
    ToolHarnessBuilder, extractor_for,
};
#[cfg(feature = "telemetry-donate")]
pub use log_donate::{DonatingLogLayer, LogDonationPump, LogDonationSender, flush_log_layer};
#[cfg(not(feature = "telemetry-donate"))]
pub use donate_stub::{DonatingLogLayer, LogDonationPump, LogDonationSender, flush_log_layer};
#[cfg(feature = "metrics")]
pub use metric_donate::MetricDonationPump;
#[cfg(not(feature = "metrics"))]
pub use metric_donate_stub::MetricDonationPump;
pub use notification::HubNotification;
pub use observability::ObservabilityBridge;
pub use oidc_provider::{
    OidcAuthProvider, OidcAuthProviderBuilder, OnRefreshCallback, RefreshEvent,
};
pub use pool::HubConnectionPool;
pub use server::{
    ResolvedSessionHandlers, SessionHandlerResolver, SessionUnboundCallback, SystemNotifyAck,
    ToolServer, ToolServerBuilder, ToolServerHandler, WeakToolServer,
};
#[cfg(feature = "telemetry-donate")]
pub use trace_donate::{HubDonatingReporter, TraceDonationPump};
#[cfg(not(feature = "telemetry-donate"))]
pub use donate_stub::{HubDonatingReporter, TraceDonationPump};
pub use xai_computer_hub_core::{
    GROK_BOT_TOOL_DESCRIPTIONS, GROK_BOT_TOOL_IDS, grok_bot_tool_arguments_schema,
    grok_bot_tool_description, is_grok_bot_tool,
};
// Re-exported so consumers that depend only on the SDK can recognize the
// server's `workspace_unavailable` error without also pulling in the core crate.
pub use xai_computer_hub_core::is_workspace_unavailable;
