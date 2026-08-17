//! No-op donation surface when feature `telemetry-donate` is off.
//!
//! Keeps public type names stable for workspace / workspace_server call sites
//! without linking the OTel / prost donation stack.

use crate::server::ToolServer;

/// fastrace reporter that drops all spans (donation not compiled in).
pub struct HubDonatingReporter;

impl fastrace::collector::Reporter for HubDonatingReporter {
    fn report(&mut self, _spans: Vec<fastrace::collector::SpanRecord>) {}
}

/// Drain handle for the stub trace donation path.
pub struct TraceDonationPump;

impl TraceDonationPump {
    pub async fn drain(&self) {}
}

/// Sender swapped into an inert [`DonatingLogLayer`]; no-op without donate.
#[derive(Clone, Debug)]
pub struct LogDonationSender;

/// Tracing layer that remains inert when donation is not compiled in.
#[derive(Clone, Debug, Default)]
pub struct DonatingLogLayer;

impl DonatingLogLayer {
    pub fn new_inert() -> Self {
        Self
    }

    pub fn activate(&self, _sender: LogDonationSender) {}
}

impl<S> tracing_subscriber::Layer<S> for DonatingLogLayer
where
    S: tracing::Subscriber,
{
    // inert
}

pub fn flush_log_layer() {}

/// Drain handle for the stub log donation path.
pub struct LogDonationPump;

impl LogDonationPump {
    pub async fn drain(&self) {}
}

impl ToolServer {
    pub fn trace_donation_reporter(
        &self,
        _service_name: impl Into<String>,
    ) -> (HubDonatingReporter, TraceDonationPump) {
        (HubDonatingReporter, TraceDonationPump)
    }

    pub fn log_donation_layer(
        &self,
        _service_name: impl Into<String>,
    ) -> (LogDonationSender, LogDonationPump) {
        (LogDonationSender, LogDonationPump)
    }
}
