//! Auto-update facade for the composition-root binary.
//!
//! When feature `auto-update` is enabled, re-exports `xai-grok-update`.
//! When off (fork slim default), provides no-op stubs so call sites compile
//! without linking the updater download path.

#![allow(dead_code)]

#[cfg(feature = "auto-update")]
pub use xai_grok_update::{
    UpdateConfig, auto_update, channel_label, channel_name, enforce_version_policy_or_exit,
};

#[cfg(not(feature = "auto-update"))]
pub use stub::*;

#[cfg(not(feature = "auto-update"))]
mod stub {
    use anyhow::{Result, bail};

    #[derive(Clone, Debug, Default)]
    pub struct UpdateConfig {
        pub proxy_base_url: String,
        pub auth_scope: String,
        pub deployment_key: Option<String>,
        pub alpha_test_key: Option<String>,
        pub channel: String,
        pub npm_registry: Option<String>,
    }

    impl UpdateConfig {
        pub fn from_environment(_env: &xai_grok_shell::env::GrokBuildEnvironment) -> Self {
            Self {
                channel: "stable".into(),
                ..Self::default()
            }
        }
    }

    pub fn enforce_version_policy_or_exit() {}

    pub fn channel_label() -> &'static str {
        ""
    }

    pub fn channel_name() -> Option<&'static str> {
        None
    }

    pub mod auto_update {
        use super::UpdateConfig;
        use anyhow::{Result, bail};

        // Same type the real updater re-exports; call sites use auto_update::CliUpdateTrigger.
        pub use xai_grok_telemetry::events::CliUpdateTrigger;

        #[derive(Clone, Copy, Debug)]
        pub enum UpdateRunMode {
            Blocking,
            NonBlocking,
        }

        #[derive(Clone, Debug)]
        pub struct UpdateAvailable {
            pub latest_version: String,
        }

        #[derive(Clone, Debug)]
        pub struct UpdateStatus {
            pub current_version: String,
            pub latest_version: Option<String>,
            pub update_available: bool,
            pub installer: Option<String>,
            pub channel: String,
            pub auto_update: Option<bool>,
            pub error: Option<String>,
        }

        #[derive(Debug)]
        pub struct EnsureLatestOutcome {
            pub installed: Option<String>,
            pub relaunch_needed: bool,
        }

        #[derive(Debug)]
        pub struct BackgroundUpdateCheck {
            pub update: Option<UpdateAvailable>,
            pub download: Option<tokio::process::Child>,
        }

        pub async fn run_update_if_available(
            _mode: UpdateRunMode,
            _interactive: bool,
            _trigger: CliUpdateTrigger,
            _config: &UpdateConfig,
        ) -> Result<bool> {
            Ok(false)
        }

        pub async fn ensure_latest_on_disk(
            _config: &UpdateConfig,
        ) -> Result<EnsureLatestOutcome> {
            Ok(EnsureLatestOutcome {
                installed: None,
                relaunch_needed: false,
            })
        }

        pub async fn check_update_background(
            _config: &UpdateConfig,
        ) -> BackgroundUpdateCheck {
            BackgroundUpdateCheck {
                update: None,
                download: None,
            }
        }

        pub async fn apply_channel_switch(
            _channel_switch: Option<&str>,
            _update_config: &mut UpdateConfig,
        ) {
        }

        pub async fn check_update_status(_update_config: &UpdateConfig) -> UpdateStatus {
            UpdateStatus {
                current_version: env!("CARGO_PKG_VERSION").into(),
                latest_version: None,
                update_available: false,
                installer: None,
                channel: "stable".into(),
                auto_update: Some(false),
                error: Some("auto-update support is not compiled into this build".into()),
            }
        }

        pub fn print_update_status(status: &UpdateStatus, json: bool) -> Result<()> {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "currentVersion": status.current_version,
                        "updateAvailable": false,
                        "error": status.error,
                    })
                );
                return Ok(());
            }
            eprintln!(
                "auto-update support is not compiled into this build (rebuild with --features auto-update)"
            );
            Ok(())
        }

        pub async fn run_update(
            _force_reinstall: bool,
            _version: Option<&str>,
            _channel_switch: Option<&str>,
            _update_config: &mut UpdateConfig,
            _trigger: CliUpdateTrigger,
        ) -> Result<Option<String>> {
            bail!(
                "auto-update support is not compiled into this build (rebuild with --features auto-update)"
            )
        }
    }
}
