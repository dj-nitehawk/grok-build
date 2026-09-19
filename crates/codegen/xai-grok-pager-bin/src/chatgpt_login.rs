//! Thin CLI wrapper around `xai_grok_shell::agent::chatgpt`.

pub async fn run_login(device: bool) -> anyhow::Result<()> {
    xai_grok_shell::agent::chatgpt::run_chatgpt_login(device).await
}

pub fn run_logout() -> anyhow::Result<()> {
    xai_grok_shell::agent::chatgpt::run_chatgpt_logout()
}

pub async fn print_token() -> anyhow::Result<()> {
    xai_grok_shell::agent::chatgpt::print_chatgpt_token().await
}
