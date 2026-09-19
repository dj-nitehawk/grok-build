use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};

pub struct ChatgptLoginCommand;

impl SlashCommand for ChatgptLoginCommand {
    slash_meta! {
        name: "chatgpt-login",
        description: "Sign in with ChatGPT Plus/Pro (Codex backend)",
        usage: "/chatgpt-login",
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let device = args.trim() == "--device";
        if device {
            CommandResult::Message(
                "ChatGPT device login cannot run inside the TUI (it prints a code and waits).\n\
                 In a terminal, run:\n  grok chatgpt-login --device\n\
                 Then switch with `/model gpt-5.6-sol` or `/model gpt-6-astra`."
                    .to_owned(),
            )
        } else {
            CommandResult::Message(
                "ChatGPT login binds localhost:1455 and cannot run inside the TUI.\n\
                 In a terminal, run:\n  grok chatgpt-login\n\
                 Headless: grok chatgpt-login --device\n\
                 Then switch with `/model gpt-5.6-sol` or `/model gpt-6-astra`.\n\
                 If port 1455 is busy (OpenCode or Codex CLI), use --device."
                    .to_owned(),
            )
        }
    }
}
