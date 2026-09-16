//! No-op memory-v2 session methods when feature `memory` is off.
//!
//! Keeps `run_loop` / slash / dream call sites compiling without linking
//! `xai-grok-memory`. Real bodies live in `memory_capture.rs` and siblings.

use super::*;

pub(super) fn is_successful_query_loop(_result: &PromptTurnResult) -> bool {
    false
}

/// Memory is off, so no turn is eligible for v2 capture.
pub(super) fn is_capturable_turn_result(_result: &PromptTurnResult) -> bool {
    false
}

impl SessionActor {
    pub(super) fn v2_capture_enabled(&self) -> bool {
        false
    }

    pub(super) fn is_capturable_front(_state: &State, _prompt_id: &str) -> bool {
        false
    }

    pub(super) async fn enqueue_v2_turn_capture(self: &Arc<Self>, _source_prompt_index: usize) {}

    pub(super) async fn resume_v2_capture(self: &Arc<Self>) {}

    pub(super) async fn flush_v2_capture(
        self: &Arc<Self>,
    ) -> crate::session::memory::v2_capture::FlushResult {
        crate::session::memory::v2_capture::FlushResult::Success
    }

    pub(crate) async fn memory_forget(
        &self,
        _path: &str,
        _expected_content_hash: &str,
    ) -> crate::extensions::memory::MemoryForgetResponse {
        crate::extensions::memory::MemoryForgetResponse::Rejected {
            reason: crate::extensions::memory::MemoryForgetRejection::MemoryDisabled,
            message: "Memory is off for this session.".to_string(),
        }
    }

    pub(super) async fn memory_v2_status(&self) -> String {
        "Memory is compiled out of this build.".to_string()
    }

    pub(super) async fn run_v2_dream_slash_command(
        self: &Arc<Self>,
    ) -> crate::extensions::memory::MemoryDreamResponse {
        crate::extensions::memory::MemoryDreamResponse::new(
            crate::extensions::memory::MemoryDreamDisposition::Disabled,
        )
    }

    pub(crate) fn memory_listing(
        &self,
    ) -> Result<crate::extensions::memory::MemoryListing, String> {
        Ok(crate::extensions::memory::MemoryListing {
            files: vec![],
            enabled: false,
            disabled_reason: Some(
                crate::extensions::notification::MemoryDisabledReason::ProcessDisabled,
            ),
            capture_enabled: false,
            dream_enabled: false,
        })
    }

    pub(crate) async fn memory_toggle_and_list(
        self: &Arc<Self>,
        _enabled: bool,
    ) -> crate::extensions::memory::MemoryToggleResponse {
        crate::extensions::memory::MemoryToggleResponse {
            message: "Memory is compiled out of this build.".to_string(),
            enabled: false,
            disabled_reason: Some(
                crate::extensions::notification::MemoryDisabledReason::ProcessDisabled,
            ),
            listing: self.memory_listing().ok(),
        }
    }

    pub(crate) async fn memory_flush_command(
        self: &Arc<Self>,
    ) -> crate::extensions::memory::MemoryFlushResponse {
        crate::extensions::memory::MemoryFlushResponse {
            flushed: false,
            disposition: crate::extensions::memory::MemoryFlushDisposition::Disabled,
            through_turn: None,
        }
    }

    pub(crate) async fn memory_dream_command(
        self: &Arc<Self>,
    ) -> crate::extensions::memory::MemoryDreamResponse {
        self.run_v2_dream_slash_command().await
    }
}
