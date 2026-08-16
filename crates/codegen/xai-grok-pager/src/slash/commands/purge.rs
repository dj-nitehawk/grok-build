//! `/purge` -- permanently delete all session history and logs, then exit.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Permanently delete all session history and logs, then quit.
pub struct PurgeCommand;

impl SlashCommand for PurgeCommand {
    fn name(&self) -> &str {
        "purge"
    }

    fn description(&self) -> &str {
        "Delete all session history and logs, then exit"
    }

    fn usage(&self) -> &str {
        "/purge"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        if !args.trim().is_empty() {
            tracing::warn!("/purge does not accept arguments; ignoring");
        }
        CommandResult::Action(Action::PurgeAndQuit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::bundle::BundleState;
    use crate::slash::command::CommandExecCtx;

    fn make_ctx(models: &ModelState) -> CommandExecCtx<'_> {
        static BUNDLE: BundleState = BundleState {
            has_cache: false,
            version: String::new(),
            personas: Vec::new(),
            roles: Vec::new(),
            agents: Vec::new(),
            skills: Vec::new(),
            persona_details: Vec::new(),
            role_details: Vec::new(),
        };
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: &BUNDLE,
            screen_mode: crate::app::ScreenMode::Inline,
            billing_surface_visible: true,
            usage_command_visible: true,
            pager_state: crate::settings::PagerLocalSnapshot::default(),
        }
    }

    #[test]
    fn bare_purge_dispatches_immediately() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let result = PurgeCommand.run(&mut ctx, "");
        assert!(
            matches!(result, CommandResult::Action(Action::PurgeAndQuit)),
            "bare /purge must purge and quit; got {result:?}"
        );
    }

    #[test]
    fn args_are_ignored() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let result = PurgeCommand.run(&mut ctx, "confirm");
        assert!(
            matches!(result, CommandResult::Action(Action::PurgeAndQuit)),
            "args must not block purge; got {result:?}"
        );
    }
}
