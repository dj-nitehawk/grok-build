//! `/handoff` dispatch: task-scoped note generation and peer session seed.
//!
//! Kept out of [`super::fork`] so upstream fork changes rebase cleanly; only
//! thin registration remains in the router / task_result switchboards.

use super::fork::build_fork_placeholder;
use crate::app::actions::Effect;
use crate::app::agent::AgentId;
use crate::app::agent_view::AgentView;
use crate::app::app_view::{ActiveView, AppView};
use crate::app::dispatch::ctx::{SwitchCause, switch_to_agent};
use crate::scrollback::block::RenderBlock;
use crate::scrollback::entry::ScrollbackEntry;

/// Finish (or replace) the in-flight `/handoff` loading entry with a final
/// system message. Mirrors the manual `/recap` fill-in-place path so the
/// animated running sidebar stops when generation ends.
fn resolve_pending_handoff_entry(agent: &mut AgentView, message: String) {
    let fill_id = agent
        .pending_handoff_entry
        .take()
        .filter(|&id| agent.scrollback.get_by_id(id).is_some());
    let block = RenderBlock::system(message);
    match fill_id {
        Some(id) if agent.scrollback.is_committed(id) => {
            agent.scrollback.remove_entry(id);
            agent.scrollback.push_block(block);
        }
        Some(id) => {
            if let Some(entry) = agent.scrollback.get_by_id_mut(id) {
                entry.block = block;
            }
            agent.scrollback.finish_running(id);
        }
        None => {
            agent.scrollback.push_block(block);
        }
    }
}

/// Top-level `/handoff` dispatcher. Requests a task-scoped note from the
/// shell (`x.ai/handoff`) on the parent session. On success,
/// [`handle_handoff_ready`] opens a new empty peer session and seeds it.
///
/// Shows an immediate running scrollback entry (animated sidebar) so the user
/// has durable feedback while the model call runs — toasts alone expire after
/// a few seconds.
pub(in crate::app::dispatch) fn dispatch_handoff(app: &mut AppView, task: String) -> Vec<Effect> {
    let ActiveView::Agent(parent_id) = app.active_view else {
        app.show_toast("/handoff only works inside a session");
        return vec![];
    };
    let Some(parent) = app.agents.get_mut(&parent_id) else {
        return vec![];
    };
    let Some(session_id) = parent.session.session_id.clone() else {
        app.show_toast("Cannot hand off: session is still being created");
        return vec![];
    };
    // Refuse a second request while one is still generating so we do not
    // stack spinners or orphan a parent Effect response.
    let already_loading = parent.pending_handoff_entry.is_some_and(|eid| {
        parent
            .scrollback
            .get_by_id(eid)
            .is_some_and(|entry| entry.is_running)
    });
    if already_loading {
        parent.show_toast("Handoff already in progress");
        return vec![];
    }
    parent.prompt.set_text("");
    let entry_id = parent.scrollback.push(ScrollbackEntry::running(RenderBlock::system(
        format!("Generating handoff note \u{2014} {task}"),
    )));
    parent.pending_handoff_entry = Some(entry_id);
    vec![Effect::Handoff {
        agent_id: parent_id,
        session_id,
        task,
    }]
}

/// Open a new empty peer session seeded with the handoff note + task.
pub(in crate::app::dispatch) fn handle_handoff_ready(
    app: &mut AppView,
    parent_id: AgentId,
    note: String,
    task: String,
) -> Vec<Effect> {
    let Some(parent) = app.agents.get(&parent_id) else {
        app.show_toast("Handoff parent session is gone");
        return vec![];
    };
    let parent_cwd = parent.session.cwd.clone();
    let parent_sid = parent
        .session
        .session_id
        .as_ref()
        .map(|s| s.0.to_string())
        .unwrap_or_default();
    let parent_chat_kind = parent.chat_kind || app.chat_mode;

    let seed = format!(
        "# Handoff context\n\n{}\n\n---\n\n## Task\n{}",
        note.trim(),
        task.trim()
    );

    let new_id = AgentId(app.next_agent_id);
    app.next_agent_id += 1;
    // Reuse the fork placeholder (empty agent peer). We CreateSession
    // instead of ForkSession so history is not copied. Clear the fork
    // command spinner that the placeholder starts; CreateSession has no
    // matching finish path for ForkSession.
    let mut new_agent = build_fork_placeholder(app, new_id, parent_id, &parent_cwd, false);
    new_agent.session.finish_command();
    new_agent.mark_turn_finished();
    app.agents.insert(new_id, new_agent);
    {
        let agent = app
            .agents
            .get_mut(&new_id)
            .expect("just-inserted handoff agent missing");
        agent.prompt.set_compact(app.appearance.prompt.compact);
        agent.prompt.adopt_slash_mru(app.slash_mru.clone());
        agent.prompt.adopt_command_tags(app.command_tags.clone());
        agent
            .prompt
            .set_contextual_hints(app.contextual_hints.undo, app.contextual_hints.plan_mode);
        agent.set_session_recap_available(app.session_recap_available);
        agent.set_voice_mode_available(app.voice_mode_enabled);
        agent.apply_app_scoped_gates(
            app.sharing_enabled,
            app.usage_visible,
            app.chat_mode,
            app.screen_mode,
            &app.active_announcements,
            &app.tier_restricted_commands,
        );
        agent.chat_kind = parent_chat_kind;
        agent.apply_credit_balance(app.credit_balance.clone(), app.auto_topup.clone());
        agent
            .prompt
            .slash_controller
            .registry_mut()
            .set_plugins_visible(!app.appearance.disable_plugins);
        agent.pending_fork_banner = Some(crate::app::agent_view::PendingForkBanner {
            parent_sid,
            worktree: false,
        });
        // Seed via the prompt queue so SessionCreated's drain sends it.
        agent.session.enqueue_prompt_front(seed);
        agent.session.created_via_new = true;
    }
    if let Some(parent_mut) = app.agents.get_mut(&parent_id) {
        resolve_pending_handoff_entry(parent_mut, format!("Handed off: {task}"));
    }
    switch_to_agent(app, new_id, SwitchCause::Fork);
    if let Some(d) = app.dashboard.as_mut()
        && d.attached_agent == Some(parent_id)
    {
        d.attached_agent = Some(new_id);
        d.focus_row(crate::views::dashboard::DashboardRowId::TopLevel(new_id));
    }
    vec![Effect::CreateSession {
        agent_id: new_id,
        cwd: parent_cwd,
        model_id: None,
        preferred_session_id: None,
        chat_kind: parent_chat_kind,
    }]
}

pub(in crate::app::dispatch) fn handle_handoff_failed(
    app: &mut AppView,
    agent_id: AgentId,
    error: String,
) -> Vec<Effect> {
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        agent.show_toast(&format!("Handoff failed: {error}"));
        resolve_pending_handoff_entry(agent, format!("Handoff failed: {error}"));
    } else {
        app.show_toast(&format!("Handoff failed: {error}"));
    }
    vec![]
}
