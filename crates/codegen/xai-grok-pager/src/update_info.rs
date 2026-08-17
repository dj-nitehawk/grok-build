//! Optional auto-update UI helpers.
//!
//! Feature `auto-update` links `xai-grok-update` for channel labels and the
//! background-update payload type. Without it, channel labels are empty and
//! [`UpdateAvailable`] is a local stub (never populated on slim).

/// Version available from a background update check.
#[cfg(feature = "auto-update")]
pub use xai_grok_update::auto_update::UpdateAvailable;

/// Stub when auto-update is not compiled in (receivers stay `None` on slim).
#[cfg(not(feature = "auto-update"))]
#[derive(Debug, Clone)]
pub struct UpdateAvailable {
    pub latest_version: String,
}

/// Release channel label suffix for version badges (e.g. `" (alpha)"`).
#[must_use]
pub fn channel_label() -> &'static str {
    #[cfg(feature = "auto-update")]
    {
        xai_grok_update::channel_label()
    }
    #[cfg(not(feature = "auto-update"))]
    {
        ""
    }
}
