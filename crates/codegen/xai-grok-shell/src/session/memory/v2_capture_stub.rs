//! Slim stand-in for `v2_capture` when product memory is compiled out.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlushResult {
    Success,
    RetryableFailure(xai_grok_telemetry::memory_telemetry::MemoryV2FailureClass),
    TerminalFailure(xai_grok_telemetry::memory_telemetry::MemoryV2FailureClass),
    Timeout,
}
