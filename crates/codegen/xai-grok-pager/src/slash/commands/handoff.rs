//! `/handoff` -- transfer task-relevant context into a new empty session.
//!
//! Generates a focused handoff note (only details relevant to the given task),
//! creates a peer empty session linked from the parent, and seeds the child's
//! first prompt with the note + task.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// `/handoff <task>` slash command.
pub struct HandoffCommand;

impl SlashCommand for HandoffCommand {
    fn name(&self) -> &str {
        "handoff"
    }

    fn description(&self) -> &str {
        "Hand off task-relevant context to a new session"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn usage(&self) -> &str {
        "/handoff <task for the new session>"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("<task>")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let task = args.trim();
        if task.is_empty() {
            return CommandResult::Error(
                "Usage: /handoff <task for the new session>".into(),
            );
        }
        CommandResult::Action(Action::Handoff {
            task: task.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;

    fn make_ctx(models: &ModelState) -> CommandExecCtx<'_> {
        let bundle = Box::leak(Box::new(crate::app::bundle::BundleState::default()));
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            billing_surface_visible: true,
            usage_command_visible: true,
            pager_state: crate::settings::PagerLocalSnapshot {
                multiline_mode: false,
                yolo_mode: false,
                ..crate::settings::PagerLocalSnapshot::default()
            },
        }
    }

    #[test]
    fn run_requires_task() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        match HandoffCommand.run(&mut ctx, "  ") {
            CommandResult::Error(msg) => assert!(msg.contains("Usage"), "got: {msg}"),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn run_with_task_returns_action() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        match HandoffCommand.run(&mut ctx, "implement the rest") {
            CommandResult::Action(Action::Handoff { task }) => {
                assert_eq!(task, "implement the rest");
            }
            other => panic!("expected Action(Handoff), got {other:?}"),
        }
    }

    #[test]
    fn metadata_matches_design() {
        let cmd = HandoffCommand;
        assert_eq!(cmd.name(), "handoff");
        assert!(cmd.takes_args());
        assert!(cmd.args_required());
        assert!(cmd.session_scoped());
        assert_eq!(cmd.arg_placeholder(), Some("<task>"));
    }
}
