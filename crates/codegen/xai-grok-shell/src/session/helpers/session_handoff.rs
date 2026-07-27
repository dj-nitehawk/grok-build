//! Session handoff note generation helpers.
//!
//! A *handoff* is a task-scoped context note for a **new** empty session.
//! Unlike compaction, it never mutates the parent conversation. Unlike
//! recap, it is not display-only: the note is returned to the client so
//! it can seed the child session's first prompt.
//!
//! The critical product rule: keep **only** details relevant to the
//! caller's task. Unrelated history must be omitted.

use crate::sampling::ConversationItem;
use crate::session::helpers::chat::floor_char_boundary;
use xai_chat_state::{compaction_utils, estimate_conversation_tokens, estimate_item_tokens};

/// Hard cap on handoff note length (characters). Generous enough for a
/// focused multi-section note; guards runaway model output.
const HANDOFF_MAX_CHARS: usize = 12_000;

/// Cap on the effective context window for handoff budgeting (same as recap).
const HANDOFF_CONTEXT_WINDOW_CAP: u64 = 500_000;

/// Fraction of the window a handoff request may occupy.
const HANDOFF_BUDGET_THRESHOLD_PERCENT: u64 = 85;

/// Estimator slack so the request stays under the real limit.
const HANDOFF_BUDGET_HEADROOM_TOKENS: u64 = 4_000;

/// Build the instruction turn that steers the model to a task-scoped handoff.
///
/// `task` is the goal for the next agent. The instruction insists that only
/// details needed for that task are retained.
pub(crate) fn handoff_instruction(tag: &str, task: &str) -> String {
    let task = task.trim();
    format!(
        "<{tag}>\
You are writing a handoff note for another AI agent that will continue in a \
**new empty session** with NO access to this chat history.

## Task for the next agent
{task}

## Rules (strict)
- Retain **only** details that are relevant to the task above.
- Omit unrelated work, dead ends, abandoned approaches, and broad history.
- Use only concrete facts from this conversation. Do not invent missing details.
- Prefer specifics: file paths, symbols, commands, errors, outputs, decisions, \
constraints that are non-negotiable for the task.
- Include line numbers only when known from this conversation.
- If a critical detail for the task is unknown, say so briefly and give the \
smallest verification step.
- Do not write a plan unless one already exists and is still needed for the task.
- Do **not** restate the task section in the note body.
- Do **not** call tools. Respond with plain markdown only.

## Output format (markdown only)
Use these headings when content exists; omit empty sections entirely:

### Goal and stop point
### Relevant files and symbols
### Completed work (task-relevant evidence only)
### Open work / unknowns
### Constraints and decisions
### Safest next action

Be tight and token-efficient. Prefer short bullets over prose dumps.\
</{tag}>"
    )
}

/// Append the handoff instruction after preparing the conversation snapshot
/// for a tool-free summarization call.
pub(crate) fn build_handoff_items(
    conversation: Vec<ConversationItem>,
    tag: &str,
    task: &str,
    strip_reasoning: bool,
) -> Vec<ConversationItem> {
    let mut items = if strip_reasoning {
        compaction_utils::strip_reasoning_blocks(conversation)
    } else {
        conversation
    };
    pop_trailing_tool_run(&mut items);
    items.push(ConversationItem::user(handoff_instruction(tag, task)));
    items
}

/// Budget-aware variant of [`build_handoff_items`].
///
/// Mirrors recap budgeting: keep the full prefix when it fits; otherwise
/// strip reasoning and front-trim to the estimated prompt budget.
pub(crate) fn budget_handoff_items(
    conversation: Vec<ConversationItem>,
    tag: &str,
    task: &str,
    strip_reasoning: bool,
    context_window: u64,
) -> Vec<ConversationItem> {
    let effective_window = context_window.min(HANDOFF_CONTEXT_WINDOW_CAP);
    let prompt_budget = (effective_window.saturating_mul(HANDOFF_BUDGET_THRESHOLD_PERCENT) / 100)
        .saturating_sub(HANDOFF_BUDGET_HEADROOM_TOKENS);

    let instruction = ConversationItem::user(handoff_instruction(tag, task));
    let snapshot_budget = prompt_budget.saturating_sub(estimate_item_tokens(&instruction));

    let pre_tokens = estimate_conversation_tokens(&conversation);
    if pre_tokens <= snapshot_budget {
        return build_handoff_items(conversation, tag, task, strip_reasoning);
    }

    let mut snapshot =
        compaction_utils::prepare_conversation_for_verbatim_summarization(conversation, true);
    pop_trailing_tool_run(&mut snapshot);
    let mut items = compaction_utils::fit_conversation_to_budget(snapshot, snapshot_budget);
    tracing::debug!(
        context_window,
        effective_window,
        prompt_budget,
        snapshot_budget,
        pre_tokens,
        post_tokens = estimate_conversation_tokens(&items),
        "handoff over budget: trimmed conversation to fit"
    );
    items.push(instruction);
    items
}

fn pop_trailing_tool_run(items: &mut Vec<ConversationItem>) {
    while let Some(last) = items.last() {
        match last {
            ConversationItem::Assistant(a) if !a.tool_calls.is_empty() => {
                items.pop();
            }
            ConversationItem::ToolResult(_) => {
                items.pop();
            }
            _ => break,
        }
    }
}

/// Clean model output into a handoff note suitable for seeding a new session.
pub(crate) fn clean_handoff_text(raw: &str) -> String {
    let mut out = raw.trim().to_string();

    // Prefer content inside <summary>...</summary> if the model used that shape.
    if let Some(start) = out.find("<summary>")
        && let Some(end) = out.find("</summary>")
        && end > start
    {
        out = out[start + "<summary>".len()..end].trim().to_string();
    }

    // Strip accidental labels.
    for label in [
        "Handoff note:",
        "Handoff:",
        "Hand-off:",
        "Summary:",
        "Context:",
    ] {
        if let Some(rest) = out.strip_prefix(label) {
            out = rest.trim_start().to_string();
            break;
        }
    }

    if out.chars().count() > HANDOFF_MAX_CHARS {
        let cut = floor_char_boundary(&out, HANDOFF_MAX_CHARS);
        out.truncate(cut);
        out = out.trim_end().to_string();
        out.push('\u{2026}');
    }

    out
}

/// Compose the first user prompt for the new session.
pub(crate) fn seed_prompt(note: &str, task: &str) -> String {
    let note = note.trim();
    let task = task.trim();
    format!(
        "# Handoff context\n\n\
{note}\n\n\
---\n\n\
## Task\n\
{task}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_embeds_task_and_relevance_rules() {
        let text = handoff_instruction("system-reminder", "implement /handoff");
        assert!(text.contains("implement /handoff"));
        assert!(text.contains("only") || text.contains("ONLY"));
        assert!(text.to_lowercase().contains("relevant"));
        assert!(text.contains("Do **not** call tools") || text.contains("Do not call tools"));
    }

    #[test]
    fn clean_strips_summary_tags_and_labels() {
        let cleaned = clean_handoff_text(
            "Handoff:\n<summary>\n### Goal and stop point\nShip handoff\n</summary>\n",
        );
        assert!(cleaned.contains("Ship handoff"));
        assert!(!cleaned.contains("<summary>"));
        assert!(!cleaned.starts_with("Handoff:"));
    }

    #[test]
    fn seed_prompt_separates_note_and_task() {
        let seed = seed_prompt("paths and decisions", "finish the feature");
        assert!(seed.contains("# Handoff context"));
        assert!(seed.contains("paths and decisions"));
        assert!(seed.contains("## Task"));
        assert!(seed.contains("finish the feature"));
    }

    #[test]
    fn build_handoff_items_appends_instruction() {
        let items = build_handoff_items(
            vec![
                ConversationItem::user("hello"),
                ConversationItem::assistant("hi"),
            ],
            "system-reminder",
            "do the thing",
            false,
        );
        assert_eq!(items.len(), 3);
        let last = items.last().unwrap().text_content();
        assert!(last.contains("do the thing"));
    }
}
