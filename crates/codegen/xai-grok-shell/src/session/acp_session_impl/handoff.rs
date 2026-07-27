//! Task-scoped handoff note generation for `SessionActor`.
//!
//! Fork-owned orchestration: keeps the model-call path out of `recap.rs`
//! so upstream recap edits do not collide with handoff on every sync.

use super::*;

use crate::remote::DEFAULT_CONTEXT_WINDOW;

impl SessionActor {
    /// Generate a task-scoped handoff note without mutating conversation.
    ///
    /// Snapshots history, appends a handoff instruction that requires keeping
    /// only details relevant to `task`, makes one tool-free model call, and
    /// returns the cleaned note for the client to seed a new empty session.
    pub(super) async fn handle_handoff(&self, task: &str) -> Result<String, String> {
        use crate::session::helpers::session_handoff;

        let task = task.trim();
        if task.is_empty() {
            return Err("handoff requires a task".into());
        }

        let sampling_client = self
            .prepare_chat_completion(false)
            .await
            .map_err(|e| format!("failed to prepare client: {e}"))?;

        let conversation = self.chat_state_handle.get_conversation().await;
        if conversation.is_empty() {
            return Err("no conversation to hand off".into());
        }

        let tag = self.reminder_wrapper_tag();
        let strip_reasoning =
            sampling_client.api_backend() == crate::sampling::ApiBackend::Messages;
        let sampling_config = self.chat_state_handle.get_sampling_config().await;
        let context_window = sampling_config
            .as_ref()
            .map(|c| c.context_window.get())
            .unwrap_or(DEFAULT_CONTEXT_WINDOW);
        let model = sampling_config.map(|c| c.model).unwrap_or_default();

        let items = session_handoff::budget_handoff_items(
            conversation,
            tag,
            task,
            strip_reasoning,
            context_window,
        );

        // Include tool specs in the prefix (like recap) so the cached prefix
        // stays warm, but the instruction forbids tool use and we do not
        // execute any tool calls from this response.
        let tool_defs = self.prepare_tool_definitions().await;
        let tools = self.turn_base_tool_specs(&tool_defs);
        let hosted_tools = self.hosted_tools_for_turn();

        let request = ConversationRequest {
            items,
            tools,
            hosted_tools,
            model: Some(model.clone()),
            temperature: None,
            x_grok_conv_id: Some(format!("handoff-{}", uuid::Uuid::new_v4())),
            x_grok_req_id: Some(format!("xai-handoff-{}", uuid::Uuid::new_v4())),
            x_grok_session_id: Some(self.session_info.id.to_string()),
            x_grok_agent_id: Some(xai_grok_telemetry::id::agent_id()),
            ..Default::default()
        };

        let response = sampling_client
            .conversation_collect(request)
            .await
            .map_err(|e| format!("handoff model call failed: {e}"))?;
        let content = response.assistant_text();
        if content.trim().is_empty() {
            return Err("No handoff note from model".into());
        }

        let note = session_handoff::clean_handoff_text(&content);
        if note.is_empty() {
            return Err("Handoff note was empty after cleanup".into());
        }
        Ok(note)
    }
}
