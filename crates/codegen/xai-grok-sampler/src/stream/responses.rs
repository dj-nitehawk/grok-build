//! Layer-2 stream transform for the OpenAI Responses API.
//!
//! Consumes a raw `rs::ResponseStreamEvent` stream and produces [`SamplingEvent`]s.
//! Pure: no I/O, no shell coupling.

use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::{BoxStream, Stream};

use xai_grok_sampling_types::{
    ConversationItem, ConversationResponse, ResponseModelMetadata, SamplingError, StopReason,
    TokenUsage, messages as messages_types, rs,
};

use crate::doom_loop_recovery::FailedResponseCapture;
use crate::events::{SamplingChannel, SamplingErrorInfo, SamplingEvent};
use crate::metrics::InferenceLatencyStats;
use crate::types::RequestId;

/// Wire values of `incomplete_details.reason` on an `Incomplete` response.
/// The xAI server emits the three `max_*` values; `content_filter` is OpenAI vocabulary, kept for spec compatibility.
const INCOMPLETE_REASON_CONTENT_FILTER: &str = "content_filter";
const INCOMPLETE_REASON_MAX_OUTPUT_TOKENS: &str = "max_output_tokens";
/// The model's context window was exhausted mid-generation (xAI extension).
const INCOMPLETE_REASON_MAX_PROMPT_TOKENS: &str = "max_prompt_tokens";
/// A server-side time limit cut generation short (xAI extension).
const INCOMPLETE_REASON_MAX_TIME_LIMIT: &str = "max_time_limit";

/// Reconcile before flattening, while output and content identities are available.
#[derive(Default)]
struct StreamedOutput {
    items: BTreeMap<u32, rs::OutputItem>,
    text: BTreeMap<(u32, u32), (String, String)>,
    arguments: BTreeMap<u32, (String, String, Option<String>)>,
}

impl StreamedOutput {
    fn observe(&mut self, event: &rs::ResponseStreamEvent) {
        use rs::ResponseStreamEvent::*;
        match event {
            ResponseOutputItemAdded(ev) => {
                if let rs::OutputItem::FunctionCall(call) = &ev.item {
                    self.arguments.entry(ev.output_index).or_insert_with(|| {
                        (
                            call.id.clone().unwrap_or_default(),
                            call.arguments.clone(),
                            Some(call.name.clone()),
                        )
                    });
                }
                self.items
                    .entry(ev.output_index)
                    .or_insert_with(|| ev.item.clone());
            }
            ResponseOutputItemDone(ev) => {
                let mut item = ev.item.clone();
                if let Some(previous) = self.items.get(&ev.output_index) {
                    Self::fill_item(&mut item, previous);
                }
                self.items.insert(ev.output_index, item);
            }
            ResponseOutputTextDelta(ev) => {
                let entry = self
                    .text
                    .entry((ev.output_index, ev.content_index))
                    .or_insert_with(|| (ev.item_id.clone(), String::new()));
                entry.1.push_str(&ev.delta);
            }
            ResponseOutputTextDone(ev) => {
                if !ev.text.is_empty() {
                    self.text.insert(
                        (ev.output_index, ev.content_index),
                        (ev.item_id.clone(), ev.text.clone()),
                    );
                }
            }
            ResponseFunctionCallArgumentsDelta(ev) => {
                let entry = self
                    .arguments
                    .entry(ev.output_index)
                    .or_insert_with(|| (ev.item_id.clone(), String::new(), None));
                if entry.0.is_empty() {
                    entry.0.clone_from(&ev.item_id);
                }
                entry.1.push_str(&ev.delta);
            }
            ResponseFunctionCallArgumentsDone(ev) => {
                let entry = self
                    .arguments
                    .entry(ev.output_index)
                    .or_insert_with(|| (ev.item_id.clone(), String::new(), None));
                if !ev.arguments.is_empty() {
                    entry.1 = ev.arguments.clone();
                }
                entry.2 = ev.name.clone();
            }
            _ => {}
        }
    }

    fn same_item(a: &rs::OutputItem, b: &rs::OutputItem) -> bool {
        match (a, b) {
            (rs::OutputItem::Message(a), rs::OutputItem::Message(b)) => {
                !a.id.is_empty() && a.id == b.id
            }
            (rs::OutputItem::FunctionCall(a), rs::OutputItem::FunctionCall(b)) => {
                (!a.call_id.is_empty() && a.call_id == b.call_id)
                    || a.id
                        .as_ref()
                        .filter(|id| !id.is_empty())
                        .is_some_and(|id| b.id.as_ref() == Some(id))
            }
            _ => false,
        }
    }

    fn fill_item(item: &mut rs::OutputItem, fallback: &rs::OutputItem) {
        match (item, fallback) {
            (rs::OutputItem::Message(item), rs::OutputItem::Message(fallback)) => {
                for (index, part) in fallback.content.iter().enumerate() {
                    match (item.content.get_mut(index), part) {
                        (
                            Some(rs::OutputMessageContent::OutputText(text)),
                            rs::OutputMessageContent::OutputText(other),
                        ) if text.text.is_empty() || other.text.starts_with(&text.text) => {
                            text.text.clone_from(&other.text)
                        }
                        (None, _) => item.content.push(part.clone()),
                        _ => {}
                    }
                }
            }
            (rs::OutputItem::FunctionCall(item), rs::OutputItem::FunctionCall(fallback)) => {
                if item.call_id.is_empty() {
                    item.call_id.clone_from(&fallback.call_id);
                }
                if item.name.is_empty() {
                    item.name.clone_from(&fallback.name);
                }
                if item.arguments.is_empty() || fallback.arguments.starts_with(&item.arguments) {
                    item.arguments.clone_from(&fallback.arguments);
                }
                if item.id.is_none() {
                    item.id.clone_from(&fallback.id);
                }
            }
            _ => {}
        }
    }

    fn reconcile(mut self, response: &mut rs::Response) {
        for ((output_index, content_index), (id, text)) in self.text {
            let item = self.items.entry(output_index).or_insert_with(|| {
                rs::OutputItem::Message(rs::OutputMessage {
                    id,
                    content: Vec::new(),
                    role: rs::AssistantRole::Assistant,
                    status: rs::OutputStatus::Completed,
                })
            });
            if let rs::OutputItem::Message(message) = item {
                while message.content.len() <= content_index as usize {
                    message.content.push(rs::OutputMessageContent::OutputText(
                        rs::OutputTextContent {
                            text: String::new(),
                            annotations: Vec::new(),
                            logprobs: None,
                        },
                    ));
                }
                if let rs::OutputMessageContent::OutputText(existing) =
                    &mut message.content[content_index as usize]
                    && (existing.text.is_empty() || text.starts_with(&existing.text))
                {
                    existing.text = text;
                }
            }
        }
        for (index, (id, arguments, name)) in self.arguments {
            // An arguments-only frame has no call_id; never invent an executable call.
            let snapshot_call = response.output.iter_mut().find(|item| {
                matches!(item, rs::OutputItem::FunctionCall(call)
                    if !id.is_empty() && call.id.as_deref() == Some(id.as_str()))
            });
            let item = self.items.get_mut(&index).or(snapshot_call);
            if let Some(rs::OutputItem::FunctionCall(call)) = item {
                if call.arguments.is_empty() || arguments.starts_with(&call.arguments) {
                    call.arguments = arguments;
                }
                if call.name.is_empty() {
                    call.name = name.unwrap_or_default();
                }
                if call.id.is_none() && !id.is_empty() {
                    call.id = Some(id);
                }
            }
        }
        let mut output: Vec<_> = std::mem::take(&mut response.output)
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let index = self
                    .items
                    .iter()
                    .find(|(_, streamed)| Self::same_item(&item, streamed))
                    .map(|(index, _)| *index as usize)
                    .unwrap_or(index);
                (index, true, item)
            })
            .collect();
        for (index, fallback) in self.items {
            if !matches!(
                fallback,
                rs::OutputItem::Message(_) | rs::OutputItem::FunctionCall(_)
            ) {
                continue;
            }
            if let Some((_, _, item)) = output
                .iter_mut()
                .find(|(_, _, item)| Self::same_item(item, &fallback))
            {
                Self::fill_item(item, &fallback);
            } else {
                if let rs::OutputItem::FunctionCall(call) = &fallback
                    && (call.call_id.is_empty() || call.name.is_empty())
                {
                    continue;
                }
                output.push((index as usize, false, fallback));
            }
        }
        output.sort_by_key(|(index, snapshot, _)| (*index, *snapshot));
        response.output = output.into_iter().map(|(_, _, item)| item).collect();
    }
}

/// Returns whether a Responses API event reflects real model progress rather than a liveness-only heartbeat or status transition.
pub(crate) fn responses_event_has_meaningful_content(event: &rs::ResponseStreamEvent) -> bool {
    use rs::ResponseStreamEvent;

    match event {
        ResponseStreamEvent::ResponseCreated(_)
        | ResponseStreamEvent::ResponseInProgress(_)
        | ResponseStreamEvent::ResponseQueued(_) => false,
        ResponseStreamEvent::ResponseOutputTextDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseOutputTextDone(event) => !event.text.is_empty(),
        ResponseStreamEvent::ResponseRefusalDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseRefusalDone(event) => !event.refusal.is_empty(),
        ResponseStreamEvent::ResponseFunctionCallArgumentsDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseFunctionCallArgumentsDone(event) => {
            !event.arguments.is_empty() || event.name.as_ref().is_some_and(|name| !name.is_empty())
        }
        ResponseStreamEvent::ResponseReasoningSummaryTextDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseReasoningSummaryTextDone(event) => !event.text.is_empty(),
        ResponseStreamEvent::ResponseReasoningTextDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseReasoningTextDone(event) => !event.text.is_empty(),
        ResponseStreamEvent::ResponseMCPCallArgumentsDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseMCPCallArgumentsDone(event) => !event.arguments.is_empty(),
        ResponseStreamEvent::ResponseCodeInterpreterCallCodeDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseCodeInterpreterCallCodeDone(event) => !event.code.is_empty(),
        ResponseStreamEvent::ResponseCustomToolCallInputDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseCustomToolCallInputDone(event) => !event.input.is_empty(),
        ResponseStreamEvent::ResponseFailed(event) => {
            !event.response.output.is_empty()
                || event
                    .response
                    .usage
                    .as_ref()
                    .is_some_and(|usage| usage.output_tokens > 0)
        }
        ResponseStreamEvent::ResponseCompleted(_)
        | ResponseStreamEvent::ResponseIncomplete(_)
        | ResponseStreamEvent::ResponseOutputItemAdded(_)
        | ResponseStreamEvent::ResponseOutputItemDone(_)
        | ResponseStreamEvent::ResponseContentPartAdded(_)
        | ResponseStreamEvent::ResponseContentPartDone(_)
        | ResponseStreamEvent::ResponseFileSearchCallInProgress(_)
        | ResponseStreamEvent::ResponseFileSearchCallSearching(_)
        | ResponseStreamEvent::ResponseFileSearchCallCompleted(_)
        | ResponseStreamEvent::ResponseWebSearchCallInProgress(_)
        | ResponseStreamEvent::ResponseWebSearchCallSearching(_)
        | ResponseStreamEvent::ResponseWebSearchCallCompleted(_)
        | ResponseStreamEvent::ResponseReasoningSummaryPartAdded(_)
        | ResponseStreamEvent::ResponseReasoningSummaryPartDone(_)
        | ResponseStreamEvent::ResponseImageGenerationCallCompleted(_)
        | ResponseStreamEvent::ResponseImageGenerationCallGenerating(_)
        | ResponseStreamEvent::ResponseImageGenerationCallInProgress(_)
        | ResponseStreamEvent::ResponseImageGenerationCallPartialImage(_)
        | ResponseStreamEvent::ResponseMCPCallCompleted(_)
        | ResponseStreamEvent::ResponseMCPCallFailed(_)
        | ResponseStreamEvent::ResponseMCPCallInProgress(_)
        | ResponseStreamEvent::ResponseMCPListToolsCompleted(_)
        | ResponseStreamEvent::ResponseMCPListToolsFailed(_)
        | ResponseStreamEvent::ResponseMCPListToolsInProgress(_)
        | ResponseStreamEvent::ResponseCodeInterpreterCallInProgress(_)
        | ResponseStreamEvent::ResponseCodeInterpreterCallInterpreting(_)
        | ResponseStreamEvent::ResponseCodeInterpreterCallCompleted(_)
        | ResponseStreamEvent::ResponseOutputTextAnnotationAdded(_)
        | ResponseStreamEvent::ResponseError(_) => true,
    }
}

pub(crate) fn responses_event_may_have_output(event: &rs::ResponseStreamEvent) -> bool {
    !matches!(event, rs::ResponseStreamEvent::ResponseError(_))
        && responses_event_has_meaningful_content(event)
}

/// Copy everything the Doom-loop capture needs out of a frame.
/// Any frame that names tool activity or compaction state vetoes the replay, since reasoning must never be retried without the item it is bound to.
fn observe_for_recovery(capture: &FailedResponseCapture, event: &rs::ResponseStreamEvent) {
    use rs::ResponseStreamEvent as Event;
    if !capture.is_armed() {
        return;
    }
    match event {
        Event::ResponseOutputTextDelta(text) => capture.record_output_delta(
            text.output_index,
            text.content_index,
            text.item_id.clone(),
            &text.delta,
        ),
        Event::ResponseOutputTextDone(text) => capture.record_output_done(
            text.output_index,
            text.content_index,
            text.item_id.clone(),
            text.text.clone(),
        ),
        Event::ResponseReasoningTextDelta(reasoning) => capture.record_reasoning_delta(
            reasoning.output_index,
            reasoning.content_index,
            reasoning.item_id.clone(),
            &reasoning.delta,
        ),
        Event::ResponseReasoningTextDone(reasoning) => capture.record_reasoning_done(
            reasoning.output_index,
            reasoning.content_index,
            reasoning.item_id.clone(),
            reasoning.text.clone(),
        ),
        Event::ResponseReasoningSummaryTextDelta(summary) => capture
            .record_reasoning_summary_delta(
                summary.output_index,
                summary.summary_index,
                summary.item_id.clone(),
                &summary.delta,
            ),
        Event::ResponseReasoningSummaryTextDone(summary) => capture.record_reasoning_summary_done(
            summary.output_index,
            summary.summary_index,
            summary.item_id.clone(),
            summary.text.clone(),
        ),
        Event::ResponseOutputItemAdded(added) => capture.record_item_start(&added.item),
        Event::ResponseOutputItemDone(done) => {
            capture.record_output_item(done.output_index, &done.item);
        }
        Event::ResponseCompleted(completed) => {
            capture.record_terminal_output(&completed.response.output);
        }
        Event::ResponseIncomplete(incomplete) => {
            capture.record_terminal_output(&incomplete.response.output);
        }
        // Frames that only name in-flight tool work
        // The item they belong to may never complete on this attempt, so the frame itself is the notice that a call was in flight
        Event::ResponseFunctionCallArgumentsDelta(_)
        | Event::ResponseFunctionCallArgumentsDone(_)
        | Event::ResponseCustomToolCallInputDelta(_)
        | Event::ResponseCustomToolCallInputDone(_)
        | Event::ResponseCodeInterpreterCallCodeDelta(_)
        | Event::ResponseCodeInterpreterCallCodeDone(_)
        | Event::ResponseCodeInterpreterCallInProgress(_)
        | Event::ResponseCodeInterpreterCallInterpreting(_)
        | Event::ResponseCodeInterpreterCallCompleted(_)
        | Event::ResponseFileSearchCallInProgress(_)
        | Event::ResponseFileSearchCallSearching(_)
        | Event::ResponseFileSearchCallCompleted(_)
        | Event::ResponseWebSearchCallInProgress(_)
        | Event::ResponseWebSearchCallSearching(_)
        | Event::ResponseWebSearchCallCompleted(_)
        | Event::ResponseImageGenerationCallInProgress(_)
        | Event::ResponseImageGenerationCallGenerating(_)
        | Event::ResponseImageGenerationCallCompleted(_)
        | Event::ResponseMCPCallInProgress(_)
        | Event::ResponseMCPCallCompleted(_)
        | Event::ResponseMCPCallFailed(_)
        | Event::ResponseMCPCallArgumentsDelta(_)
        | Event::ResponseMCPCallArgumentsDone(_) => capture.record_unreplayable(),
        _ => {}
    }
}

/// Transform a raw Responses API event stream into a stream of [`SamplingEvent`]s.
/// `None` (check disabled) leaves the response untouched.
pub fn stream_responses<'a>(
    raw_stream: BoxStream<'a, Result<rs::ResponseStreamEvent, SamplingError>>,
    model_metadata: Option<ResponseModelMetadata>,
    request_id: RequestId,
    idle_timeout: Duration,
    doom_loop: Option<crate::doom_loop::DoomLoopSignalCollector>,
) -> impl Stream<Item = SamplingEvent> + Send + 'a {
    stream_responses_tracked(
        raw_stream,
        model_metadata,
        request_id,
        idle_timeout,
        doom_loop,
        Arc::new(AtomicBool::new(false)),
        FailedResponseCapture::default(),
    )
}

pub(crate) fn stream_responses_tracked<'a>(
    raw_stream: BoxStream<'a, Result<rs::ResponseStreamEvent, SamplingError>>,
    model_metadata: Option<ResponseModelMetadata>,
    request_id: RequestId,
    idle_timeout: Duration,
    doom_loop: Option<crate::doom_loop::DoomLoopSignalCollector>,
    output_observed: Arc<AtomicBool>,
    failed_response: FailedResponseCapture,
) -> impl Stream<Item = SamplingEvent> + Send + 'a {
    async_stream::stream! {
        use rs::{ResponseStreamEvent, Status};

        let decode_region = crate::span_timing::Region::from_span(tracing::info_span!(
            "sampling.stream_decode",
            ttft_ms = tracing::field::Empty,
            ttlb_ms = tracing::field::Empty,
            output_tokens = tracing::field::Empty,
            chunk_count = tracing::field::Empty,
        ));
        let stream_start = Instant::now();
        let mut chunk_timestamps: Vec<Instant> = Vec::new();

        yield SamplingEvent::StreamStarted {
            request_id: request_id.clone(),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        };

        if let Some(metadata) = model_metadata {
            yield SamplingEvent::ModelMetadata {
                request_id: request_id.clone(),
                metadata,
            };
        }

        let mut final_response: Option<rs::Response> = None;
        let mut chunk_index: u64 = 0;
        let mut message_chunk_count: u64 = 0;
        let mut first_token_emitted = false;
        let mut reasoning_acc = String::new();
        let mut streamed_output = StreamedOutput::default();
        let mut last_content_chunk_at = Instant::now();

        // Maps Responses API `output_index` to our tool-only `tool_index`.
        // Populated when `ResponseOutputItemAdded` carries a `FunctionCall`
        // Later `ResponseFunctionCallArgumentsDelta` events look up `output_index` here to find the matching `tool_index`
        let mut output_to_tool_index: BTreeMap<u32, u32> = BTreeMap::new();
        let mut next_tool_index: u32 = 0;


        let mut stream = raw_stream;
        loop {
            let event_result = match tokio::time::timeout(idle_timeout, stream.next()).await {
                Ok(Some(event_result)) => event_result,
                Ok(None) => break,
                Err(_elapsed) => {
                    let err = SamplingError::IdleTimeout {
                        elapsed_secs: idle_timeout.as_secs(),
                    };
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }
            };

            let event = match event_result {
                Ok(event) => event,
                Err(err) => {
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }
            };

            if responses_event_may_have_output(&event) {
                output_observed.store(true, Ordering::Relaxed);
            }

            // A confident midstream signal aborts the attempt immediately.
            // Terminal frames are processed so their complete response items remain available to the retry loop
            // `drive_l2` rejects the completed response before it can be accepted
            let is_terminal_response = matches!(
                &event,
                ResponseStreamEvent::ResponseCompleted(_)
                    | ResponseStreamEvent::ResponseIncomplete(_)
            );
            // Observation happens before the abort gate so the aborting frame lands in the capture like any other
            // The attempt is discarded either way, so nothing here reaches downstream consumers
            observe_for_recovery(&failed_response, &event);

            if !is_terminal_response
                && let Some(triggers) = doom_loop.as_ref().and_then(|c| c.abort_triggers())
            {
                let all_triggers = doom_loop
                    .as_ref()
                    .map(|collector| {
                        collector
                            .take()
                            .into_iter()
                            .map(|signal| signal.raw)
                            .collect()
                    })
                    .unwrap_or_default();
                yield SamplingEvent::DoomLoopSignals {
                    request_id: request_id.clone(),
                    triggers: all_triggers,
                };
                let err = SamplingError::DoomLoopDetected {
                    triggers,
                    aborted_at_chunk: Some(chunk_index),
                };
                yield SamplingEvent::Failed {
                    request_id: request_id.clone(),
                    error: SamplingErrorInfo::from(&err),
                };
                return;
            }

            streamed_output.observe(&event);
            let event_has_content = responses_event_has_meaningful_content(&event);

            // Track whether ResponseIncomplete should break the loop after the content-aware idle check below
            let mut should_break = false;

            match event {
                ResponseStreamEvent::ResponseOutputTextDelta(text_delta_event) => {
                    let delta = text_delta_event.delta;
                    if !delta.is_empty() {
                        if !first_token_emitted {
                            first_token_emitted = true;
                            yield SamplingEvent::FirstToken {
                                request_id: request_id.clone(),
                            };
                        }
                        chunk_timestamps.push(Instant::now());
                        chunk_index += 1;
                        message_chunk_count += 1;
                        yield SamplingEvent::ChannelToken {
                            request_id: request_id.clone(),
                            channel: SamplingChannel::Text,
                            text: delta,
                            chunk_index,
                        };
                    }
                }

                ResponseStreamEvent::ResponseReasoningSummaryTextDelta(summary_event) => {
                    let delta = summary_event.delta;
                    if !delta.is_empty() {
                        if !first_token_emitted {
                            first_token_emitted = true;
                            yield SamplingEvent::FirstToken {
                                request_id: request_id.clone(),
                            };
                        }
                        chunk_index += 1;
                        yield SamplingEvent::ChannelToken {
                            request_id: request_id.clone(),
                            channel: SamplingChannel::Reasoning,
                            text: delta,
                            chunk_index,
                        };
                    }
                }

                ResponseStreamEvent::ResponseReasoningTextDelta(reasoning_event) => {
                    let delta = reasoning_event.delta;
                    if !delta.is_empty() {
                        if !first_token_emitted {
                            first_token_emitted = true;
                            yield SamplingEvent::FirstToken {
                                request_id: request_id.clone(),
                            };
                        }
                        chunk_index += 1;
                        reasoning_acc.push_str(&delta);
                        yield SamplingEvent::ChannelToken {
                            request_id: request_id.clone(),
                            channel: SamplingChannel::Reasoning,
                            text: delta,
                            chunk_index,
                        };
                    }
                }

                // Start of a Responses FunctionCall: emit the initial id and name, and remember the output_index to tool_index mapping
                ResponseStreamEvent::ResponseOutputItemAdded(added_event) => {
                    if let rs::OutputItem::FunctionCall(fc) = added_event.item
                        && !output_to_tool_index.contains_key(&added_event.output_index)
                    {
                        let tool_index = *output_to_tool_index.entry(added_event.output_index).or_insert_with(|| {
                            let index = next_tool_index;
                            next_tool_index += 1;
                            index
                        });

                        yield SamplingEvent::ToolCallDelta {
                            request_id: request_id.clone(),
                            tool_index,
                            id: Some(fc.call_id),
                            name: Some(fc.name),
                            arguments_delta: None,
                        };
                    }
                }

                // Continuation chunk for a streaming FunctionCall's args.
                // The delta is dropped silently when no preceding OutputItemAdded mapped its output_index
                ResponseStreamEvent::ResponseFunctionCallArgumentsDelta(args_event) => {
                    let delta = args_event.delta;
                    if !delta.is_empty() {
                        if let Some(&tool_index) =
                            output_to_tool_index.get(&args_event.output_index)
                        {
                            yield SamplingEvent::ToolCallDelta {
                                request_id: request_id.clone(),
                                tool_index,
                                id: None,
                                name: None,
                                arguments_delta: Some(delta),
                            };
                        }
                    }
                }

                ResponseStreamEvent::ResponseCompleted(completed_event) => {
                    final_response = Some(completed_event.response);
                }

                ResponseStreamEvent::ResponseIncomplete(incomplete_event) => {
                    final_response = Some(incomplete_event.response);
                    should_break = true;
                }

                ResponseStreamEvent::ResponseFailed(failed_event) => {
                    let response = failed_event.response;
                    let error_message = response
                        .error
                        .as_ref()
                        .map(|e| format!("{}: {}", e.code, e.message))
                        .unwrap_or_else(|| "Response failed with unknown error".to_string());
                    let err = SamplingError::Api {
                        status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                        message: error_message,
                        model_metadata: None,
                        retry_after_secs: None,
                        should_retry: None,
                        error_code: response
                            .error
                            .as_ref()
                            .map(|e| xai_grok_sampling_types::ApiErrorCode::parse(&e.code)),
                    };
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }

                ResponseStreamEvent::ResponseError(error_event) => {
                    let error_message = format!(
                        "{}: {}",
                        error_event.code.as_deref().unwrap_or("error"),
                        error_event.message
                    );
                    let err = SamplingError::Api {
                        status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                        message: error_message,
                        model_metadata: None,
                        retry_after_secs: None,
                        should_retry: None,
                        // The wire code, absent when the event carried none.
                        error_code: error_event
                            .code
                            .as_deref()
                            .map(xai_grok_sampling_types::ApiErrorCode::parse),
                    };
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }

                // ── Backend-hosted tool lifecycle events ────────────
                // These tools are executed server-side by the agentic sampler
                // We emit progress events so the shell/pager can show status to the user

                // Web search
                ResponseStreamEvent::ResponseWebSearchCallInProgress(ev) => {
                    yield SamplingEvent::BackendToolCallStarted {
                        request_id: request_id.clone(),
                        call_id: ev.item_id.clone(),
                        name: "web_search".to_string(),
                    };
                }
                // Completed/Searching carry no data; the real payload arrives via ResponseOutputItemDone(WebSearchCall) below
                ResponseStreamEvent::ResponseWebSearchCallCompleted(_)
                | ResponseStreamEvent::ResponseWebSearchCallSearching(_) => {}

                // Code interpreter runs server-side, like web/x search
                // The shell renders that as a client `tool_use` and `user` `tool_result` split grok has no HostedTool::CodeInterpreter, so these events never arrive under the current hosted-tool set
                // The started event fires on InProgress; the full payload (code and outputs) rides ResponseOutputItemDone(CodeInterpreterCall) below
                ResponseStreamEvent::ResponseCodeInterpreterCallInProgress(ev) => {
                    yield SamplingEvent::BackendToolCallStarted {
                        request_id: request_id.clone(),
                        call_id: ev.item_id.clone(),
                        name: "code_interpreter".to_string(),
                    };
                }
                // Interpreting/Completed carry no payload; the result arrives via ResponseOutputItemDone(CodeInterpreterCall) below
                ResponseStreamEvent::ResponseCodeInterpreterCallInterpreting(_)
                | ResponseStreamEvent::ResponseCodeInterpreterCallCompleted(_) => {}

                // OutputItemDone carries the full result for backend tools.
                // For WebSearchCall this includes the query and source URLs.
                // For CustomToolCall this includes x_search results.
                ResponseStreamEvent::ResponseOutputItemDone(done_event) => {
                    match &done_event.item {
                        rs::OutputItem::WebSearchCall(ws) => {
                            let result = serde_json::to_value(ws).ok();
                            yield SamplingEvent::BackendToolCallCompleted {
                                request_id: request_id.clone(),
                                call_id: ws.id.clone(),
                                name: "web_search".to_string(),
                                result,
                            };
                        }
                        // X search results arrive as CustomToolCall with names like x_keyword_search, x_semantic_search, etc
                        // Use "x_search" consistently (matching the Started event)
                        // The specific sub-type is in the serialized result payload and extracted by the pager from raw_output.name
                        rs::OutputItem::CustomToolCall(ct) => {
                            let result = serde_json::to_value(ct).ok();
                            yield SamplingEvent::BackendToolCallCompleted {
                                request_id: request_id.clone(),
                                call_id: ct.id.clone(),
                                name: "x_search".to_string(),
                                result,
                            };
                        }
                        // Code interpreter: the full call (code and outputs) rides the done item
                        // The completed event uses the shared "code_interpreter" name (matching the Started event)
                        rs::OutputItem::CodeInterpreterCall(ci) => {
                            let result = serde_json::to_value(ci).ok();
                            yield SamplingEvent::BackendToolCallCompleted {
                                request_id: request_id.clone(),
                                call_id: ci.id.clone(),
                                name: "code_interpreter".to_string(),
                                result,
                            };
                        }
                        _ => {}
                    }
                }

                // CustomToolCallInputDelta is x_search in-progress streaming.
                // Emit a started event on first delta per item_id.
                ResponseStreamEvent::ResponseCustomToolCallInputDone(ev) => {
                    yield SamplingEvent::BackendToolCallStarted {
                        request_id: request_id.clone(),
                        call_id: ev.item_id.clone(),
                        name: "x_search".to_string(),
                    };
                }

                // All other events (intermediate progress, annotations, image gen, file search, etc.) need no action
                _ => {}
            }

            if event_has_content {
                last_content_chunk_at = Instant::now();
            } else if last_content_chunk_at.elapsed() > idle_timeout {
                let err = SamplingError::IdleTimeout {
                    elapsed_secs: idle_timeout.as_secs(),
                };
                yield SamplingEvent::Failed {
                    request_id: request_id.clone(),
                    error: SamplingErrorInfo::from(&err),
                };
                return;
            }

            if should_break {
                break;
            }
        }

        // ── Build the final response ─────────────────────────────────
        let mut response = match final_response {
            Some(r) => r,
            None => {
                let err = SamplingError::Api {
                    status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                    message: "No ResponseCompleted or ResponseIncomplete event received from \
                              Responses API"
                        .to_string(),
                    model_metadata: None,
                    retry_after_secs: None,
                    should_retry: None,
                    // Synthesized client-side; no wire envelope to read.
                    error_code: None,
                };
                yield SamplingEvent::Failed {
                    request_id: request_id.clone(),
                    error: SamplingErrorInfo::from(&err),
                };
                return;
            }
        };

        // Billing fields (`prompt_tokens`, `completion_tokens`, `cached_prompt_tokens`, `reasoning_tokens`) are the cumulative wire values
        // The SSE decoder (`deserialize_response_event`) has already rewritten `u.total_tokens` to `context_details.input + output`
        let usage = response.usage.as_ref().map(|u| TokenUsage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
            total_tokens: u.total_tokens,
            reasoning_tokens: u.output_tokens_details.reasoning_tokens,
            cached_prompt_tokens: u.input_tokens_details.cached_tokens,
            cache_creation_prompt_tokens: 0,
        });

        let cost_usd_ticks = response
            .metadata
            .as_mut()
            .and_then(|m| m.remove(crate::client::COST_USD_TICKS_METADATA_KEY))
            .and_then(|s| s.parse::<i64>().ok());

        let status = response.status.clone();
        // Wire reason for an incomplete response (the `INCOMPLETE_REASON_*` values above)
        // It is captured before `response` is consumed below
        let incomplete_reason = response
            .incomplete_details
            .as_ref()
            .map(|d| d.reason.clone());

        streamed_output.reconcile(&mut response);
        let mut items = xai_grok_sampling_types::response_to_conversation_items(response);
        xai_grok_sampling_types::inject_streaming_reasoning_fallback(&mut items, reasoning_acc);

        let has_tool_calls = items.iter().any(|i| match i {
            ConversationItem::Assistant(a) => !a.tool_calls.is_empty(),
            _ => false,
        });

        // The single classification of an Incomplete response: the collapsed [`StopReason`] plus the typed raw reason carried to consumers
        // The Responses wire strings never leave this module; the raw reason reuses the Messages wire strings so the shell speaks one vocabulary
        // Only the strings match: the xAI Messages backend itself reports a context cut as `max_tokens`, and only this mapping splits it
        let incomplete_classification: Option<(StopReason, Option<messages_types::StopReason>)> =
            if matches!(status, Status::Incomplete) {
                Some(match incomplete_reason.as_deref() {
                    // A moderation cut ("content_filter") maps to ContentFilter, not Length
                    // A filter-cut response must never be salvaged and continued by `LengthPolicy`
                    Some(INCOMPLETE_REASON_CONTENT_FILTER) => (StopReason::ContentFilter, None),
                    Some(INCOMPLETE_REASON_MAX_OUTPUT_TOKENS) => (
                        StopReason::Length,
                        Some(messages_types::StopReason::MaxTokens),
                    ),
                    Some(INCOMPLETE_REASON_MAX_PROMPT_TOKENS) => (
                        StopReason::Length,
                        Some(messages_types::StopReason::ModelContextWindowExceeded),
                    ),
                    // A time-limit cut is a Length cut with no Messages vocabulary word
                    // Log it because the truncation notice the user sees says "output limit"
                    Some(INCOMPLETE_REASON_MAX_TIME_LIMIT) => {
                        tracing::info!(
                            request_id = %request_id,
                            "response cut by the server-side time limit"
                        );
                        (StopReason::Length, None)
                    }
                    // An Incomplete response without a reason is a length cut with nothing to carry
                    None => (StopReason::Length, None),
                    Some(other) => {
                        tracing::warn!(
                            reason = %other,
                            "unknown incomplete reason; treating as Length"
                        );
                        (StopReason::Length, None)
                    }
                })
            } else {
                None
            };

        // NOTE: tool calls win even over an Incomplete status, the opposite precedence from the Messages backend
        // On the Messages backend Length wins, so the `LengthPolicy` gate can refuse a possibly argument-truncated trailing call
        // The difference is deliberate; don't "fix" it here
        let (stop_reason, raw_stop_reason) = if has_tool_calls {
            if matches!(incomplete_classification, Some((StopReason::Length, _))) {
                tracing::warn!(
                    request_id = %request_id,
                    "tool calls mask a length-truncated response; arguments may be truncated"
                );
            }
            // Keep the pair coherent: a tool-bearing turn reports ToolCalls with no raw length reason (the warn above is the truncation signal)
            // That preserves the headless output's `tool_use`
            (Some(StopReason::ToolCalls), None)
        } else {
            match status {
                Status::Completed => (Some(StopReason::Stop), None),
                Status::Incomplete => match incomplete_classification {
                    Some((stop, raw)) => (Some(stop), raw.map(|r| r.wire_str())),
                    None => (None, None),
                },
                _ => (None, None),
            }
        };

        let stream_end = Instant::now();
        let metrics =
            InferenceLatencyStats::from_timestamps(stream_start, &chunk_timestamps, stream_end);

        decode_region
            .span()
            .record("ttlb_ms", metrics.time_to_last_byte_ms as i64);
        decode_region
            .span()
            .record("chunk_count", metrics.chunk_count as i64);
        if let Some(ttft) = metrics.time_to_first_token_ms {
            decode_region.span().record("ttft_ms", ttft as i64);
        }
        if let Some(u) = usage.as_ref() {
            decode_region
                .span()
                .record("output_tokens", u.completion_tokens as i64);
        }
        drop(decode_region);

        // Warn-only for now: log the server-reported triggers once per request (raw labels only, ZDR-safe) and attach them for callers
        let doom_loop_signals = doom_loop
            .as_ref()
            .map(|collector| collector.take())
            .unwrap_or_default();
        if !doom_loop_signals.is_empty() {
            tracing::warn!(
                request_id = %request_id,
                triggers = ?doom_loop_signals.iter().map(|s| s.raw.as_str()).collect::<Vec<_>>(),
                "server reported doom-loop triggers for this response"
            );
        }

        let conversation_response = ConversationResponse {
            items,
            stop_reason,
            usage,
            cost_usd_ticks,
            message_chunks_emitted: message_chunk_count,
            doom_loop_signals,
            stop_message: None, // not reported on the Responses API
            message_id: None,   // no provider message id on the Responses API
            raw_stop_reason,
            stop_sequence: None,
        };

        yield SamplingEvent::Completed {
            request_id: request_id.clone(),
            response: Box::new(conversation_response),
            metrics,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_openai::types::responses as rs_types;
    use futures_util::stream;
    use std::pin::pin;

    fn rid() -> RequestId {
        RequestId::from("resp-test")
    }

    fn build_response(status: rs_types::Status) -> rs_types::Response {
        rs_types::Response {
            background: None,
            billing: None,
            conversation: None,
            created_at: 0,
            completed_at: None,
            error: None,
            id: "resp_1".into(),
            incomplete_details: None,
            instructions: None,
            max_output_tokens: None,
            metadata: None,
            model: "test-model".into(),
            object: "response".into(),
            output: vec![],
            parallel_tool_calls: None,
            previous_response_id: None,
            prompt: None,
            prompt_cache_key: None,
            prompt_cache_retention: None,
            reasoning: None,
            safety_identifier: None,
            service_tier: None,
            status,
            temperature: None,
            text: None,
            tool_choice: None,
            tools: None,
            top_logprobs: None,
            top_p: None,
            truncation: None,
            usage: None,
        }
    }

    fn empty_completed_response() -> rs_types::Response {
        build_response(rs_types::Status::Completed)
    }

    fn failed_response_with_error(message: &str) -> rs_types::Response {
        let mut r = build_response(rs_types::Status::Failed);
        r.error = Some(rs_types::ErrorObject {
            code: "server_error".into(),
            message: message.into(),
        });
        r
    }

    fn text_delta_event(delta: &str) -> rs::ResponseStreamEvent {
        rs::ResponseStreamEvent::ResponseOutputTextDelta(rs_types::ResponseTextDeltaEvent {
            sequence_number: 0,
            item_id: "item-1".into(),
            output_index: 0,
            content_index: 0,
            delta: delta.into(),
            logprobs: None,
        })
    }

    fn completed_event() -> rs::ResponseStreamEvent {
        rs::ResponseStreamEvent::ResponseCompleted(rs_types::ResponseCompletedEvent {
            response: empty_completed_response(),
            sequence_number: 0,
        })
    }

    async fn collect(s: impl Stream<Item = SamplingEvent>) -> Vec<SamplingEvent> {
        let mut out = Vec::new();
        let mut s = pin!(s);
        while let Some(ev) = s.next().await {
            out.push(ev);
        }
        out
    }

    /// A confident signal that aborts on a custom-tool input frame still vetoes the replay.
    /// The frame is the only notice that a call was in flight, and reasoning must never be retried without it.
    /// The same holds for the code-interpreter code frames.
    #[tokio::test]
    async fn an_abort_on_a_tool_input_frame_vetoes_the_replay() {
        for tool_frame in [
            rs::ResponseStreamEvent::ResponseCustomToolCallInputDelta(
                rs_types::ResponseCustomToolCallInputDeltaEvent {
                    sequence_number: 1,
                    output_index: 1,
                    item_id: "custom-1".into(),
                    delta: "{\"q\":".into(),
                },
            ),
            rs::ResponseStreamEvent::ResponseCodeInterpreterCallCodeDelta(
                rs_types::ResponseCodeInterpreterCallCodeDeltaEvent {
                    sequence_number: 1,
                    output_index: 1,
                    item_id: "ci-1".into(),
                    delta: "print(".into(),
                },
            ),
        ] {
            let capture = FailedResponseCapture::armed();
            // A collector that has already seen a confident trigger: the next non-terminal frame aborts the attempt
            let collector = crate::doom_loop::DoomLoopSignalCollector::new(
                xai_grok_sampling_types::DoomLoopRecoveryPolicy::default(),
            );
            collector.absorb(
                xai_grok_sampling_types::doom_loop::DOOM_LOOP_CHECK_EVENT_TYPE,
                r#"{"type":"response.doom_loop_check","doom_loop_check":{"triggers":["tail_repetition:8@thinking"]}}"#,
            );

            // Reasoning is already captured, so an intact replay would carry it: only the veto can empty the capture
            // The collector is armed before the stream runs, so the abort lands on the tool frame
            capture.record_reasoning_delta(0, 0, "reasoning-1".into(), "looping thought");
            let raw = stream::iter(vec![Ok(tool_frame), Ok(completed_event())]).boxed();
            let events = collect(stream_responses_tracked(
                raw,
                None,
                rid(),
                Duration::from_secs(60),
                Some(collector),
                Arc::new(AtomicBool::new(false)),
                capture.clone(),
            ))
            .await;

            assert!(
                matches!(events.last(), Some(SamplingEvent::Failed { .. })),
                "the confident signal aborts the attempt"
            );
            assert!(
                capture.take_items().is_empty(),
                "a turn with a call in flight replays nothing"
            );
        }
    }

    #[tokio::test]
    async fn missing_completed_event_yields_failed() {
        let raw =
            stream::iter(Vec::<Result<rs::ResponseStreamEvent, SamplingError>>::new()).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(error.kind, crate::events::SamplingErrorKind::Api);
                assert_eq!(error.status_code, Some(500));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    fn incomplete_event(reason: &str) -> rs::ResponseStreamEvent {
        let mut response = build_response(rs_types::Status::Incomplete);
        response.incomplete_details = Some(rs_types::IncompleteDetails {
            reason: reason.into(),
        });
        rs::ResponseStreamEvent::ResponseIncomplete(rs_types::ResponseIncompleteEvent {
            response,
            sequence_number: 0,
        })
    }

    /// Returns the (collapsed stop reason, raw wire stop reason) for an Incomplete response ending with the given `incomplete_details.reason`.
    async fn stop_reasons_for_incomplete(reason: &str) -> (Option<StopReason>, Option<String>) {
        let raw = stream::iter(vec![
            Ok(text_delta_event("cut")),
            Ok(incomplete_event(reason)),
        ])
        .boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                (response.stop_reason, response.raw_stop_reason.clone())
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    async fn stop_reason_for_incomplete(reason: &str) -> Option<StopReason> {
        stop_reasons_for_incomplete(reason).await.0
    }

    /// A token-budget cut maps to Length (the salvageable class).
    #[tokio::test]
    async fn incomplete_max_output_tokens_maps_to_length() {
        assert_eq!(
            stop_reasons_for_incomplete("max_output_tokens").await,
            (Some(StopReason::Length), Some("max_tokens".to_string()))
        );
    }

    /// Context-window exhaustion ("max_prompt_tokens", the xAI extension) is also a Length cut, not the unknown-reason fallback.
    /// It keeps its wire distinction in `raw_stop_reason`, in the Messages vocabulary.
    #[tokio::test]
    async fn incomplete_max_prompt_tokens_maps_to_length() {
        assert_eq!(
            stop_reasons_for_incomplete("max_prompt_tokens").await,
            (
                Some(StopReason::Length),
                Some("model_context_window_exceeded".to_string())
            )
        );
    }

    /// A server time-limit cut ("max_time_limit", the xAI extension) is a known Length cut, not the unknown-reason fallback.
    /// It carries no raw reason (the Messages vocabulary has no word for it).
    #[tokio::test]
    async fn incomplete_max_time_limit_maps_to_length() {
        assert_eq!(
            stop_reasons_for_incomplete("max_time_limit").await,
            (Some(StopReason::Length), None)
        );
    }

    /// A moderation cut maps to ContentFilter, never Length: a filter-cut response must not be salvaged and continued by `LengthPolicy`.
    #[tokio::test]
    async fn incomplete_content_filter_maps_to_content_filter() {
        assert_eq!(
            stop_reason_for_incomplete("content_filter").await,
            Some(StopReason::ContentFilter)
        );
    }

    /// A missing `incomplete_details` still maps to Length: an Incomplete response must never look like a clean Stop.
    #[tokio::test]
    async fn incomplete_without_details_maps_to_length() {
        let event =
            rs::ResponseStreamEvent::ResponseIncomplete(rs_types::ResponseIncompleteEvent {
                response: build_response(rs_types::Status::Incomplete),
                sequence_number: 0,
            });
        let raw = stream::iter(vec![Ok(text_delta_event("cut")), Ok(event)]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.stop_reason, Some(StopReason::Length));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    /// Pins the precedence where tool calls beat an Incomplete status.
    /// A truncated response that still carries a function call reports ToolCalls, not Length.
    /// The pair stays coherent: no raw length reason rides along, so the headless output keeps reporting `tool_use` for tool-bearing turns.
    #[tokio::test]
    async fn incomplete_with_tool_calls_maps_to_tool_calls() {
        let mut response = build_response(rs_types::Status::Incomplete);
        response.incomplete_details = Some(rs_types::IncompleteDetails {
            reason: "max_output_tokens".into(),
        });
        response.output = vec![rs_types::OutputItem::FunctionCall(
            rs_types::FunctionToolCall {
                arguments: "{\"x\":1".into(),
                call_id: "call_1".into(),
                name: "do_thing".into(),
                id: None,
                status: None,
            },
        )];
        let event =
            rs::ResponseStreamEvent::ResponseIncomplete(rs_types::ResponseIncompleteEvent {
                response,
                sequence_number: 0,
            });
        let raw = stream::iter(vec![Ok(event)]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.stop_reason, Some(StopReason::ToolCalls));
                assert_eq!(response.raw_stop_reason, None);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    /// The unknown-reason arm is the forward-compatibility story.
    /// A wire value this client has never seen collapses to Length (salvageable, never a parse failure) and carries no raw reason.
    #[tokio::test]
    async fn incomplete_unknown_reason_maps_to_length() {
        assert_eq!(
            stop_reasons_for_incomplete("some_future_reason").await,
            (Some(StopReason::Length), None)
        );
    }

    #[tokio::test]
    async fn text_delta_then_completed_yields_completed_with_stop() {
        let raw = stream::iter(vec![Ok(text_delta_event("hello")), Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        let text_tokens: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                SamplingEvent::ChannelToken {
                    channel: SamplingChannel::Text,
                    text,
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text_tokens, vec!["hello"]);

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.stop_reason, Some(StopReason::Stop));
                assert_eq!(
                    response.assistant().map(|a| a.content.as_ref()),
                    Some("hello"),
                    "streamed deltas must land on the assistant so empty-response retries do not concatenate"
                );
                assert!(
                    response.empty_reason().is_none(),
                    "a streamed greeting must not be classified empty"
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    fn text_done_event(text: &str) -> rs::ResponseStreamEvent {
        rs::ResponseStreamEvent::ResponseOutputTextDone(rs_types::ResponseTextDoneEvent {
            sequence_number: 0,
            item_id: "item-1".into(),
            output_index: 0,
            content_index: 0,
            text: text.into(),
            logprobs: None,
        })
    }

    #[tokio::test]
    async fn output_text_done_without_deltas_fills_empty_snapshot() {
        let raw = stream::iter(vec![Ok(text_done_event("hello")), Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(
                    response.assistant().map(|a| a.content.as_ref()),
                    Some("hello")
                );
                assert!(response.empty_reason().is_none());
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn empty_failed_response_is_not_treated_as_output() {
        let event = rs::ResponseStreamEvent::ResponseFailed(rs_types::ResponseFailedEvent {
            response: failed_response_with_error("boom"),
            sequence_number: 0,
        });
        assert!(!responses_event_may_have_output(&event));
    }

    #[tokio::test]
    async fn response_failed_yields_failed_500() {
        let failed = rs::ResponseStreamEvent::ResponseFailed(rs_types::ResponseFailedEvent {
            response: failed_response_with_error("boom"),
            sequence_number: 0,
        });
        let raw = stream::iter(vec![Ok(failed)]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(error.kind, crate::events::SamplingErrorKind::Api);
                assert_eq!(error.status_code, Some(500));
                assert!(error.message.contains("boom"));
                // The wire code passes through verbatim; dropping it here would disable strip recovery for coded Responses failures
                assert_eq!(
                    error.error_code,
                    Some(xai_grok_sampling_types::ApiErrorCode::Other(
                        "server_error".into()
                    ))
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// A coded `error` event must carry its code into the Failed info.
    /// This is the whole mid-stream strip-recovery chain for the Responses backend (the synthesized 500 with its code classifies as an image error).
    #[tokio::test]
    async fn response_error_event_carries_code_into_failed() {
        let error_event = rs::ResponseStreamEvent::ResponseError(rs_types::ResponseErrorEvent {
            sequence_number: 0,
            code: Some(xai_grok_sampling_types::INVALID_IMAGE_ERROR_CODE.into()),
            message: "could not decode image".into(),
            param: None,
        });
        let raw = stream::iter(vec![Ok(error_event)]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(
                    error.error_code,
                    Some(xai_grok_sampling_types::ApiErrorCode::InvalidImage)
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mid_stream_transport_error_yields_failed() {
        let raw = stream::iter(vec![
            Ok(text_delta_event("hi")),
            Err(SamplingError::EventStreamError("conn reset".into())),
        ])
        .boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        assert!(
            events
                .iter()
                .any(|e| matches!(e, SamplingEvent::Failed { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SamplingEvent::Completed { .. }))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn idle_timeout_when_stream_stalls() {
        let raw = stream::iter(vec![Ok(text_delta_event("hi"))])
            .chain(stream::pending())
            .boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_millis(100),
            None,
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(error.kind, crate::events::SamplingErrorKind::IdleTimeout);
            }
            other => panic!("expected Failed(IdleTimeout), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn model_metadata_yielded_after_stream_started() {
        let raw = stream::iter(vec![Ok(completed_event())]).boxed();
        let metadata = ResponseModelMetadata {
            context_window: Some(8192),
            ..Default::default()
        };
        let events = collect(stream_responses(
            raw,
            Some(metadata),
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        assert!(matches!(
            events.first(),
            Some(SamplingEvent::StreamStarted { .. })
        ));
        assert!(matches!(
            events.get(1),
            Some(SamplingEvent::ModelMetadata { .. })
        ));
    }

    #[test]
    fn meaningful_content_classifier_basics() {
        let event = text_delta_event("foo");
        assert!(responses_event_has_meaningful_content(&event));
        let empty = text_delta_event("");
        assert!(!responses_event_has_meaningful_content(&empty));
        assert!(responses_event_has_meaningful_content(&completed_event()));
    }

    #[test]
    fn output_classifier_covers_non_forwarded_backend_events() {
        let queued = rs::ResponseStreamEvent::ResponseQueued(rs_types::ResponseQueuedEvent {
            sequence_number: 0,
            response: empty_completed_response(),
        });
        assert!(!responses_event_may_have_output(&queued));

        let response_error = rs::ResponseStreamEvent::ResponseError(rs_types::ResponseErrorEvent {
            sequence_number: 1,
            code: Some("server_error".into()),
            message: "failed before output".into(),
            param: None,
        });
        assert!(!responses_event_may_have_output(&response_error));

        let refusal =
            rs::ResponseStreamEvent::ResponseRefusalDelta(rs_types::ResponseRefusalDeltaEvent {
                sequence_number: 1,
                item_id: "item-1".into(),
                output_index: 0,
                content_index: 0,
                delta: "no".into(),
            });
        assert!(responses_event_may_have_output(&refusal));

        let backend_progress = rs::ResponseStreamEvent::ResponseWebSearchCallSearching(
            rs_types::ResponseWebSearchCallSearchingEvent {
                sequence_number: 2,
                output_index: 0,
                item_id: "search-1".into(),
            },
        );
        assert!(responses_event_may_have_output(&backend_progress));
    }

    #[tokio::test]
    async fn tracked_stream_marks_non_forwarded_refusal_as_output() {
        let output_observed = Arc::new(AtomicBool::new(false));
        let refusal =
            rs::ResponseStreamEvent::ResponseRefusalDelta(rs_types::ResponseRefusalDeltaEvent {
                sequence_number: 0,
                item_id: "item-1".into(),
                output_index: 0,
                content_index: 0,
                delta: "no".into(),
            });
        let raw = stream::iter(vec![Ok(refusal), Ok(completed_event())]).boxed();
        let _ = collect(stream_responses_tracked(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
            Arc::clone(&output_observed),
            FailedResponseCapture::default(),
        ))
        .await;

        assert!(output_observed.load(Ordering::Relaxed));
    }

    /// A server-side code-interpreter run is emitted as a generic backend tool call named "code_interpreter", the same shape as x_search.
    /// It starts on InProgress and completes on OutputItemDone.
    #[tokio::test]
    async fn code_interpreter_forwards_backend_tool_call() {
        let in_progress = rs::ResponseStreamEvent::ResponseCodeInterpreterCallInProgress(
            rs_types::ResponseCodeInterpreterCallInProgressEvent {
                sequence_number: 0,
                output_index: 0,
                item_id: "ci-1".into(),
            },
        );
        let done = rs::ResponseStreamEvent::ResponseOutputItemDone(
            rs_types::ResponseOutputItemDoneEvent {
                sequence_number: 1,
                output_index: 0,
                item: rs_types::OutputItem::CodeInterpreterCall(
                    rs_types::CodeInterpreterToolCall {
                        code: Some("print(1)".into()),
                        container_id: "cont-1".into(),
                        id: "ci-1".into(),
                        outputs: None,
                        status: rs_types::CodeInterpreterToolCallStatus::Completed,
                    },
                ),
            },
        );
        let raw = stream::iter(vec![Ok(in_progress), Ok(done), Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        assert!(
            events.iter().any(|e| matches!(
                e,
                SamplingEvent::BackendToolCallStarted { call_id, name, .. }
                    if call_id == "ci-1" && name == "code_interpreter"
            )),
            "expected a code_interpreter BackendToolCallStarted, got {events:?}"
        );
        let completed = events.iter().find_map(|e| match e {
            SamplingEvent::BackendToolCallCompleted {
                call_id,
                name,
                result,
                ..
            } if name == "code_interpreter" => Some((call_id.clone(), result.clone())),
            _ => None,
        });
        let (call_id, result) = completed.expect("a code_interpreter BackendToolCallCompleted");
        assert_eq!(call_id, "ci-1");
        let result = result.expect("serialized code-interpreter payload");
        assert_eq!(result.get("code"), Some(&serde_json::json!("print(1)")));
    }

    fn message_item(id: &str, texts: &[&str]) -> rs::OutputItem {
        rs::OutputItem::Message(rs::OutputMessage {
            id: id.into(),
            content: texts
                .iter()
                .map(|text| {
                    rs::OutputMessageContent::OutputText(rs::OutputTextContent {
                        text: (*text).into(),
                        annotations: vec![],
                        logprobs: None,
                    })
                })
                .collect(),
            role: rs::AssistantRole::Assistant,
            status: rs::OutputStatus::Completed,
        })
    }

    fn indexed_text(index: u32, content: u32, text: &str, done: bool) -> rs::ResponseStreamEvent {
        let mut event = if done {
            text_done_event(text)
        } else {
            text_delta_event(text)
        };
        match &mut event {
            rs::ResponseStreamEvent::ResponseOutputTextDelta(ev) => {
                ev.output_index = index;
                ev.content_index = content;
                ev.item_id = format!("msg-{index}");
            }
            rs::ResponseStreamEvent::ResponseOutputTextDone(ev) => {
                ev.output_index = index;
                ev.content_index = content;
                ev.item_id = format!("msg-{index}");
            }
            _ => unreachable!(),
        }
        event
    }

    #[tokio::test]
    async fn text_recovery_reconciles_multiple_items_parts_and_done_frames() {
        let mut response = empty_completed_response();
        // The compact snapshot omits the first message and one content part.
        response.output = vec![message_item("msg-2", &["second"])];
        let frames = vec![
            indexed_text(0, 0, "hel", false),
            indexed_text(2, 0, "second", false),
            indexed_text(0, 0, "hello", true),
            indexed_text(0, 1, "world", true),
            indexed_text(2, 1, "tail", true),
            rs::ResponseStreamEvent::ResponseCompleted(rs::ResponseCompletedEvent {
                response,
                sequence_number: 0,
            }),
        ];
        let events = collect(stream_responses(
            stream::iter(frames.into_iter().map(Ok)).boxed(),
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        let SamplingEvent::Completed { response, .. } = events.last().unwrap() else {
            panic!("missing completion")
        };
        assert_eq!(
            response.assistant().unwrap().content.as_ref(),
            "hello\nworld\nsecond\ntail"
        );
    }

    #[tokio::test]
    async fn tool_recovery_merges_partial_snapshot_by_item_id_and_preserves_other_calls() {
        let mut response = empty_completed_response();
        let mut snapshot = function_tool_call("", "", "{\"x\":");
        snapshot.id = Some("item-1".into());
        response.output = vec![rs::OutputItem::FunctionCall(snapshot)];
        let mut first = function_tool_call("call-1", "read_file", "");
        first.id = Some("item-1".into());
        let frames = vec![
            rs::ResponseStreamEvent::ResponseOutputItemAdded(rs::ResponseOutputItemAddedEvent {
                output_index: 1,
                item: rs::OutputItem::FunctionCall(first),
                sequence_number: 0,
            }),
            function_call_args_delta_event(1, "{\"x\":1}"),
            function_call_added_with_args(3, "call-2", "bash", "{}"),
            rs::ResponseStreamEvent::ResponseCompleted(rs::ResponseCompletedEvent {
                response,
                sequence_number: 0,
            }),
        ];
        let events = collect(stream_responses(
            stream::iter(frames.into_iter().map(Ok)).boxed(),
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        let SamplingEvent::Completed { response, .. } = events.last().unwrap() else {
            panic!("missing completion")
        };
        let calls = &response.assistant().unwrap().tool_calls;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id.as_ref(), "call-1");
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments.as_ref(), "{\"x\":1}");
        assert_eq!(calls[1].id.as_ref(), "call-2");
    }

    #[tokio::test]
    async fn repeated_tool_added_and_done_do_not_duplicate_calls_or_headers() {
        let frames = vec![
            function_call_added_event(0, "call-1", "bash"),
            function_call_added_event(0, "call-1", "bash"),
            function_call_args_delta_event(0, "{}"),
            function_call_done_event(0, "call-1", "bash", ""),
            completed_event(),
        ];
        let events = collect(stream_responses(
            stream::iter(frames.into_iter().map(Ok)).boxed(),
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        assert_eq!(tool_call_deltas(&events).len(), 2);
        let SamplingEvent::Completed { response, .. } = events.last().unwrap() else {
            panic!("missing completion")
        };
        let calls = &response.assistant().unwrap().tool_calls;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments.as_ref(), "{}");
    }

    #[test]
    fn snapshot_conflicts_win_without_losing_streamed_siblings() {
        let mut streamed = StreamedOutput::default();
        streamed.observe(&indexed_text(0, 0, "stream", false));
        streamed.observe(&indexed_text(0, 1, "sibling", true));
        let mut response = empty_completed_response();
        response.output = vec![message_item("msg-0", &["snapshot"])];
        streamed.reconcile(&mut response);
        let items = xai_grok_sampling_types::response_to_conversation_items(response);
        let ConversationItem::Assistant(assistant) = items.last().unwrap() else {
            panic!("missing assistant")
        };
        assert_eq!(assistant.content.as_ref(), "snapshot\nsibling");
    }

    #[test]
    fn sparse_snapshot_uses_streamed_indices_not_compacted_positions() {
        let mut streamed = StreamedOutput::default();
        streamed.observe(&indexed_text(1, 0, "first", true));
        streamed.observe(&indexed_text(3, 0, "last", true));
        let mut response = empty_completed_response();
        response.output = vec![message_item("msg-3", &["last"])];
        streamed.reconcile(&mut response);
        let items = xai_grok_sampling_types::response_to_conversation_items(response);
        let ConversationItem::Assistant(assistant) = items.last().unwrap() else {
            panic!("missing assistant")
        };
        assert_eq!(assistant.content.as_ref(), "first\nlast");
    }

    #[test]
    fn arguments_before_added_and_empty_done_keep_partial_call() {
        let mut streamed = StreamedOutput::default();
        streamed.observe(&function_call_args_delta_event(0, "{\"partial\":"));
        streamed.observe(&function_call_added_event(0, "call-1", "bash"));
        streamed.observe(&function_call_done_event(0, "call-1", "bash", ""));
        let mut response = empty_completed_response();
        streamed.reconcile(&mut response);
        let rs::OutputItem::FunctionCall(call) = &response.output[0] else {
            panic!("missing call")
        };
        assert_eq!(call.call_id, "call-1");
        assert_eq!(call.arguments, "{\"partial\":");
    }

    #[test]
    fn message_done_without_text_frames_recovers_all_parts() {
        let mut streamed = StreamedOutput::default();
        streamed.observe(&rs::ResponseStreamEvent::ResponseOutputItemDone(
            rs::ResponseOutputItemDoneEvent {
                output_index: 0,
                sequence_number: 0,
                item: message_item("msg-0", &["one", "two"]),
            },
        ));
        let mut response = empty_completed_response();
        streamed.reconcile(&mut response);
        let items = xai_grok_sampling_types::response_to_conversation_items(response);
        let ConversationItem::Assistant(assistant) = items.last().unwrap() else {
            panic!("missing assistant")
        };
        assert_eq!(assistant.content.as_ref(), "one\ntwo");
    }

    #[test]
    fn arguments_only_frames_can_complete_an_identified_snapshot_call() {
        let mut streamed = StreamedOutput::default();
        streamed.observe(&function_call_args_delta_event(4, "{\"x\":1}"));
        let mut call = function_tool_call("call-4", "bash", "");
        call.id = Some("item-4".into());
        let mut response = empty_completed_response();
        response.output = vec![rs::OutputItem::FunctionCall(call)];
        streamed.reconcile(&mut response);
        assert_eq!(response.output.len(), 1);
        let rs::OutputItem::FunctionCall(call) = &response.output[0] else {
            panic!("missing call")
        };
        assert_eq!(call.arguments, "{\"x\":1}");
    }

    #[test]
    fn initial_arguments_and_deltas_merge_without_losing_the_prefix() {
        let mut streamed = StreamedOutput::default();
        streamed.observe(&function_call_added_with_args(
            0, "call-1", "bash", "{\"x\":",
        ));
        streamed.observe(&function_call_args_delta_event(0, "1}"));
        let mut response = empty_completed_response();
        streamed.reconcile(&mut response);
        let rs::OutputItem::FunctionCall(call) = &response.output[0] else {
            panic!("missing call")
        };
        assert_eq!(call.arguments, "{\"x\":1}");
    }

    fn function_tool_call(
        call_id: &str,
        name: &str,
        arguments: &str,
    ) -> rs_types::FunctionToolCall {
        rs_types::FunctionToolCall {
            arguments: arguments.into(),
            call_id: call_id.into(),
            name: name.into(),
            id: None,
            status: None,
        }
    }

    fn function_call_added_event(
        output_index: u32,
        call_id: &str,
        name: &str,
    ) -> rs::ResponseStreamEvent {
        function_call_added_with_args(output_index, call_id, name, "")
    }

    fn function_call_added_with_args(
        output_index: u32,
        call_id: &str,
        name: &str,
        arguments: &str,
    ) -> rs::ResponseStreamEvent {
        rs::ResponseStreamEvent::ResponseOutputItemAdded(rs_types::ResponseOutputItemAddedEvent {
            sequence_number: 0,
            output_index,
            item: rs_types::OutputItem::FunctionCall(function_tool_call(call_id, name, arguments)),
        })
    }

    fn function_call_done_event(
        output_index: u32,
        call_id: &str,
        name: &str,
        arguments: &str,
    ) -> rs::ResponseStreamEvent {
        rs::ResponseStreamEvent::ResponseOutputItemDone(rs_types::ResponseOutputItemDoneEvent {
            sequence_number: 0,
            output_index,
            item: rs_types::OutputItem::FunctionCall(function_tool_call(call_id, name, arguments)),
        })
    }

    fn function_call_args_delta_event(output_index: u32, delta: &str) -> rs::ResponseStreamEvent {
        rs::ResponseStreamEvent::ResponseFunctionCallArgumentsDelta(
            rs_types::ResponseFunctionCallArgumentsDeltaEvent {
                sequence_number: 0,
                item_id: format!("item-{output_index}"),
                output_index,
                delta: delta.into(),
            },
        )
    }

    type Delta = (u32, Option<String>, Option<String>, Option<String>);

    /// Extract all ToolCallDelta events as (tool_index, id, name, arguments_delta).
    fn tool_call_deltas(evs: &[SamplingEvent]) -> Vec<Delta> {
        evs.iter()
            .filter_map(|e| match e {
                SamplingEvent::ToolCallDelta {
                    tool_index,
                    id,
                    name,
                    arguments_delta,
                    ..
                } => Some((
                    *tool_index,
                    id.clone(),
                    name.clone(),
                    arguments_delta.clone(),
                )),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn function_call_emits_initial_id_name_then_arg_deltas() {
        let events: Vec<Result<rs::ResponseStreamEvent, SamplingError>> = vec![
            Ok(function_call_added_event(0, "call_xyz", "do_thing")),
            Ok(function_call_args_delta_event(0, "{\"x\":")),
            Ok(function_call_args_delta_event(0, "1}")),
            Ok(completed_event()),
        ];
        let raw = stream::iter(events).boxed();
        let evs = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        let deltas = tool_call_deltas(&evs);

        let [d0, d1, d2] = deltas.as_slice() else {
            panic!("expected three deltas: {deltas:?}");
        };
        assert_eq!(d0.0, 0);
        assert_eq!(d0.1.as_deref(), Some("call_xyz"));
        assert_eq!(d0.2.as_deref(), Some("do_thing"));
        assert_eq!(d0.3, None);
        assert_eq!(d1.0, 0);
        assert_eq!(d1.1, None);
        assert_eq!(d1.2, None);
        assert_eq!(d1.3.as_deref(), Some("{\"x\":"));
        assert_eq!(d2.3.as_deref(), Some("1}"));

        match evs.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let assistant = response.assistant().expect("assistant");
                assert_eq!(assistant.tool_calls.len(), 1);
                assert_eq!(assistant.tool_calls[0].id.as_ref(), "call_xyz");
                assert_eq!(assistant.tool_calls[0].name, "do_thing");
                assert_eq!(assistant.tool_calls[0].arguments.as_ref(), "{\"x\":1}");
                assert_eq!(response.stop_reason, Some(StopReason::ToolCalls));
                assert!(
                    response.empty_reason().is_none(),
                    "streamed function calls must not be classified empty"
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    /// Codex (`store: false`) can emit a complete FunctionCall on Added, then
    /// `response.completed` with an empty `output` array. Astra turns that look
    /// like `writing_tool_call` then `empty response | retrying`.
    #[tokio::test]
    async fn function_call_on_added_fills_empty_snapshot() {
        let events: Vec<Result<rs::ResponseStreamEvent, SamplingError>> = vec![
            Ok(function_call_added_with_args(
                0,
                "call_xyz",
                "read_file",
                "{\"target_file\":\"foo.rs\"}",
            )),
            Ok(completed_event()),
        ];
        let raw = stream::iter(events).boxed();
        let evs = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        match evs.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let assistant = response.assistant().expect("assistant");
                assert_eq!(assistant.tool_calls.len(), 1);
                assert_eq!(assistant.tool_calls[0].id.as_ref(), "call_xyz");
                assert_eq!(assistant.tool_calls[0].name, "read_file");
                assert_eq!(
                    assistant.tool_calls[0].arguments.as_ref(),
                    "{\"target_file\":\"foo.rs\"}"
                );
                assert_eq!(response.stop_reason, Some(StopReason::ToolCalls));
                assert!(response.empty_reason().is_none());
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn function_call_on_done_fills_empty_snapshot() {
        let events: Vec<Result<rs::ResponseStreamEvent, SamplingError>> = vec![
            Ok(function_call_done_event(
                0,
                "call_xyz",
                "read_file",
                "{\"target_file\":\"foo.rs\"}",
            )),
            Ok(completed_event()),
        ];
        let raw = stream::iter(events).boxed();
        let evs = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        match evs.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let assistant = response.assistant().expect("assistant");
                assert_eq!(assistant.tool_calls.len(), 1);
                assert_eq!(assistant.tool_calls[0].name, "read_file");
                assert_eq!(
                    assistant.tool_calls[0].arguments.as_ref(),
                    "{\"target_file\":\"foo.rs\"}"
                );
                assert!(response.empty_reason().is_none());
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn function_call_fallback_preserves_distinct_snapshot_calls() {
        let mut response = empty_completed_response();
        response.output = vec![rs_types::OutputItem::FunctionCall(function_tool_call(
            "from_snapshot",
            "bash",
            "{}",
        ))];
        let completed =
            rs::ResponseStreamEvent::ResponseCompleted(rs_types::ResponseCompletedEvent {
                response,
                sequence_number: 0,
            });
        let events: Vec<Result<rs::ResponseStreamEvent, SamplingError>> = vec![
            Ok(function_call_added_with_args(
                0,
                "from_stream",
                "read_file",
                "{\"x\":1}",
            )),
            Ok(completed),
        ];
        let raw = stream::iter(events).boxed();
        let evs = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        match evs.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let assistant = response.assistant().expect("assistant");
                assert_eq!(assistant.tool_calls.len(), 2);
                assert_eq!(assistant.tool_calls[1].id.as_ref(), "from_snapshot");
                assert_eq!(assistant.tool_calls[1].name, "bash");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn function_call_args_delta_without_added_event_is_dropped() {
        // ArgumentsDelta with no preceding OutputItemAdded has no output_index to tool_index mapping, so it is dropped silently
        let events: Vec<Result<rs::ResponseStreamEvent, SamplingError>> = vec![
            Ok(function_call_args_delta_event(7, "{\"oops\":1}")),
            Ok(completed_event()),
        ];
        let raw = stream::iter(events).boxed();
        let evs = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        assert_eq!(tool_call_deltas(&evs).len(), 0);
    }

    #[tokio::test]
    async fn multiple_function_calls_get_distinct_tool_indices() {
        let events: Vec<Result<rs::ResponseStreamEvent, SamplingError>> = vec![
            Ok(function_call_added_event(0, "call_a", "tool_a")),
            Ok(function_call_added_event(1, "call_b", "tool_b")),
            Ok(function_call_args_delta_event(0, "a-args")),
            Ok(function_call_args_delta_event(1, "b-args")),
            Ok(completed_event()),
        ];
        let raw = stream::iter(events).boxed();
        let evs = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        let deltas = tool_call_deltas(&evs);

        let [d0, d1, d2, d3] = deltas.as_slice() else {
            panic!("expected four deltas: {deltas:?}");
        };
        assert_eq!(d0.0, 0);
        assert_eq!(d0.1.as_deref(), Some("call_a"));
        assert_eq!(d1.0, 1);
        assert_eq!(d1.1.as_deref(), Some("call_b"));
        assert_eq!(d2.0, 0);
        assert_eq!(d2.3.as_deref(), Some("a-args"));
        assert_eq!(d3.0, 1);
        assert_eq!(d3.3.as_deref(), Some("b-args"));
    }

    #[tokio::test]
    async fn doom_loop_collector_signals_land_on_completed_response() {
        use xai_grok_sampling_types::doom_loop::{
            DOOM_LOOP_CHECK_EVENT_TYPE, SAMPLE_CHECK_EVENT_DATA,
        };
        let collector = crate::doom_loop::DoomLoopSignalCollector::default();
        assert!(collector.absorb(DOOM_LOOP_CHECK_EVENT_TYPE, SAMPLE_CHECK_EVENT_DATA));
        let raw = stream::iter(vec![Ok(text_delta_event("hello")), Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            Some(collector),
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.doom_loop_signals.len(), 1);
                let Some(signal) = response.doom_loop_signals.first() else {
                    panic!("expected doom loop signal: {response:?}");
                };
                assert_eq!(signal.raw, "tail_repetition:4@response");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    /// An armed collector holding a confident signal aborts the attempt with a retryable doom-loop failure; all detector labels are emitted first.
    /// After `disarm_abort`, the same stream completes and the signals ride the response.
    #[tokio::test]
    async fn confident_signal_aborts_stream_unless_disarmed() {
        let confident = r#"{"type":"response.doom_loop_check","doom_loop_check":{"triggers":["tail_repetition:8@thinking","exact_repetition:42x3@thinking"]}}"#;

        let collector = crate::doom_loop::DoomLoopSignalCollector::default();
        assert!(collector.absorb("response.doom_loop_check", confident));
        let raw = stream::iter(vec![Ok(text_delta_event("hi")), Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            Some(collector),
        ))
        .await;
        assert!(matches!(
            events.get(events.len().saturating_sub(2)),
            Some(SamplingEvent::DoomLoopSignals { triggers, .. })
                if triggers == &[
                    "tail_repetition:8@thinking".to_string(),
                    "exact_repetition:42x3@thinking".to_string(),
                ]
        ));
        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(
                    error.kind,
                    crate::events::SamplingErrorKind::DoomLoopDetected
                );
                assert!(error.is_retryable);
                assert_eq!(
                    error.doom_loop_triggers.as_deref(),
                    Some(["tail_repetition:8@thinking".to_string()].as_slice())
                );
            }
            other => panic!("expected Failed(DoomLoopDetected), got {other:?}"),
        }
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SamplingEvent::Completed { .. }))
        );

        let collector = crate::doom_loop::DoomLoopSignalCollector::default();
        assert!(collector.absorb("response.doom_loop_check", confident));
        collector.disarm_abort();
        let raw = stream::iter(vec![Ok(text_delta_event("hi")), Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            Some(collector),
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.doom_loop_signals.len(), 2);
            }
            other => panic!("expected Completed after disarm, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn doom_loop_signals_empty_without_collector_or_triggers() {
        let raw = stream::iter(vec![Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert!(response.doom_loop_signals.is_empty());
            }
            other => panic!("expected Completed, got {other:?}"),
        }

        // A collector that never saw a trigger also leaves the field empty.
        let raw = stream::iter(vec![Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            Some(crate::doom_loop::DoomLoopSignalCollector::default()),
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert!(response.doom_loop_signals.is_empty());
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }
}
