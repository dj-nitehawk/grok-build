//! Thin facade over `xai-grok-plugin-marketplace` for UI call sites.
//!
//! When feature `marketplace` is off, constants stay for string layout only and
//! matchers/official-source probes no-op so CTA/install paths stay compiled.

#[cfg(feature = "marketplace")]
pub use xai_grok_plugin_marketplace::{
    OFFICIAL_SOURCE_GIT_URL, OFFICIAL_SOURCE_NAME, is_official_source_url,
};

#[cfg(not(feature = "marketplace"))]
pub const OFFICIAL_SOURCE_NAME: &str = "xAI Official";

#[cfg(not(feature = "marketplace"))]
pub const OFFICIAL_SOURCE_GIT_URL: &str =
    "https://github.com/xai-org/plugin-marketplace.git";

#[cfg(not(feature = "marketplace"))]
#[must_use]
pub fn is_official_source_url(_url: &str) -> bool {
    false
}

#[cfg(feature = "marketplace")]
pub use xai_grok_plugin_marketplace::matcher::{KeywordCandidate, match_plugin_keyword};

/// Keyword candidate for CTA matching (stub when marketplace is off).
#[cfg(not(feature = "marketplace"))]
#[derive(Debug, Clone, Copy)]
pub struct KeywordCandidate<'a> {
    pub name: &'a str,
    pub domains: &'a [String],
    pub keywords: &'a [String],
}

/// Always no match when marketplace is compiled out.
#[cfg(not(feature = "marketplace"))]
#[must_use]
pub fn match_plugin_keyword(
    _prompt: &str,
    _candidates: &[KeywordCandidate<'_>],
) -> Option<usize> {
    None
}
