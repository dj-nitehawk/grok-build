//! Facade over `xai-grok-foreign-sessions`.
//!
//! When feature `foreign-sessions` is on, this re-exports the leaf crate.
//! When off, types stay (shell stubs) and scan helpers no-op so welcome /
//! effects / event_loop keep stable paths without linking the crate.

#[cfg(feature = "foreign-sessions")]
pub use xai_grok_foreign_sessions::*;

#[cfg(not(feature = "foreign-sessions"))]
pub use xai_grok_shell::foreign_sessions::*;
