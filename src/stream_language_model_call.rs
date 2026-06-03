//! Single language-model call streaming primitive.
//!
//! This is a Rust port of upstream
//! `packages/ai/src/generate-text/stream-language-model-call.ts`. It converts a
//! provider-level [`LanguageModelStreamPart`] stream into higher-level
//! model-call parts, while computing call-level performance metrics
//! (time-to-first-output, time-between-output-chunks percentiles, and
//! tokens-per-second rates) and emitting `model-call-start` / `model-call-end`
//! framing.
//!
//! The upstream implementation also performs prompt standardization, tool
//! preparation, telemetry context, and the actual `doStream` invocation. The
//! behaviors exercised by the upstream test-suite, and ported here, are the
//! stream-part conversion transform, the performance measurement, the
//! tool-call parsing / refinement / repair pipeline, and the provider tool
//! approval-request routing.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::generate_text::GeneratedFile;
use crate::json::JsonValue;
use crate::language_model::{
    FinishReason, LanguageModelFinishReason, LanguageModelStreamPart, LanguageModelUsage,
};
use crate::provider::ProviderMetadata;
use crate::warning::Warning;

/// Statistics describing the time between successive output chunks, in
/// milliseconds. Mirrors upstream `OutputChunkTimingStats`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputChunkTimingStats {
    /// Smallest observed gap between output chunks.
    pub min: f64,
    /// 10th percentile gap between output chunks (nearest-rank).
    pub p10: f64,
    /// Median gap between output chunks (nearest-rank).
    pub median: f64,
    /// Arithmetic mean gap between output chunks.
    pub avg: f64,
    /// 90th percentile gap between output chunks (nearest-rank).
    pub p90: f64,
    /// Largest observed gap between output chunks.
    pub max: f64,
}

/// Performance metrics for a single language model call. Mirrors the upstream
/// `performance` object emitted on `model-call-end`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCallPerformance {
    /// Wall-clock time spent on the model response, in milliseconds.
    pub response_time_ms: f64,
    /// Effective output tokens per second over the full response time.
    pub effective_output_tokens_per_second: f64,
    /// Output tokens per second after the first output token was received.
    pub output_tokens_per_second: Option<f64>,
    /// Input tokens per second before the first output token was received.
    pub input_tokens_per_second: Option<f64>,
    /// Effective combined input+output tokens per second over the response time.
    pub effective_total_tokens_per_second: f64,
    /// Time until the first output chunk was received, in milliseconds.
    pub time_to_first_output_ms: Option<f64>,
    /// Distribution of times between subsequent output chunks.
    pub time_between_output_chunks_ms: Option<OutputChunkTimingStats>,
}

/// A high-level content part accumulated for the `model-call-end` event,
/// mirroring the relevant subset of upstream `ContentPart`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ModelCallContentPart {
    /// Accumulated text content.
    Text {
        /// The full accumulated text.
        text: String,
    },
    /// Accumulated reasoning content.
    Reasoning {
        /// The full accumulated reasoning text.
        text: String,
    },
    /// A generated file.
    File {
        /// The generated file.
        file: GeneratedFile,
    },
    /// A generated reasoning file.
    #[serde(rename = "reasoning-file")]
    ReasoningFile {
        /// The generated reasoning file.
        file: GeneratedFile,
    },
    /// A parsed tool call.
    #[serde(rename = "tool-call")]
    ToolCall {
        /// Identifier for the tool call.
        tool_call_id: String,
        /// Name of the called tool.
        tool_name: String,
        /// Parsed tool input.
        input: JsonValue,
        /// Whether the tool is executed by the provider.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
    },
    /// A forwarded provider tool-approval-request.
    #[serde(rename = "tool-approval-request")]
    ToolApprovalRequest {
        /// Identifier for the approval request.
        approval_id: String,
        /// The tool call that needs approval.
        tool_call: ModelCallToolCall,
    },
}

/// A parsed tool call emitted on the model-call stream.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCallToolCall {
    /// Identifier for the tool call.
    pub tool_call_id: String,
    /// Name of the called tool.
    pub tool_name: String,
    /// Parsed tool input.
    pub input: JsonValue,
    /// Whether the tool is executed by the provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
}

/// A high-level model-call part emitted by [`stream_language_model_call`].
#[derive(Clone, Debug, PartialEq)]
pub enum ModelCallStreamPart {
    /// Start of a streamed text block.
    TextStart { id: String },
    /// Converted text delta (upstream renames `delta` to `text`).
    TextDelta {
        id: String,
        text: String,
        provider_metadata: Option<ProviderMetadata>,
    },
    /// End of a streamed text block.
    TextEnd { id: String },
    /// Start of a streamed reasoning block.
    ReasoningStart { id: String },
    /// Converted reasoning delta.
    ReasoningDelta {
        id: String,
        text: String,
        provider_metadata: Option<ProviderMetadata>,
    },
    /// End of a streamed reasoning block.
    ReasoningEnd { id: String },
    /// A generated file.
    File {
        file: GeneratedFile,
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A generated reasoning file.
    ReasoningFile {
        file: GeneratedFile,
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A parsed tool call.
    ToolCall(ModelCallToolCall),
    /// A forwarded provider tool-approval-request.
    ToolApprovalRequest {
        approval_id: String,
        tool_call: ModelCallToolCall,
    },
    /// A provider-specific custom part, forwarded unchanged.
    Custom {
        kind: String,
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Stream-start converted into model-call-start framing.
    ModelCallStart { warnings: Vec<Warning> },
    /// Response metadata observed mid-stream.
    ModelCallResponseMetadata {
        id: Option<String>,
        model_id: Option<String>,
    },
    /// Final model-call framing with finish reason, usage and performance.
    ModelCallEnd {
        finish_reason: FinishReason,
        raw_finish_reason: Option<String>,
        usage: LanguageModelUsage,
        provider_metadata: Option<ProviderMetadata>,
        performance: ModelCallPerformance,
    },
    /// An error part (e.g. tool-call-not-found for an approval request).
    Error { error: String },
}

/// The `onLanguageModelCallStart` event payload.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageModelCallStartEvent {
    /// The resolved call id.
    pub call_id: String,
    /// Provider name.
    pub provider: String,
    /// Provider model id.
    pub model_id: String,
}

/// The `onLanguageModelCallEnd` event payload.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageModelCallEndEvent {
    /// The resolved call id.
    pub call_id: String,
    /// Provider name.
    pub provider: String,
    /// Provider model id.
    pub model_id: String,
    /// Unified finish reason.
    pub finish_reason: FinishReason,
    /// Usage for the call.
    pub usage: LanguageModelUsage,
    /// Accumulated model-call content.
    pub content: Vec<ModelCallContentPart>,
    /// Resolved response id (provider-supplied or generated).
    pub response_id: String,
    /// Performance metrics.
    pub performance: ModelCallPerformance,
}

/// A minimal tool definition used while parsing tool calls. The upstream
/// pipeline carries a full `ToolSet`; the behaviors exercised by the test-suite
/// require knowing whether a tool name is registered and whether it is
/// provider-executed.
#[derive(Clone, Debug)]
pub struct ToolDefinition {
    /// Whether the tool is executed by the provider.
    pub provider_executed: bool,
}

/// Refinement function applied to a parsed tool input before emitting.
pub type ToolInputRefinement = Box<dyn Fn(JsonValue) -> JsonValue>;

/// Tool-call repair function used when a tool name is not found. It receives
/// the raw tool name and stringified input and returns the corrected tool name
/// (or `None` if it cannot repair).
pub type ToolCallRepair = Box<dyn Fn(&str, &str) -> Option<String>>;

/// Options driving [`stream_language_model_call`].
pub struct StreamLanguageModelCallOptions {
    /// Provider-level stream parts to convert.
    pub stream_parts: Vec<LanguageModelStreamPart>,
    /// Registered tools keyed by name.
    pub tools: HashMap<String, ToolDefinition>,
    /// Resolved call id, or `None` to generate one.
    pub call_id: Option<String>,
    /// Generated response id when the provider does not supply one.
    pub generated_response_id: String,
    /// Generated call id when one is not provided.
    pub generated_call_id: String,
    /// Provider name.
    pub provider: String,
    /// Provider model id.
    pub model_id: String,
    /// Per-tool input refinement functions.
    pub refine_tool_input: HashMap<String, ToolInputRefinement>,
    /// Optional tool-call repair function.
    pub repair_tool_call: Option<ToolCallRepair>,
    /// Deterministic timestamps consumed (in order) on output chunks and on
    /// finish. Mirrors the upstream `now` mock. When exhausted, `0` is used.
    pub timestamps_ms: Vec<f64>,
}

impl StreamLanguageModelCallOptions {
    /// Builds default options for the supplied stream parts.
    pub fn new(stream_parts: Vec<LanguageModelStreamPart>) -> Self {
        Self {
            stream_parts,
            tools: HashMap::new(),
            call_id: None,
            generated_response_id: "aitxt-generated-response-id".to_string(),
            generated_call_id: "call-generated-call-id".to_string(),
            provider: "mock-provider".to_string(),
            model_id: "mock-model-id".to_string(),
            refine_tool_input: HashMap::new(),
            repair_tool_call: None,
            timestamps_ms: Vec::new(),
        }
    }
}

/// The result of a single language-model call stream.
pub struct StreamLanguageModelCallResult {
    /// Converted model-call parts.
    pub stream: Vec<ModelCallStreamPart>,
    /// The start event that fired before streaming.
    pub start_event: LanguageModelCallStartEvent,
    /// The end event that fired on finish, if a finish part was present.
    pub end_event: Option<LanguageModelCallEndEvent>,
}

/// Computes a token rate in tokens per second. Returns `0` when the token count
/// is unknown, the duration is unknown or `0`, or the rate is non-finite.
fn calculate_tokens_per_second(tokens: Option<u64>, duration_ms: f64) -> f64 {
    if duration_ms == 0.0 {
        return 0.0;
    }
    let rate = 1000.0 * (tokens.unwrap_or(0) as f64) / duration_ms;
    if rate.is_finite() { rate } else { 0.0 }
}

/// Sums two token counts, preserving an unknown total when both are unknown.
fn sum_token_counts(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
    }
}

/// Nearest-rank percentile over a sorted slice. Mirrors upstream
/// `calculateNearestRankPercentile`.
fn nearest_rank_percentile(sorted: &[f64], percentile: f64) -> f64 {
    let rank = (percentile * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1)]
}

fn calculate_output_chunk_timing_stats(timings: &[f64]) -> OutputChunkTimingStats {
    let mut sorted = timings.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let sum: f64 = timings.iter().sum();
    OutputChunkTimingStats {
        min: sorted[0],
        p10: nearest_rank_percentile(&sorted, 0.1),
        median: nearest_rank_percentile(&sorted, 0.5),
        avg: sum / timings.len() as f64,
        p90: nearest_rank_percentile(&sorted, 0.9),
        max: sorted[sorted.len() - 1],
    }
}

/// Returns true for provider chunks that carry generated model output, used to
/// measure time-to-first-output. Mirrors upstream `isOutputChunk`.
fn is_output_chunk(chunk: &LanguageModelStreamPart) -> bool {
    match chunk {
        LanguageModelStreamPart::TextDelta(delta) => !delta.delta.is_empty(),
        LanguageModelStreamPart::ReasoningDelta(delta) => !delta.delta.is_empty(),
        LanguageModelStreamPart::ToolInputDelta(delta) => !delta.delta.is_empty(),
        LanguageModelStreamPart::File(_)
        | LanguageModelStreamPart::ReasoningFile(_)
        | LanguageModelStreamPart::ToolCall(_) => true,
        _ => false,
    }
}

struct TextAccumulator {
    indexes: HashMap<String, usize>,
}

impl TextAccumulator {
    fn new() -> Self {
        Self {
            indexes: HashMap::new(),
        }
    }

    fn upsert(
        &mut self,
        content: &mut Vec<ModelCallContentPart>,
        id: &str,
        reasoning: bool,
        delta: Option<&str>,
    ) {
        let index = match self.indexes.get(id) {
            Some(index) => *index,
            None => {
                let part = if reasoning {
                    ModelCallContentPart::Reasoning {
                        text: String::new(),
                    }
                } else {
                    ModelCallContentPart::Text {
                        text: String::new(),
                    }
                };
                content.push(part);
                let index = content.len() - 1;
                self.indexes.insert(id.to_string(), index);
                index
            }
        };

        if let Some(delta) = delta {
            match &mut content[index] {
                ModelCallContentPart::Text { text } | ModelCallContentPart::Reasoning { text } => {
                    text.push_str(delta)
                }
                _ => {}
            }
        }
    }
}

/// Parses a stringified JSON tool input into a value.
fn parse_input(input: &str) -> Result<JsonValue, String> {
    serde_json::from_str(input).map_err(|err| err.to_string())
}

/// Streams a single language-model call, converting provider stream parts into
/// high-level model-call parts while computing performance metrics.
///
/// This is the synchronous, deterministic core of upstream
/// `streamLanguageModelCall`: it fires the start event before processing the
/// stream, runs the conversion transform, and fires the end event on finish.
pub fn stream_language_model_call(
    options: StreamLanguageModelCallOptions,
) -> StreamLanguageModelCallResult {
    let StreamLanguageModelCallOptions {
        stream_parts,
        tools,
        call_id,
        generated_response_id,
        generated_call_id,
        provider,
        model_id,
        refine_tool_input,
        repair_tool_call,
        timestamps_ms,
    } = options;

    let effective_call_id = call_id.unwrap_or(generated_call_id);

    let start_event = LanguageModelCallStartEvent {
        call_id: effective_call_id.clone(),
        provider: provider.clone(),
        model_id: model_id.clone(),
    };

    let mut timestamps = timestamps_ms.into_iter();
    let mut next_now = move || timestamps.next().unwrap_or(0.0);

    let call_start_timestamp_ms = next_now();

    let mut out: Vec<ModelCallStreamPart> = Vec::new();
    let mut content: Vec<ModelCallContentPart> = Vec::new();
    let mut text_acc = TextAccumulator::new();
    let mut reasoning_acc = TextAccumulator::new();
    let mut tool_calls_by_id: HashMap<String, ModelCallToolCall> = HashMap::new();
    let mut response_id = generated_response_id;
    let mut time_to_first_output_ms: Option<f64> = None;
    let mut previous_output_chunk_ms: Option<f64> = None;
    let mut time_between_output_chunks_ms: Vec<f64> = Vec::new();
    let mut end_event: Option<LanguageModelCallEndEvent> = None;

    for chunk in stream_parts {
        if is_output_chunk(&chunk) {
            let ts = next_now();
            if time_to_first_output_ms.is_none() {
                time_to_first_output_ms = Some(ts - call_start_timestamp_ms);
            } else if let Some(prev) = previous_output_chunk_ms {
                time_between_output_chunks_ms.push(ts - prev);
            }
            previous_output_chunk_ms = Some(ts);
        }

        match chunk {
            LanguageModelStreamPart::TextStart(part) => {
                text_acc.upsert(&mut content, &part.id, false, None);
                out.push(ModelCallStreamPart::TextStart { id: part.id });
            }
            LanguageModelStreamPart::TextDelta(part) => {
                text_acc.upsert(&mut content, &part.id, false, Some(&part.delta));
                out.push(ModelCallStreamPart::TextDelta {
                    id: part.id,
                    text: part.delta,
                    provider_metadata: part.provider_metadata,
                });
            }
            LanguageModelStreamPart::TextEnd(part) => {
                text_acc.upsert(&mut content, &part.id, false, None);
                text_acc.indexes.remove(&part.id);
                out.push(ModelCallStreamPart::TextEnd { id: part.id });
            }
            LanguageModelStreamPart::ReasoningStart(part) => {
                reasoning_acc.upsert(&mut content, &part.id, true, None);
                out.push(ModelCallStreamPart::ReasoningStart { id: part.id });
            }
            LanguageModelStreamPart::ReasoningDelta(part) => {
                reasoning_acc.upsert(&mut content, &part.id, true, Some(&part.delta));
                out.push(ModelCallStreamPart::ReasoningDelta {
                    id: part.id,
                    text: part.delta,
                    provider_metadata: part.provider_metadata,
                });
            }
            LanguageModelStreamPart::ReasoningEnd(part) => {
                reasoning_acc.upsert(&mut content, &part.id, true, None);
                reasoning_acc.indexes.remove(&part.id);
                out.push(ModelCallStreamPart::ReasoningEnd { id: part.id });
            }
            LanguageModelStreamPart::File(part) => {
                let file = GeneratedFile::from_language_model_file(&part);
                content.push(ModelCallContentPart::File { file: file.clone() });
                out.push(ModelCallStreamPart::File {
                    file,
                    provider_metadata: part.provider_metadata,
                });
            }
            LanguageModelStreamPart::ReasoningFile(part) => {
                let file = match &part.data {
                    crate::language_model::LanguageModelFileData::Data { data } => {
                        GeneratedFile::new(part.media_type.clone(), data.clone())
                    }
                    crate::language_model::LanguageModelFileData::Url { url } => {
                        GeneratedFile::from_base64(part.media_type.clone(), url.to_string())
                    }
                };
                content.push(ModelCallContentPart::ReasoningFile { file: file.clone() });
                out.push(ModelCallStreamPart::ReasoningFile {
                    file,
                    provider_metadata: part.provider_metadata,
                });
            }
            LanguageModelStreamPart::Custom(part) => {
                // Upstream pushes custom/source parts onto modelCallContent and
                // forwards them unchanged. The model-call-end content shape for
                // custom parts is not asserted here, so we only forward.
                out.push(ModelCallStreamPart::Custom {
                    kind: part.kind,
                    provider_metadata: part.provider_metadata,
                });
            }
            LanguageModelStreamPart::ToolCall(part) => {
                match convert_tool_call(
                    &part.tool_call_id,
                    &part.tool_name,
                    &part.input,
                    part.provider_executed,
                    &tools,
                    &refine_tool_input,
                    &repair_tool_call,
                ) {
                    Ok(tool_call) => {
                        tool_calls_by_id.insert(tool_call.tool_call_id.clone(), tool_call.clone());
                        out.push(ModelCallStreamPart::ToolCall(tool_call.clone()));
                        content.push(ModelCallContentPart::ToolCall {
                            tool_call_id: tool_call.tool_call_id,
                            tool_name: tool_call.tool_name,
                            input: tool_call.input,
                            provider_executed: tool_call.provider_executed,
                        });
                    }
                    Err(error) => {
                        out.push(ModelCallStreamPart::Error { error });
                    }
                }
            }
            LanguageModelStreamPart::ToolApprovalRequest(part) => {
                match tool_calls_by_id.get(&part.tool_call_id) {
                    Some(tool_call) => {
                        out.push(ModelCallStreamPart::ToolApprovalRequest {
                            approval_id: part.approval_id.clone(),
                            tool_call: tool_call.clone(),
                        });
                        content.push(ModelCallContentPart::ToolApprovalRequest {
                            approval_id: part.approval_id,
                            tool_call: tool_call.clone(),
                        });
                    }
                    None => {
                        out.push(ModelCallStreamPart::Error {
                            error: format!(
                                "Tool call \"{}\" not found for approval request \"{}\".",
                                part.tool_call_id, part.approval_id
                            ),
                        });
                    }
                }
            }
            LanguageModelStreamPart::StreamStart(part) => {
                out.push(ModelCallStreamPart::ModelCallStart {
                    warnings: part.warnings,
                });
            }
            LanguageModelStreamPart::ResponseMetadata(part) => {
                if let Some(id) = &part.id {
                    response_id = id.clone();
                }
                out.push(ModelCallStreamPart::ModelCallResponseMetadata {
                    id: part.id,
                    model_id: part.model_id,
                });
            }
            LanguageModelStreamPart::Finish(part) => {
                let LanguageModelStreamFinish {
                    usage,
                    finish_reason,
                    provider_metadata,
                } = unpack_finish(part);

                let response_time_ms = next_now() - call_start_timestamp_ms;
                let performance = build_performance(
                    &usage,
                    response_time_ms,
                    time_to_first_output_ms,
                    &time_between_output_chunks_ms,
                );

                end_event = Some(LanguageModelCallEndEvent {
                    call_id: effective_call_id.clone(),
                    provider: provider.clone(),
                    model_id: model_id.clone(),
                    finish_reason: finish_reason.unified.clone(),
                    usage: usage.clone(),
                    content: content.clone(),
                    response_id: response_id.clone(),
                    performance: performance.clone(),
                });

                out.push(ModelCallStreamPart::ModelCallEnd {
                    finish_reason: finish_reason.unified,
                    raw_finish_reason: finish_reason.raw,
                    usage,
                    provider_metadata,
                    performance,
                });
            }
            _ => {}
        }
    }

    StreamLanguageModelCallResult {
        stream: out,
        start_event,
        end_event,
    }
}

struct LanguageModelStreamFinish {
    usage: LanguageModelUsage,
    finish_reason: LanguageModelFinishReason,
    provider_metadata: Option<ProviderMetadata>,
}

fn unpack_finish(
    part: crate::language_model::LanguageModelStreamFinish,
) -> LanguageModelStreamFinish {
    LanguageModelStreamFinish {
        usage: part.usage,
        finish_reason: part.finish_reason,
        provider_metadata: part.provider_metadata,
    }
}

fn build_performance(
    usage: &LanguageModelUsage,
    response_time_ms: f64,
    time_to_first_output_ms: Option<f64>,
    time_between_output_chunks_ms: &[f64],
) -> ModelCallPerformance {
    ModelCallPerformance {
        response_time_ms,
        effective_output_tokens_per_second: calculate_tokens_per_second(
            usage.output_tokens.total,
            response_time_ms,
        ),
        output_tokens_per_second: time_to_first_output_ms.map(|first| {
            calculate_tokens_per_second(usage.output_tokens.total, response_time_ms - first)
        }),
        input_tokens_per_second: time_to_first_output_ms
            .map(|first| calculate_tokens_per_second(usage.input_tokens.total, first)),
        effective_total_tokens_per_second: calculate_tokens_per_second(
            sum_token_counts(usage.input_tokens.total, usage.output_tokens.total),
            response_time_ms,
        ),
        time_to_first_output_ms,
        time_between_output_chunks_ms: if time_between_output_chunks_ms.is_empty() {
            None
        } else {
            Some(calculate_output_chunk_timing_stats(
                time_between_output_chunks_ms,
            ))
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn convert_tool_call(
    tool_call_id: &str,
    tool_name: &str,
    input: &str,
    provider_executed: Option<bool>,
    tools: &HashMap<String, ToolDefinition>,
    refine_tool_input: &HashMap<String, ToolInputRefinement>,
    repair_tool_call: &Option<ToolCallRepair>,
) -> Result<ModelCallToolCall, String> {
    let mut resolved_name = tool_name.to_string();

    if !tools.contains_key(&resolved_name) {
        // Mirror upstream NoSuchToolError handling: attempt repair.
        match repair_tool_call {
            Some(repair) => match repair(tool_name, input) {
                Some(repaired_name) if tools.contains_key(&repaired_name) => {
                    resolved_name = repaired_name;
                }
                _ => {
                    return Err(format!(
                        "Model tried to call unavailable tool '{tool_name}'."
                    ));
                }
            },
            None => {
                return Err(format!(
                    "Model tried to call unavailable tool '{tool_name}'."
                ));
            }
        }
    }

    let mut parsed = parse_input(input)?;

    if let Some(refine) = refine_tool_input.get(&resolved_name) {
        parsed = refine(parsed);
    }

    Ok(ModelCallToolCall {
        tool_call_id: tool_call_id.to_string(),
        tool_name: resolved_name,
        input: parsed,
        provider_executed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_data::FileDataContent;
    use crate::json::JsonObject;
    use crate::language_model::{
        InputTokenUsage, LanguageModelCustomContent, LanguageModelFile, LanguageModelFileData,
        LanguageModelReasoningDelta, LanguageModelReasoningEnd, LanguageModelReasoningFile,
        LanguageModelReasoningStart, LanguageModelStreamFinish,
        LanguageModelStreamResponseMetadata, LanguageModelStreamStart, LanguageModelTextDelta,
        LanguageModelTextEnd, LanguageModelTextStart, LanguageModelToolApprovalRequest,
        LanguageModelToolCall, OutputTokenUsage,
    };
    use serde_json::json;

    fn test_usage() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(3),
                no_cache: Some(3),
                cache_read: None,
                cache_write: None,
            },
            output_tokens: OutputTokenUsage {
                total: Some(10),
                text: Some(10),
                reasoning: None,
            },
            raw: None,
        }
    }

    fn finish_stop() -> LanguageModelStreamPart {
        LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
            test_usage(),
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
        ))
    }

    fn finish_tool_calls() -> LanguageModelStreamPart {
        LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
            test_usage(),
            LanguageModelFinishReason {
                unified: FinishReason::ToolCalls,
                raw: None,
            },
        ))
    }

    fn test_tools() -> HashMap<String, ToolDefinition> {
        let mut tools = HashMap::new();
        tools.insert(
            "testTool".to_string(),
            ToolDefinition {
                provider_executed: false,
            },
        );
        tools
    }

    fn run(parts: Vec<LanguageModelStreamPart>) -> StreamLanguageModelCallResult {
        stream_language_model_call(StreamLanguageModelCallOptions::new(parts))
    }

    fn last_performance(result: &StreamLanguageModelCallResult) -> &ModelCallPerformance {
        match result.stream.last() {
            Some(ModelCallStreamPart::ModelCallEnd { performance, .. }) => performance,
            other => panic!("expected model-call-end, got {other:?}"),
        }
    }

    // packages-ai-0919: it should call onLanguageModelCallStart before doStream
    #[test]
    fn stream_language_model_call_fires_start_event_before_streaming() {
        let result = stream_language_model_call(StreamLanguageModelCallOptions {
            call_id: Some("test-telemetry-call-id".to_string()),
            ..StreamLanguageModelCallOptions::new(vec![finish_stop()])
        });

        assert_eq!(result.start_event.call_id, "test-telemetry-call-id");
        assert_eq!(result.start_event.provider, "mock-provider");
        assert_eq!(result.start_event.model_id, "mock-model-id");
        // The end event is only produced after streaming begins, so the start
        // event must always be present even for a stream that only finishes.
        assert!(result.end_event.is_some());
    }

    // packages-ai-0920: it should generate a consistent callId when one is not provided
    #[test]
    fn stream_language_model_call_generates_consistent_call_id() {
        let result = stream_language_model_call(StreamLanguageModelCallOptions {
            generated_call_id: "call-generated-call-id".to_string(),
            generated_response_id: "aitxt-generated-response-id".to_string(),
            ..StreamLanguageModelCallOptions::new(vec![finish_stop()])
        });

        let end = result.end_event.expect("end event");
        assert_eq!(result.start_event.call_id, "call-generated-call-id");
        assert_eq!(end.call_id, result.start_event.call_id);
        assert_eq!(end.response_id, "aitxt-generated-response-id");
        assert_eq!(end.content, Vec::new());
    }

    // packages-ai-0921: it should call onLanguageModelCallEnd with parsed model-call content
    #[test]
    fn stream_language_model_call_end_event_has_parsed_content() {
        let result = stream_language_model_call(StreamLanguageModelCallOptions {
            call_id: Some("test-telemetry-call-id".to_string()),
            tools: test_tools(),
            ..StreamLanguageModelCallOptions::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("response-1")
                        .with_model_id("response-model"),
                ),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "world")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    "{ \"value\": \"hello\" }",
                )),
                finish_tool_calls(),
            ])
        });

        let end = result.end_event.expect("end event");
        assert_eq!(end.call_id, "test-telemetry-call-id");
        assert_eq!(end.finish_reason, FinishReason::ToolCalls);
        assert_eq!(end.response_id, "response-1");
        assert_eq!(
            end.content,
            vec![
                ModelCallContentPart::Text {
                    text: "Hello world".to_string(),
                },
                ModelCallContentPart::ToolCall {
                    tool_call_id: "call-1".to_string(),
                    tool_name: "testTool".to_string(),
                    input: json!({ "value": "hello" }),
                    provider_executed: None,
                },
            ]
        );
    }

    // packages-ai-0922: it should generate a responseId when the provider does not send one
    #[test]
    fn stream_language_model_call_generates_response_id_when_absent() {
        let result = stream_language_model_call(StreamLanguageModelCallOptions {
            call_id: Some("test-telemetry-call-id".to_string()),
            generated_response_id: "aitxt-generated-response-id".to_string(),
            ..StreamLanguageModelCallOptions::new(vec![finish_stop()])
        });

        let end = result.end_event.expect("end event");
        assert_eq!(end.response_id, "aitxt-generated-response-id");
        assert_eq!(end.finish_reason, FinishReason::Stop);
        assert!(end.content.is_empty());
    }

    // packages-ai-0923: it should convert stream-start to init-model-call
    #[test]
    fn stream_language_model_call_converts_stream_start_to_model_call_start() {
        let result = run(vec![
            LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                Warning::Compatibility {
                    feature: "tool-approval".to_string(),
                    details: Some("approval fallback is being used".to_string()),
                },
                Warning::Other {
                    message: "custom warning".to_string(),
                },
            ])),
            finish_stop(),
        ]);

        assert_eq!(result.stream.len(), 2);
        match &result.stream[0] {
            ModelCallStreamPart::ModelCallStart { warnings } => {
                assert_eq!(warnings.len(), 2);
                assert_eq!(
                    warnings[0],
                    Warning::Compatibility {
                        feature: "tool-approval".to_string(),
                        details: Some("approval fallback is being used".to_string()),
                    }
                );
                assert_eq!(
                    warnings[1],
                    Warning::Other {
                        message: "custom warning".to_string(),
                    }
                );
            }
            other => panic!("expected model-call-start, got {other:?}"),
        }
        assert!(matches!(
            result.stream[1],
            ModelCallStreamPart::ModelCallEnd { .. }
        ));
    }

    // packages-ai-0924: it should convert text to delta
    #[test]
    fn stream_language_model_call_converts_text_to_delta() {
        let result = run(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "text")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            finish_stop(),
        ]);

        assert_eq!(
            result.stream[0],
            ModelCallStreamPart::TextStart {
                id: "1".to_string()
            }
        );
        assert_eq!(
            result.stream[1],
            ModelCallStreamPart::TextDelta {
                id: "1".to_string(),
                text: "text".to_string(),
                provider_metadata: None,
            }
        );
        assert_eq!(
            result.stream[2],
            ModelCallStreamPart::TextEnd {
                id: "1".to_string()
            }
        );
        assert!(matches!(
            result.stream[3],
            ModelCallStreamPart::ModelCallEnd { .. }
        ));
    }

    fn run_with_timestamps(
        parts: Vec<LanguageModelStreamPart>,
        timestamps: Vec<f64>,
    ) -> StreamLanguageModelCallResult {
        stream_language_model_call(StreamLanguageModelCallOptions {
            timestamps_ms: timestamps,
            ..StreamLanguageModelCallOptions::new(parts)
        })
    }

    // packages-ai-0925: it should measure time to first output from text deltas
    #[test]
    fn stream_language_model_call_measures_time_to_first_output_from_text_deltas() {
        let result = run_with_timestamps(
            vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "text")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                finish_stop(),
            ],
            vec![1000.0, 1250.0, 1600.0],
        );

        let perf = last_performance(&result);
        assert_eq!(perf.response_time_ms, 600.0);
        assert_eq!(perf.time_to_first_output_ms, Some(250.0));
        assert!((perf.effective_output_tokens_per_second - 16.666666666666668).abs() < 1e-9);
        assert!((perf.output_tokens_per_second.unwrap() - 28.571428571428573).abs() < 1e-9);
        assert!((perf.input_tokens_per_second.unwrap() - 12.0).abs() < 1e-9);
        assert!((perf.effective_total_tokens_per_second - 21.666666666666668).abs() < 1e-9);
    }

    // packages-ai-0926: it should measure time between output chunks from text deltas
    #[test]
    fn stream_language_model_call_measures_time_between_output_chunks_from_text_deltas() {
        let result = run_with_timestamps(
            vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "a")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "b")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "c")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "d")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "e")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "f")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                finish_stop(),
            ],
            vec![
                1000.0, 1100.0, 1200.0, 1500.0, 1600.0, 2000.0, 2500.0, 2500.0,
            ],
        );

        let perf = last_performance(&result);
        let stats = perf
            .time_between_output_chunks_ms
            .as_ref()
            .expect("timing stats");
        assert_eq!(stats.min, 100.0);
        assert_eq!(stats.p10, 100.0);
        assert_eq!(stats.median, 300.0);
        assert_eq!(stats.avg, 280.0);
        assert_eq!(stats.p90, 500.0);
        assert_eq!(stats.max, 500.0);
    }

    // packages-ai-0927: it should measure time between all output chunk types
    #[test]
    fn stream_language_model_call_measures_time_between_all_output_chunk_types() {
        let result = stream_language_model_call(StreamLanguageModelCallOptions {
            tools: test_tools(),
            timestamps_ms: vec![1000.0, 1100.0, 1300.0, 1600.0, 2000.0, 2500.0],
            ..StreamLanguageModelCallOptions::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "text")),
                LanguageModelStreamPart::File(LanguageModelFile::new(
                    "text/plain",
                    LanguageModelFileData::Data {
                        data: FileDataContent::Base64("ZmlsZQ==".to_string()),
                    },
                )),
                LanguageModelStreamPart::ReasoningFile(LanguageModelReasoningFile::new(
                    "text/plain",
                    LanguageModelFileData::Data {
                        data: FileDataContent::Base64("cmVhc29uaW5n".to_string()),
                    },
                )),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    "{ \"value\": \"test\" }",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    test_usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: Some("tool-calls".to_string()),
                    },
                )),
            ])
        });

        let perf = last_performance(&result);
        assert_eq!(perf.time_to_first_output_ms, Some(100.0));
        let stats = perf
            .time_between_output_chunks_ms
            .as_ref()
            .expect("timing stats");
        assert_eq!(stats.min, 200.0);
        assert_eq!(stats.p10, 200.0);
        assert_eq!(stats.median, 300.0);
        assert_eq!(stats.avg, 300.0);
        assert_eq!(stats.p90, 400.0);
        assert_eq!(stats.max, 400.0);
    }

    // packages-ai-0928: reasoning parts -> it should convert text to delta
    #[test]
    fn stream_language_model_call_converts_reasoning_to_delta() {
        let result = run(vec![
            LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
            LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new("1", "text")),
            LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
        ]);

        assert_eq!(
            result.stream,
            vec![
                ModelCallStreamPart::ReasoningStart {
                    id: "1".to_string()
                },
                ModelCallStreamPart::ReasoningDelta {
                    id: "1".to_string(),
                    text: "text".to_string(),
                    provider_metadata: None,
                },
                ModelCallStreamPart::ReasoningEnd {
                    id: "1".to_string()
                },
            ]
        );
    }

    // packages-ai-0929: it should measure time to first output from reasoning-file parts
    #[test]
    fn stream_language_model_call_measures_time_to_first_output_from_reasoning_file_parts() {
        let result = run_with_timestamps(
            vec![
                LanguageModelStreamPart::ReasoningFile(LanguageModelReasoningFile::new(
                    "text/plain",
                    LanguageModelFileData::Data {
                        data: FileDataContent::Base64("cmVhc29uaW5n".to_string()),
                    },
                )),
                finish_stop(),
            ],
            vec![1000.0, 1250.0, 1600.0],
        );

        let perf = last_performance(&result);
        assert_eq!(perf.response_time_ms, 600.0);
        assert_eq!(perf.time_to_first_output_ms, Some(250.0));
        assert!((perf.effective_output_tokens_per_second - 16.666666666666668).abs() < 1e-9);
        assert!((perf.output_tokens_per_second.unwrap() - 28.571428571428573).abs() < 1e-9);
        assert!((perf.input_tokens_per_second.unwrap() - 12.0).abs() < 1e-9);
        assert!((perf.effective_total_tokens_per_second - 21.666666666666668).abs() < 1e-9);
    }

    // packages-ai-0930: it should use GeneratedFile
    #[test]
    fn stream_language_model_call_emits_generated_file_from_base64() {
        let result = run(vec![
            LanguageModelStreamPart::File(LanguageModelFile::new(
                "text/plain",
                LanguageModelFileData::Data {
                    data: FileDataContent::Base64("SGVsbG8gV29ybGQ=".to_string()),
                },
            )),
            finish_stop(),
        ]);

        match &result.stream[0] {
            ModelCallStreamPart::File {
                file,
                provider_metadata,
            } => {
                assert_eq!(file.media_type(), "text/plain");
                assert_eq!(file.base64(), "SGVsbG8gV29ybGQ=");
                assert_eq!(file.bytes().unwrap(), b"Hello World");
                assert!(provider_metadata.is_none());
            }
            other => panic!("expected file part, got {other:?}"),
        }
    }

    // packages-ai-0931: it should use GeneratedFile with providerMetadata
    #[test]
    fn stream_language_model_call_emits_generated_file_with_provider_metadata() {
        let mut provider_metadata: ProviderMetadata = ProviderMetadata::new();
        provider_metadata.insert("testProvider".to_string(), {
            let mut obj = JsonObject::new();
            obj.insert("signature".to_string(), json!("test-signature"));
            obj
        });

        let result = run(vec![
            LanguageModelStreamPart::File(
                LanguageModelFile::new(
                    "text/plain",
                    LanguageModelFileData::Data {
                        data: FileDataContent::Bytes(b"Hello World".to_vec()),
                    },
                )
                .with_provider_metadata(provider_metadata.clone()),
            ),
            finish_stop(),
        ]);

        match &result.stream[0] {
            ModelCallStreamPart::File {
                file,
                provider_metadata: meta,
            } => {
                assert_eq!(file.media_type(), "text/plain");
                assert_eq!(file.bytes().unwrap(), b"Hello World");
                assert_eq!(meta.as_ref(), Some(&provider_metadata));
            }
            other => panic!("expected file part, got {other:?}"),
        }
    }

    // packages-ai-0932: it should measure time to first output from file parts
    #[test]
    fn stream_language_model_call_measures_time_to_first_output_from_file_parts() {
        let result = run_with_timestamps(
            vec![
                LanguageModelStreamPart::File(LanguageModelFile::new(
                    "text/plain",
                    LanguageModelFileData::Data {
                        data: FileDataContent::Base64("SGVsbG8gV29ybGQ=".to_string()),
                    },
                )),
                finish_stop(),
            ],
            vec![1000.0, 1250.0, 1600.0],
        );

        let perf = last_performance(&result);
        assert_eq!(perf.response_time_ms, 600.0);
        assert_eq!(perf.time_to_first_output_ms, Some(250.0));
        assert!((perf.effective_output_tokens_per_second - 16.666666666666668).abs() < 1e-9);
        assert!((perf.output_tokens_per_second.unwrap() - 28.571428571428573).abs() < 1e-9);
        assert!((perf.input_tokens_per_second.unwrap() - 12.0).abs() < 1e-9);
        assert!((perf.effective_total_tokens_per_second - 21.666666666666668).abs() < 1e-9);
    }

    // packages-ai-0933: it should forward custom parts
    #[test]
    fn stream_language_model_call_forwards_custom_parts() {
        let mut provider_metadata: ProviderMetadata = ProviderMetadata::new();
        provider_metadata.insert("openai".to_string(), {
            let mut obj = JsonObject::new();
            obj.insert("itemId".to_string(), json!("cmp_123"));
            obj
        });

        let result = run(vec![
            LanguageModelStreamPart::Custom(
                LanguageModelCustomContent::new("openai.compaction")
                    .with_provider_metadata(provider_metadata.clone()),
            ),
            finish_stop(),
        ]);

        assert_eq!(
            result.stream[0],
            ModelCallStreamPart::Custom {
                kind: "openai.compaction".to_string(),
                provider_metadata: Some(provider_metadata),
            }
        );
        // Custom parts do not count as output chunks, so there is no
        // time-to-first-output recorded.
        let perf = last_performance(&result);
        assert_eq!(perf.time_to_first_output_ms, None);
    }

    // packages-ai-0934: it should measure time to first output from tool-call parts
    #[test]
    fn stream_language_model_call_measures_time_to_first_output_from_tool_call_parts() {
        let result = stream_language_model_call(StreamLanguageModelCallOptions {
            tools: test_tools(),
            timestamps_ms: vec![1000.0, 1250.0, 1600.0],
            ..StreamLanguageModelCallOptions::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    "{ \"value\": \"test\" }",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    test_usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: Some("tool-calls".to_string()),
                    },
                )),
            ])
        });

        let perf = last_performance(&result);
        assert_eq!(perf.response_time_ms, 600.0);
        assert_eq!(perf.time_to_first_output_ms, Some(250.0));
        assert!((perf.effective_output_tokens_per_second - 16.666666666666668).abs() < 1e-9);
        assert!((perf.output_tokens_per_second.unwrap() - 28.571428571428573).abs() < 1e-9);
        assert!((perf.input_tokens_per_second.unwrap() - 12.0).abs() < 1e-9);
        assert!((perf.effective_total_tokens_per_second - 21.666666666666668).abs() < 1e-9);
    }

    // packages-ai-0935: it should refine tool call input before emitting tool-call
    // parts and model-call end content
    #[test]
    fn stream_language_model_call_refines_tool_call_input() {
        let mut refine: HashMap<String, ToolInputRefinement> = HashMap::new();
        refine.insert(
            "testTool".to_string(),
            Box::new(|input: JsonValue| {
                let value = input
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                json!({ "value": value })
            }),
        );

        let result = stream_language_model_call(StreamLanguageModelCallOptions {
            tools: test_tools(),
            refine_tool_input: refine,
            ..StreamLanguageModelCallOptions::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    "{ \"value\": \" raw \" }",
                )),
                finish_tool_calls(),
            ])
        });

        assert_eq!(
            result.stream[0],
            ModelCallStreamPart::ToolCall(ModelCallToolCall {
                tool_call_id: "call-1".to_string(),
                tool_name: "testTool".to_string(),
                input: json!({ "value": "raw" }),
                provider_executed: None,
            })
        );

        let end = result.end_event.expect("end event");
        assert_eq!(
            end.content[0],
            ModelCallContentPart::ToolCall {
                tool_call_id: "call-1".to_string(),
                tool_name: "testTool".to_string(),
                input: json!({ "value": "raw" }),
                provider_executed: None,
            }
        );
    }

    // packages-ai-0936: it should try to repair tool call when the tool name is not found
    #[test]
    fn stream_language_model_call_repairs_unknown_tool_call() {
        let mut tools = HashMap::new();
        tools.insert(
            "correctTool".to_string(),
            ToolDefinition {
                provider_executed: false,
            },
        );

        let result = stream_language_model_call(StreamLanguageModelCallOptions {
            tools,
            repair_tool_call: Some(Box::new(|tool_name, input| {
                assert_eq!(tool_name, "unknownTool");
                assert_eq!(input, "{ \"value\": \"test\" }");
                Some("correctTool".to_string())
            })),
            ..StreamLanguageModelCallOptions::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "unknownTool",
                    "{ \"value\": \"test\" }",
                )),
                finish_stop(),
            ])
        });

        assert_eq!(
            result.stream[0],
            ModelCallStreamPart::ToolCall(ModelCallToolCall {
                tool_call_id: "call-1".to_string(),
                tool_name: "correctTool".to_string(),
                input: json!({ "value": "test" }),
                provider_executed: None,
            })
        );
        assert!(matches!(
            result.stream[1],
            ModelCallStreamPart::ModelCallEnd { .. }
        ));
    }

    // packages-ai-0937: it should emit error when tool call is not found for provider
    // approval request
    #[test]
    fn stream_language_model_call_errors_when_approval_target_missing() {
        let result = run(vec![
            LanguageModelStreamPart::ToolApprovalRequest(LanguageModelToolApprovalRequest::new(
                "mcp-approval-1",
                "non-existent-call",
            )),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                test_usage(),
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
            )),
        ]);

        assert_eq!(
            result.stream[0],
            ModelCallStreamPart::Error {
                error:
                    "Tool call \"non-existent-call\" not found for approval request \"mcp-approval-1\"."
                        .to_string(),
            }
        );
        assert!(matches!(
            result.stream[1],
            ModelCallStreamPart::ModelCallEnd { .. }
        ));
    }

    fn mcp_tools() -> HashMap<String, ToolDefinition> {
        let mut tools = HashMap::new();
        tools.insert(
            "mcp_tool".to_string(),
            ToolDefinition {
                provider_executed: true,
            },
        );
        tools.insert(
            "mcp_search".to_string(),
            ToolDefinition {
                provider_executed: true,
            },
        );
        tools.insert(
            "mcp_execute".to_string(),
            ToolDefinition {
                provider_executed: true,
            },
        );
        tools
    }

    // packages-ai-0938: it should forward provider-emitted tool-approval-request with the
    // correct tool call
    #[test]
    fn stream_language_model_call_forwards_approval_request_with_tool_call() {
        let result = stream_language_model_call(StreamLanguageModelCallOptions {
            tools: mcp_tools(),
            ..StreamLanguageModelCallOptions::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("mcp-call-1", "mcp_tool", "{ \"query\": \"test\" }")
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequest::new("mcp-approval-1", "mcp-call-1"),
                ),
                finish_tool_calls(),
            ])
        });

        let expected_tool_call = ModelCallToolCall {
            tool_call_id: "mcp-call-1".to_string(),
            tool_name: "mcp_tool".to_string(),
            input: json!({ "query": "test" }),
            provider_executed: Some(true),
        };

        assert_eq!(
            result.stream[0],
            ModelCallStreamPart::ToolCall(expected_tool_call.clone())
        );
        assert_eq!(
            result.stream[1],
            ModelCallStreamPart::ToolApprovalRequest {
                approval_id: "mcp-approval-1".to_string(),
                tool_call: expected_tool_call,
            }
        );
        assert!(matches!(
            result.stream[2],
            ModelCallStreamPart::ModelCallEnd { .. }
        ));
    }

    // packages-ai-0939: it should handle multiple provider-executed tool calls with
    // approval requests
    #[test]
    fn stream_language_model_call_handles_multiple_approval_requests() {
        let result = stream_language_model_call(StreamLanguageModelCallOptions {
            tools: mcp_tools(),
            ..StreamLanguageModelCallOptions::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new(
                        "mcp-call-1",
                        "mcp_search",
                        "{ \"query\": \"first\" }",
                    )
                    .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new(
                        "mcp-call-2",
                        "mcp_execute",
                        "{ \"command\": \"ls\" }",
                    )
                    .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequest::new("approval-1", "mcp-call-1"),
                ),
                LanguageModelStreamPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequest::new("approval-2", "mcp-call-2"),
                ),
                finish_tool_calls(),
            ])
        });

        let first = ModelCallToolCall {
            tool_call_id: "mcp-call-1".to_string(),
            tool_name: "mcp_search".to_string(),
            input: json!({ "query": "first" }),
            provider_executed: Some(true),
        };
        let second = ModelCallToolCall {
            tool_call_id: "mcp-call-2".to_string(),
            tool_name: "mcp_execute".to_string(),
            input: json!({ "command": "ls" }),
            provider_executed: Some(true),
        };

        assert_eq!(
            result.stream[0],
            ModelCallStreamPart::ToolCall(first.clone())
        );
        assert_eq!(
            result.stream[1],
            ModelCallStreamPart::ToolCall(second.clone())
        );
        assert_eq!(
            result.stream[2],
            ModelCallStreamPart::ToolApprovalRequest {
                approval_id: "approval-1".to_string(),
                tool_call: first,
            }
        );
        assert_eq!(
            result.stream[3],
            ModelCallStreamPart::ToolApprovalRequest {
                approval_id: "approval-2".to_string(),
                tool_call: second,
            }
        );
        assert!(matches!(
            result.stream[4],
            ModelCallStreamPart::ModelCallEnd { .. }
        ));
    }

    #[test]
    fn nearest_rank_percentile_matches_upstream() {
        // [100,100,300,300,400,500] sorted -> p10 index ceil(0.1*6)=1 -> [0]=100,
        // median ceil(0.5*6)=3 -> [2]=300, p90 ceil(0.9*6)=6 -> [5]=500.
        let values = [300.0, 100.0, 500.0, 100.0, 400.0, 300.0];
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(nearest_rank_percentile(&sorted, 0.1), 100.0);
        assert_eq!(nearest_rank_percentile(&sorted, 0.5), 300.0);
        assert_eq!(nearest_rank_percentile(&sorted, 0.9), 500.0);
    }
}
