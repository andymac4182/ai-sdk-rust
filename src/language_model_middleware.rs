use std::collections::BTreeMap;
use std::future::{Future, Pending, Ready, ready};
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    LanguageModel, LanguageModelCallOptions, LanguageModelContent, LanguageModelGenerateResult,
    LanguageModelReasoning, LanguageModelReasoningDelta, LanguageModelReasoningEnd,
    LanguageModelReasoningStart, LanguageModelResponse, LanguageModelResponseFormat,
    LanguageModelStreamFinish, LanguageModelStreamPart, LanguageModelStreamResponseMetadata,
    LanguageModelStreamResult, LanguageModelStreamResultResponse, LanguageModelStreamStart,
    LanguageModelSupportedUrls, LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd,
    LanguageModelTextStart, LanguageModelTool, LanguageModelToolChoice,
    LanguageModelToolInputExample,
};
use crate::provider::{ProviderOptions, SpecificationVersion};

/// Original language generation operation passed to middleware wrappers.
pub type LanguageModelDoGenerate<'a> = Box<
    dyn FnOnce() -> Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
        + Send
        + 'a,
>;

/// Original language streaming operation passed to middleware wrappers.
pub type LanguageModelDoStream<'a, S> = Box<
    dyn FnOnce() -> Pin<Box<dyn Future<Output = LanguageModelStreamResult<S>> + Send + 'a>>
        + Send
        + 'a,
>;

/// Language model operation whose parameters are being transformed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LanguageModelMiddlewareCallType {
    /// Non-streaming generation.
    Generate,

    /// Streaming generation.
    Stream,
}

/// Options passed to language middleware hooks that only inspect the model.
#[derive(Debug)]
pub struct LanguageModelMiddlewareModelOptions<'a, M: LanguageModel> {
    /// The language model being wrapped.
    pub model: &'a M,
}

impl<'a, M: LanguageModel> LanguageModelMiddlewareModelOptions<'a, M> {
    /// Creates model-only middleware hook options.
    pub fn new(model: &'a M) -> Self {
        Self { model }
    }
}

/// Options passed to language middleware parameter transforms.
#[derive(Debug)]
pub struct LanguageModelTransformParamsOptions<'a, M: LanguageModel> {
    /// Whether the transformed parameters will be used for generate or stream.
    pub call_type: LanguageModelMiddlewareCallType,

    /// Original language model call options.
    pub params: LanguageModelCallOptions,

    /// The language model being wrapped.
    pub model: &'a M,
}

impl<'a, M: LanguageModel> LanguageModelTransformParamsOptions<'a, M> {
    /// Creates transform-params middleware hook options.
    pub fn new(
        call_type: LanguageModelMiddlewareCallType,
        params: LanguageModelCallOptions,
        model: &'a M,
    ) -> Self {
        Self {
            call_type,
            params,
            model,
        }
    }
}

/// Options passed to language generation middleware wrappers.
pub struct LanguageModelWrapGenerateOptions<'a, M: LanguageModel> {
    /// Original language generation operation.
    pub do_generate: LanguageModelDoGenerate<'a>,

    /// Original language streaming operation.
    pub do_stream: LanguageModelDoStream<'a, M::Stream>,

    /// Language model call options, transformed if a transform hook ran first.
    pub params: LanguageModelCallOptions,

    /// The language model being wrapped.
    pub model: &'a M,
}

impl<'a, M: LanguageModel> LanguageModelWrapGenerateOptions<'a, M> {
    /// Creates wrap-generate middleware hook options.
    pub fn new(
        do_generate: LanguageModelDoGenerate<'a>,
        do_stream: LanguageModelDoStream<'a, M::Stream>,
        params: LanguageModelCallOptions,
        model: &'a M,
    ) -> Self {
        Self {
            do_generate,
            do_stream,
            params,
            model,
        }
    }
}

/// Options passed to language stream middleware wrappers.
pub struct LanguageModelWrapStreamOptions<'a, M: LanguageModel> {
    /// Original language generation operation.
    pub do_generate: LanguageModelDoGenerate<'a>,

    /// Original language streaming operation.
    pub do_stream: LanguageModelDoStream<'a, M::Stream>,

    /// Language model call options, transformed if a transform hook ran first.
    pub params: LanguageModelCallOptions,

    /// The language model being wrapped.
    pub model: &'a M,
}

impl<'a, M: LanguageModel> LanguageModelWrapStreamOptions<'a, M> {
    /// Creates wrap-stream middleware hook options.
    pub fn new(
        do_generate: LanguageModelDoGenerate<'a>,
        do_stream: LanguageModelDoStream<'a, M::Stream>,
        params: LanguageModelCallOptions,
        model: &'a M,
    ) -> Self {
        Self {
            do_generate,
            do_stream,
            params,
            model,
        }
    }
}

/// Middleware for provider-v4 language models.
///
/// Upstream `LanguageModelV4Middleware` exposes optional hooks for overriding
/// identity/supported-url values, transforming call options, and wrapping both
/// `doGenerate` and `doStream`. This Rust trait represents optional hooks as
/// methods that return `None` when the middleware does not handle that step.
pub trait LanguageModelMiddleware<M: LanguageModel> {
    /// Future returned by [`LanguageModelMiddleware::override_supported_urls`].
    type OverrideSupportedUrlsFuture<'a>: Future<Output = LanguageModelSupportedUrls> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`LanguageModelMiddleware::transform_params`].
    type TransformParamsFuture<'a>: Future<Output = LanguageModelCallOptions> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`LanguageModelMiddleware::wrap_generate`].
    type WrapGenerateFuture<'a>: Future<Output = LanguageModelGenerateResult> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`LanguageModelMiddleware::wrap_stream`].
    type WrapStreamFuture<'a>: Future<Output = LanguageModelStreamResult<M::Stream>> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Returns the middleware interface version.
    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    /// Optionally overrides the provider identifier reported by the model.
    fn override_provider(
        &self,
        options: LanguageModelMiddlewareModelOptions<'_, M>,
    ) -> Option<String> {
        let _ = options;
        None
    }

    /// Optionally overrides the provider-specific model id.
    fn override_model_id(
        &self,
        options: LanguageModelMiddlewareModelOptions<'_, M>,
    ) -> Option<String> {
        let _ = options;
        None
    }

    /// Optionally overrides supported URL patterns grouped by media type.
    fn override_supported_urls<'a>(
        &'a self,
        options: LanguageModelMiddlewareModelOptions<'a, M>,
    ) -> Option<Self::OverrideSupportedUrlsFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally transforms call options before invoking the model.
    fn transform_params<'a>(
        &'a self,
        options: LanguageModelTransformParamsOptions<'a, M>,
    ) -> Option<Self::TransformParamsFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally wraps the model's non-streaming generation operation.
    fn wrap_generate<'a>(
        &'a self,
        options: LanguageModelWrapGenerateOptions<'a, M>,
    ) -> Option<Self::WrapGenerateFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally wraps the model's streaming generation operation.
    fn wrap_stream<'a>(
        &'a self,
        options: LanguageModelWrapStreamOptions<'a, M>,
    ) -> Option<Self::WrapStreamFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }
}

/// Language model wrapper that applies one middleware around a provider-v4 model.
///
/// Upstream `wrapLanguageModel` accepts one or more middlewares. This Rust
/// wrapper models the same behavior for a single middleware without allocating
/// a middleware collection; callers can wrap the returned model again to
/// compose additional middleware.
#[derive(Clone, Debug)]
pub struct WrappedLanguageModel<M, W> {
    model: M,
    middleware: W,
    provider_id: String,
    model_id: String,
}

impl<M, W> WrappedLanguageModel<M, W>
where
    M: LanguageModel,
    W: LanguageModelMiddleware<M>,
{
    /// Creates a language model wrapper using middleware-provided identity
    /// overrides when present.
    pub fn new(model: M, middleware: W) -> Self {
        let provider_id = middleware
            .override_provider(LanguageModelMiddlewareModelOptions::new(&model))
            .unwrap_or_else(|| model.provider().to_string());
        let model_id = middleware
            .override_model_id(LanguageModelMiddlewareModelOptions::new(&model))
            .unwrap_or_else(|| model.model_id().to_string());

        Self {
            model,
            middleware,
            provider_id,
            model_id,
        }
    }

    /// Sets an explicit provider id, taking precedence over middleware identity
    /// overrides and the wrapped model's provider id.
    pub fn with_provider_id(mut self, provider_id: impl Into<String>) -> Self {
        self.provider_id = provider_id.into();
        self
    }

    /// Sets an explicit model id, taking precedence over middleware identity
    /// overrides and the wrapped model's model id.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Returns the wrapped base language model.
    pub fn model(&self) -> &M {
        &self.model
    }

    /// Returns the middleware applied by this wrapper.
    pub fn middleware(&self) -> &W {
        &self.middleware
    }

    /// Consumes the wrapper into the base model and middleware.
    pub fn into_parts(self) -> (M, W) {
        (self.model, self.middleware)
    }
}

/// Wraps a language model with middleware.
pub fn wrap_language_model<M, W>(model: M, middleware: W) -> WrappedLanguageModel<M, W>
where
    M: LanguageModel,
    W: LanguageModelMiddleware<M>,
{
    WrappedLanguageModel::new(model, middleware)
}

impl<M, W> LanguageModel for WrappedLanguageModel<M, W>
where
    M: LanguageModel + Sync,
    W: LanguageModelMiddleware<M> + Sync,
{
    type SupportedUrlsFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelSupportedUrls> + Send + 'a>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
    where
        Self: 'a;

    type Stream = M::Stream;

    type StreamFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelStreamResult<Self::Stream>> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.provider_id
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        Box::pin(async move {
            if let Some(supported_urls) = self
                .middleware
                .override_supported_urls(LanguageModelMiddlewareModelOptions::new(&self.model))
            {
                supported_urls.await
            } else {
                self.model.supported_urls().await
            }
        })
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(async move {
            let params = if let Some(transform_params) =
                self.middleware
                    .transform_params(LanguageModelTransformParamsOptions::new(
                        LanguageModelMiddlewareCallType::Generate,
                        options.clone(),
                        &self.model,
                    )) {
                transform_params.await
            } else {
                options
            };

            let do_generate_params = params.clone();
            let do_stream_params = params.clone();
            let fallback_params = params.clone();
            let model = &self.model;
            let do_generate: LanguageModelDoGenerate<'_> =
                Box::new(move || Box::pin(model.do_generate(do_generate_params)));
            let do_stream: LanguageModelDoStream<'_, M::Stream> =
                Box::new(move || Box::pin(model.do_stream(do_stream_params)));

            if let Some(wrap_generate) =
                self.middleware
                    .wrap_generate(LanguageModelWrapGenerateOptions::new(
                        do_generate,
                        do_stream,
                        params,
                        &self.model,
                    ))
            {
                wrap_generate.await
            } else {
                self.model.do_generate(fallback_params).await
            }
        })
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(async move {
            let params = if let Some(transform_params) =
                self.middleware
                    .transform_params(LanguageModelTransformParamsOptions::new(
                        LanguageModelMiddlewareCallType::Stream,
                        options.clone(),
                        &self.model,
                    )) {
                transform_params.await
            } else {
                options
            };

            let do_generate_params = params.clone();
            let do_stream_params = params.clone();
            let fallback_params = params.clone();
            let model = &self.model;
            let do_generate: LanguageModelDoGenerate<'_> =
                Box::new(move || Box::pin(model.do_generate(do_generate_params)));
            let do_stream: LanguageModelDoStream<'_, M::Stream> =
                Box::new(move || Box::pin(model.do_stream(do_stream_params)));

            if let Some(wrap_stream) =
                self.middleware
                    .wrap_stream(LanguageModelWrapStreamOptions::new(
                        do_generate,
                        do_stream,
                        params,
                        &self.model,
                    ))
            {
                wrap_stream.await
            } else {
                self.model.do_stream(fallback_params).await
            }
        })
    }
}

/// Default provider call settings applied by [`DefaultSettingsMiddleware`].
///
/// Upstream `defaultSettingsMiddleware` accepts a partial provider-v4 language
/// model call options object without `prompt`. Rust keeps that same boundary as
/// an explicit settings record and treats `None` as "no default".
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelDefaultSettings {
    /// Default maximum number of output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,

    /// Default sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Default stop sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Default nucleus sampling setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Default top-k sampling setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u64>,

    /// Default presence penalty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,

    /// Default frequency penalty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,

    /// Default response format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<LanguageModelResponseFormat>,

    /// Default deterministic sampling seed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,

    /// Default provider tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<LanguageModelTool>>,

    /// Default provider tool choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<LanguageModelToolChoice>,

    /// Default HTTP headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Default provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelDefaultSettings {
    /// Creates an empty default-settings record.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the default maximum number of output tokens.
    pub fn with_max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Sets the default sampling temperature.
    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Adds a default stop sequence.
    pub fn with_stop_sequence(mut self, stop_sequence: impl Into<String>) -> Self {
        self.stop_sequences
            .get_or_insert_with(Vec::new)
            .push(stop_sequence.into());
        self
    }

    /// Sets the default nucleus sampling value.
    pub fn with_top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Sets the default top-k sampling value.
    pub fn with_top_k(mut self, top_k: u64) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Sets the default presence penalty.
    pub fn with_presence_penalty(mut self, presence_penalty: f64) -> Self {
        self.presence_penalty = Some(presence_penalty);
        self
    }

    /// Sets the default frequency penalty.
    pub fn with_frequency_penalty(mut self, frequency_penalty: f64) -> Self {
        self.frequency_penalty = Some(frequency_penalty);
        self
    }

    /// Sets the default response format.
    pub fn with_response_format(mut self, response_format: LanguageModelResponseFormat) -> Self {
        self.response_format = Some(response_format);
        self
    }

    /// Sets the default deterministic sampling seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Adds a default provider tool.
    pub fn with_tool(mut self, tool: LanguageModelTool) -> Self {
        self.tools.get_or_insert_with(Vec::new).push(tool);
        self
    }

    /// Sets the default tool choice.
    pub fn with_tool_choice(mut self, tool_choice: LanguageModelToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Adds a default HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the default provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// Language model middleware that applies default call settings.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultSettingsMiddleware {
    /// Settings applied when callers leave the corresponding call option unset.
    pub settings: LanguageModelDefaultSettings,
}

impl DefaultSettingsMiddleware {
    /// Creates default-settings middleware from a settings record.
    pub fn new(settings: LanguageModelDefaultSettings) -> Self {
        Self { settings }
    }

    fn apply_settings(&self, mut params: LanguageModelCallOptions) -> LanguageModelCallOptions {
        if params.max_output_tokens.is_none() {
            params.max_output_tokens = self.settings.max_output_tokens;
        }

        if params.temperature.is_none() {
            params.temperature = self.settings.temperature;
        }

        if params.stop_sequences.is_none() {
            params.stop_sequences = self.settings.stop_sequences.clone();
        }

        if params.top_p.is_none() {
            params.top_p = self.settings.top_p;
        }

        if params.top_k.is_none() {
            params.top_k = self.settings.top_k;
        }

        if params.presence_penalty.is_none() {
            params.presence_penalty = self.settings.presence_penalty;
        }

        if params.frequency_penalty.is_none() {
            params.frequency_penalty = self.settings.frequency_penalty;
        }

        if params.response_format.is_none() {
            params.response_format = self.settings.response_format.clone();
        }

        if params.seed.is_none() {
            params.seed = self.settings.seed;
        }

        if params.tools.is_none() {
            params.tools = self.settings.tools.clone();
        }

        if params.tool_choice.is_none() {
            params.tool_choice = self.settings.tool_choice.clone();
        }

        params.headers = merge_headers(self.settings.headers.as_ref(), params.headers);
        params.provider_options = merge_provider_options_deep(
            self.settings.provider_options.as_ref(),
            params.provider_options,
        );

        params
    }
}

/// Creates language model middleware that applies default call settings.
pub fn default_settings_middleware(
    settings: LanguageModelDefaultSettings,
) -> DefaultSettingsMiddleware {
    DefaultSettingsMiddleware::new(settings)
}

/// Formats a tool input example for [`AddToolInputExamplesMiddleware`].
pub type ToolInputExampleFormatFunction = fn(&LanguageModelToolInputExample, usize) -> String;

/// Formats a tool input example using the upstream default JSON representation.
pub fn default_format_tool_input_example(
    example: &LanguageModelToolInputExample,
    _index: usize,
) -> String {
    serde_json::to_string(&example.input).expect("JSON object examples serialize")
}

/// Language model middleware that appends tool input examples to descriptions.
///
/// Upstream `addToolInputExamplesMiddleware` is useful for providers that do
/// not natively support `inputExamples`. This Rust port serializes function
/// tool examples into the description and removes the structured examples by
/// default, matching upstream behavior.
#[derive(Clone, Debug)]
pub struct AddToolInputExamplesMiddleware {
    prefix: String,
    remove: bool,
    format: ToolInputExampleFormatFunction,
}

impl AddToolInputExamplesMiddleware {
    /// Creates middleware with the upstream default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the prefix inserted before formatted examples.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Sets whether structured `inputExamples` are removed after appending.
    pub fn with_remove(mut self, remove: bool) -> Self {
        self.remove = remove;
        self
    }

    /// Sets a custom formatter for each example.
    pub fn with_format(mut self, format: ToolInputExampleFormatFunction) -> Self {
        self.format = format;
        self
    }

    /// Returns the prefix inserted before formatted examples.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Returns whether structured input examples are removed after appending.
    pub fn remove(&self) -> bool {
        self.remove
    }

    fn apply_examples(&self, mut params: LanguageModelCallOptions) -> LanguageModelCallOptions {
        let Some(tools) = params.tools.take() else {
            return params;
        };

        params.tools = Some(
            tools
                .into_iter()
                .map(|tool| self.transform_tool(tool))
                .collect(),
        );
        params
    }

    fn transform_tool(&self, tool: LanguageModelTool) -> LanguageModelTool {
        let mut function_tool = match tool {
            LanguageModelTool::Function(function_tool) => function_tool,
            other => return other,
        };

        let Some(input_examples) = function_tool.input_examples.as_ref() else {
            return LanguageModelTool::Function(function_tool);
        };

        if input_examples.is_empty() {
            return LanguageModelTool::Function(function_tool);
        }

        let formatted_examples = input_examples
            .iter()
            .enumerate()
            .map(|(index, example)| (self.format)(example, index))
            .collect::<Vec<_>>()
            .join("\n");
        let examples_section = format!("{}\n{formatted_examples}", self.prefix);

        function_tool.description = Some(match function_tool.description.take() {
            Some(description) => format!("{description}\n\n{examples_section}"),
            None => examples_section,
        });

        if self.remove {
            function_tool.input_examples = None;
        }

        LanguageModelTool::Function(function_tool)
    }
}

impl Default for AddToolInputExamplesMiddleware {
    fn default() -> Self {
        Self {
            prefix: "Input Examples:".to_string(),
            remove: true,
            format: default_format_tool_input_example,
        }
    }
}

/// Creates language model middleware that appends input examples to tool descriptions.
pub fn add_tool_input_examples_middleware() -> AddToolInputExamplesMiddleware {
    AddToolInputExamplesMiddleware::new()
}

/// Transforms text for [`ExtractJsonMiddleware`].
pub type ExtractJsonTransformFunction = fn(&str) -> String;

/// Removes common Markdown JSON code fences from generated text.
pub fn default_extract_json_transform(text: &str) -> String {
    let mut value = text.trim();

    if let Some(rest) = value.strip_prefix("```json") {
        value = rest.trim_start();
    } else if let Some(rest) = value.strip_prefix("```") {
        value = rest.trim_start();
    }

    value = value.trim_end();

    if let Some(rest) = value.strip_suffix("```") {
        value = rest.trim_end();
    }

    value.trim().to_string()
}

/// Language model middleware that extracts JSON from text content.
///
/// Upstream `extractJsonMiddleware` strips Markdown JSON fences before object
/// parsing. This Rust port applies the same default transform to non-streaming
/// text parts and to collected `Vec<LanguageModelStreamPart>` text blocks.
#[derive(Clone, Debug)]
pub struct ExtractJsonMiddleware {
    transform: ExtractJsonTransformFunction,
}

impl ExtractJsonMiddleware {
    /// Creates middleware with the upstream default transform.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a custom text transform.
    pub fn with_transform(mut self, transform: ExtractJsonTransformFunction) -> Self {
        self.transform = transform;
        self
    }

    fn transform_content(&self, content: LanguageModelContent) -> LanguageModelContent {
        match content {
            LanguageModelContent::Text(mut text) => {
                text.text = (self.transform)(&text.text);
                LanguageModelContent::Text(text)
            }
            other => other,
        }
    }

    fn transform_stream(
        &self,
        stream: Vec<LanguageModelStreamPart>,
    ) -> Vec<LanguageModelStreamPart> {
        let mut transformed = Vec::with_capacity(stream.len());
        let mut text_starts: BTreeMap<String, LanguageModelTextStart> = BTreeMap::new();
        let mut text_buffers: BTreeMap<String, String> = BTreeMap::new();

        for part in stream {
            match part {
                LanguageModelStreamPart::TextStart(start) => {
                    text_buffers.insert(start.id.clone(), String::new());
                    text_starts.insert(start.id.clone(), start);
                }
                LanguageModelStreamPart::TextDelta(delta) => {
                    if let Some(buffer) = text_buffers.get_mut(&delta.id) {
                        buffer.push_str(&delta.delta);
                    } else {
                        transformed.push(LanguageModelStreamPart::TextDelta(delta));
                    }
                }
                LanguageModelStreamPart::TextEnd(end) => {
                    if let Some(start) = text_starts.remove(&end.id) {
                        transformed.push(LanguageModelStreamPart::TextStart(start));
                        let text = text_buffers.remove(&end.id).unwrap_or_default();
                        let text = (self.transform)(&text);

                        if !text.is_empty() {
                            transformed.push(LanguageModelStreamPart::TextDelta(
                                LanguageModelTextDelta::new(end.id.clone(), text),
                            ));
                        }

                        transformed.push(LanguageModelStreamPart::TextEnd(end));
                    } else {
                        transformed.push(LanguageModelStreamPart::TextEnd(end));
                    }
                }
                other => transformed.push(other),
            }
        }

        for (_, start) in text_starts {
            let text = text_buffers.remove(&start.id).unwrap_or_default();
            transformed.push(LanguageModelStreamPart::TextStart(start.clone()));

            let text = (self.transform)(&text);
            if !text.is_empty() {
                transformed.push(LanguageModelStreamPart::TextDelta(
                    LanguageModelTextDelta::new(start.id, text),
                ));
            }
        }

        transformed
    }
}

impl Default for ExtractJsonMiddleware {
    fn default() -> Self {
        Self {
            transform: default_extract_json_transform,
        }
    }
}

/// Creates language model middleware that strips JSON formatting from text.
pub fn extract_json_middleware() -> ExtractJsonMiddleware {
    ExtractJsonMiddleware::new()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReasoningTextMatch {
    start: usize,
    end: usize,
    text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReasoningExtractionState {
    is_first_reasoning: bool,
    is_first_text: bool,
    after_switch: bool,
    is_reasoning: bool,
    buffer: String,
    id_counter: usize,
    text_id: String,
}

impl ReasoningExtractionState {
    fn new(text_id: String, start_with_reasoning: bool) -> Self {
        Self {
            is_first_reasoning: true,
            is_first_text: true,
            after_switch: false,
            is_reasoning: start_with_reasoning,
            buffer: String::new(),
            id_counter: 0,
            text_id,
        }
    }

    fn reasoning_id(&self) -> String {
        format!("reasoning-{}", self.id_counter)
    }
}

/// Language model middleware that extracts XML-tagged reasoning from text.
///
/// Upstream `extractReasoningMiddleware` converts text between configured XML
/// tags into reasoning content while leaving the remaining text as normal text.
/// This Rust port applies the same behavior to non-streaming content and to the
/// crate's deterministic `Vec<LanguageModelStreamPart>` stream boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractReasoningMiddleware {
    tag_name: String,
    separator: String,
    start_with_reasoning: bool,
}

impl ExtractReasoningMiddleware {
    /// Creates middleware that extracts reasoning from `<tag_name>...</tag_name>`.
    pub fn new(tag_name: impl Into<String>) -> Self {
        Self {
            tag_name: tag_name.into(),
            separator: "\n".to_string(),
            start_with_reasoning: false,
        }
    }

    /// Sets the separator inserted between extracted reasoning or text sections.
    pub fn with_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Treats the first text delta/content as already inside a reasoning block.
    pub fn with_start_with_reasoning(mut self, start_with_reasoning: bool) -> Self {
        self.start_with_reasoning = start_with_reasoning;
        self
    }

    /// Returns the configured reasoning tag name.
    pub fn tag_name(&self) -> &str {
        &self.tag_name
    }

    /// Returns the configured section separator.
    pub fn separator(&self) -> &str {
        &self.separator
    }

    /// Returns whether generation starts inside a reasoning block.
    pub fn start_with_reasoning(&self) -> bool {
        self.start_with_reasoning
    }

    fn opening_tag(&self) -> String {
        format!("<{}>", self.tag_name)
    }

    fn closing_tag(&self) -> String {
        format!("</{}>", self.tag_name)
    }

    fn transform_content(&self, content: LanguageModelContent) -> Vec<LanguageModelContent> {
        match content {
            LanguageModelContent::Text(text) => {
                let opening_tag = self.opening_tag();
                let closing_tag = self.closing_tag();
                let source_text = if self.start_with_reasoning {
                    format!("{opening_tag}{}", text.text)
                } else {
                    text.text.clone()
                };
                let matches = extract_reasoning_matches(&source_text, &opening_tag, &closing_tag);

                if matches.is_empty() {
                    return vec![LanguageModelContent::Text(text)];
                }

                let reasoning_text = matches
                    .iter()
                    .map(|reasoning_match| reasoning_match.text.as_str())
                    .collect::<Vec<_>>()
                    .join(&self.separator);
                let text_without_reasoning =
                    remove_reasoning_matches(&source_text, &matches, &self.separator);

                vec![
                    LanguageModelContent::Reasoning(LanguageModelReasoning::new(reasoning_text)),
                    LanguageModelContent::Text(LanguageModelText::new(text_without_reasoning)),
                ]
            }
            other => vec![other],
        }
    }

    fn transform_stream(
        &self,
        stream: Vec<LanguageModelStreamPart>,
    ) -> Vec<LanguageModelStreamPart> {
        let opening_tag = self.opening_tag();
        let closing_tag = self.closing_tag();
        let mut transformed = Vec::with_capacity(stream.len());
        let mut reasoning_extractions: BTreeMap<String, ReasoningExtractionState> = BTreeMap::new();
        let mut delayed_text_start: Option<LanguageModelTextStart> = None;

        for part in stream {
            match part {
                LanguageModelStreamPart::TextStart(start) => {
                    delayed_text_start = Some(start);
                }
                LanguageModelStreamPart::TextEnd(end) => {
                    if let Some(start) = delayed_text_start.take() {
                        transformed.push(LanguageModelStreamPart::TextStart(start));
                    }
                    transformed.push(LanguageModelStreamPart::TextEnd(end));
                }
                LanguageModelStreamPart::TextDelta(delta) => {
                    let active_extraction = reasoning_extractions
                        .entry(delta.id.clone())
                        .or_insert_with(|| {
                            ReasoningExtractionState::new(
                                delta.id.clone(),
                                self.start_with_reasoning,
                            )
                        });
                    active_extraction.buffer.push_str(&delta.delta);

                    loop {
                        let next_tag = if active_extraction.is_reasoning {
                            &closing_tag
                        } else {
                            &opening_tag
                        };

                        let Some(start_index) =
                            get_potential_start_index(&active_extraction.buffer, next_tag)
                        else {
                            let text = active_extraction.buffer.clone();
                            publish_reasoning_extraction_text(
                                &mut transformed,
                                &mut delayed_text_start,
                                active_extraction,
                                &self.separator,
                                &text,
                            );
                            active_extraction.buffer.clear();
                            break;
                        };

                        let text = active_extraction.buffer[..start_index].to_string();
                        publish_reasoning_extraction_text(
                            &mut transformed,
                            &mut delayed_text_start,
                            active_extraction,
                            &self.separator,
                            &text,
                        );

                        let found_full_match =
                            start_index + next_tag.len() <= active_extraction.buffer.len();

                        if found_full_match {
                            active_extraction.buffer = active_extraction.buffer
                                [start_index + next_tag.len()..]
                                .to_string();

                            if active_extraction.is_reasoning {
                                if active_extraction.is_first_reasoning {
                                    transformed.push(LanguageModelStreamPart::ReasoningStart(
                                        LanguageModelReasoningStart::new(
                                            active_extraction.reasoning_id(),
                                        ),
                                    ));
                                }

                                transformed.push(LanguageModelStreamPart::ReasoningEnd(
                                    LanguageModelReasoningEnd::new(
                                        active_extraction.reasoning_id(),
                                    ),
                                ));
                                active_extraction.id_counter += 1;
                            }

                            active_extraction.is_reasoning = !active_extraction.is_reasoning;
                            active_extraction.after_switch = true;
                        } else {
                            active_extraction.buffer =
                                active_extraction.buffer[start_index..].to_string();
                            break;
                        }
                    }
                }
                other => transformed.push(other),
            }
        }

        transformed
    }
}

/// Creates language model middleware that extracts tagged reasoning from text.
pub fn extract_reasoning_middleware(tag_name: impl Into<String>) -> ExtractReasoningMiddleware {
    ExtractReasoningMiddleware::new(tag_name)
}

fn extract_reasoning_matches(
    text: &str,
    opening_tag: &str,
    closing_tag: &str,
) -> Vec<ReasoningTextMatch> {
    let mut matches = Vec::new();
    let mut search_start = 0usize;

    while let Some(opening_index) = text[search_start..].find(opening_tag) {
        let start = search_start + opening_index;
        let reasoning_start = start + opening_tag.len();

        let Some(closing_index) = text[reasoning_start..].find(closing_tag) else {
            break;
        };

        let reasoning_end = reasoning_start + closing_index;
        let end = reasoning_end + closing_tag.len();
        matches.push(ReasoningTextMatch {
            start,
            end,
            text: text[reasoning_start..reasoning_end].to_string(),
        });
        search_start = end;
    }

    matches
}

fn remove_reasoning_matches(text: &str, matches: &[ReasoningTextMatch], separator: &str) -> String {
    let mut text_without_reasoning = text.to_string();

    for reasoning_match in matches.iter().rev() {
        let before_match = text_without_reasoning[..reasoning_match.start].to_string();
        let after_match = text_without_reasoning[reasoning_match.end..].to_string();
        let separator = if !before_match.is_empty() && !after_match.is_empty() {
            separator
        } else {
            ""
        };
        text_without_reasoning = format!("{before_match}{separator}{after_match}");
    }

    text_without_reasoning
}

fn get_potential_start_index(text: &str, searched_text: &str) -> Option<usize> {
    if searched_text.is_empty() {
        return None;
    }

    if let Some(index) = text.find(searched_text) {
        return Some(index);
    }

    text.char_indices()
        .rev()
        .map(|(index, _)| index)
        .find(|index| searched_text.starts_with(&text[*index..]))
}

fn publish_reasoning_extraction_text(
    transformed: &mut Vec<LanguageModelStreamPart>,
    delayed_text_start: &mut Option<LanguageModelTextStart>,
    active_extraction: &mut ReasoningExtractionState,
    separator: &str,
    text: &str,
) {
    if text.is_empty() {
        return;
    }

    let should_prefix = active_extraction.after_switch
        && if active_extraction.is_reasoning {
            !active_extraction.is_first_reasoning
        } else {
            !active_extraction.is_first_text
        };
    let delta = if should_prefix {
        format!("{separator}{text}")
    } else {
        text.to_string()
    };

    if active_extraction.is_reasoning
        && (active_extraction.after_switch || active_extraction.is_first_reasoning)
    {
        transformed.push(LanguageModelStreamPart::ReasoningStart(
            LanguageModelReasoningStart::new(active_extraction.reasoning_id()),
        ));
    }

    if active_extraction.is_reasoning {
        transformed.push(LanguageModelStreamPart::ReasoningDelta(
            LanguageModelReasoningDelta::new(active_extraction.reasoning_id(), delta),
        ));
    } else {
        if let Some(start) = delayed_text_start.take() {
            transformed.push(LanguageModelStreamPart::TextStart(start));
        }
        transformed.push(LanguageModelStreamPart::TextDelta(
            LanguageModelTextDelta::new(active_extraction.text_id.clone(), delta),
        ));
    }

    active_extraction.after_switch = false;

    if active_extraction.is_reasoning {
        active_extraction.is_first_reasoning = false;
    } else {
        active_extraction.is_first_text = false;
    }
}

/// Language model middleware that simulates streaming from `doGenerate`.
///
/// Upstream `simulateStreamingMiddleware` turns a non-streaming generation
/// result into provider-v4 stream parts. This Rust port targets models that use
/// the crate's deterministic `Vec<LanguageModelStreamPart>` stream boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SimulateStreamingMiddleware;

impl SimulateStreamingMiddleware {
    /// Creates streaming simulation middleware.
    pub fn new() -> Self {
        Self
    }

    fn simulate_stream(
        &self,
        result: LanguageModelGenerateResult,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let LanguageModelGenerateResult {
            content,
            finish_reason,
            usage,
            provider_metadata,
            request,
            response,
            warnings,
        } = result;

        let mut stream = vec![LanguageModelStreamPart::StreamStart(
            LanguageModelStreamStart::new(warnings),
        )];

        if let Some(response) = &response {
            stream.push(LanguageModelStreamPart::ResponseMetadata(
                response_metadata_from_generate_response(response),
            ));
        }

        let mut id = 0usize;
        for content in content {
            match content {
                LanguageModelContent::Text(text) => {
                    if !text.text.is_empty() {
                        let text_id = id.to_string();
                        stream.push(LanguageModelStreamPart::TextStart(
                            LanguageModelTextStart::new(text_id.clone()),
                        ));
                        stream.push(LanguageModelStreamPart::TextDelta(
                            LanguageModelTextDelta::new(text_id.clone(), text.text),
                        ));
                        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
                            text_id,
                        )));
                        id += 1;
                    }
                }
                LanguageModelContent::Reasoning(reasoning) => {
                    let reasoning_id = id.to_string();
                    let mut start = LanguageModelReasoningStart::new(reasoning_id.clone());
                    start.provider_metadata = reasoning.provider_metadata;
                    stream.push(LanguageModelStreamPart::ReasoningStart(start));
                    stream.push(LanguageModelStreamPart::ReasoningDelta(
                        LanguageModelReasoningDelta::new(reasoning_id.clone(), reasoning.text),
                    ));
                    stream.push(LanguageModelStreamPart::ReasoningEnd(
                        LanguageModelReasoningEnd::new(reasoning_id),
                    ));
                    id += 1;
                }
                LanguageModelContent::Custom(custom) => {
                    stream.push(LanguageModelStreamPart::Custom(custom));
                }
                LanguageModelContent::ReasoningFile(reasoning_file) => {
                    stream.push(LanguageModelStreamPart::ReasoningFile(reasoning_file));
                }
                LanguageModelContent::File(file) => {
                    stream.push(LanguageModelStreamPart::File(file));
                }
                LanguageModelContent::ToolApprovalRequest(tool_approval_request) => {
                    stream.push(LanguageModelStreamPart::ToolApprovalRequest(
                        tool_approval_request,
                    ));
                }
                LanguageModelContent::Source(source) => {
                    stream.push(LanguageModelStreamPart::Source(source));
                }
                LanguageModelContent::ToolCall(tool_call) => {
                    stream.push(LanguageModelStreamPart::ToolCall(tool_call));
                }
                LanguageModelContent::ToolResult(tool_result) => {
                    stream.push(LanguageModelStreamPart::ToolResult(tool_result));
                }
            }
        }

        let mut finish = LanguageModelStreamFinish::new(usage, finish_reason);
        if let Some(provider_metadata) = provider_metadata {
            finish = finish.with_provider_metadata(provider_metadata);
        }
        stream.push(LanguageModelStreamPart::Finish(finish));

        LanguageModelStreamResult {
            stream,
            request,
            response: response.map(|response| LanguageModelStreamResultResponse {
                headers: response.headers,
            }),
        }
    }
}

fn response_metadata_from_generate_response(
    response: &LanguageModelResponse,
) -> LanguageModelStreamResponseMetadata {
    let mut metadata = LanguageModelStreamResponseMetadata::new();
    metadata.id = response.id.clone();
    metadata.timestamp = response.timestamp;
    metadata.model_id = response.model_id.clone();
    metadata
}

/// Creates language model middleware that simulates streams from generate calls.
pub fn simulate_streaming_middleware() -> SimulateStreamingMiddleware {
    SimulateStreamingMiddleware::new()
}

impl<M: LanguageModel> LanguageModelMiddleware<M> for DefaultSettingsMiddleware {
    type OverrideSupportedUrlsFuture<'a>
        = Ready<LanguageModelSupportedUrls>
    where
        Self: 'a,
        M: 'a;

    type TransformParamsFuture<'a>
        = Ready<LanguageModelCallOptions>
    where
        Self: 'a,
        M: 'a;

    type WrapGenerateFuture<'a>
        = Pending<LanguageModelGenerateResult>
    where
        Self: 'a,
        M: 'a;

    type WrapStreamFuture<'a>
        = Pending<LanguageModelStreamResult<M::Stream>>
    where
        Self: 'a,
        M: 'a;

    fn transform_params<'a>(
        &'a self,
        options: LanguageModelTransformParamsOptions<'a, M>,
    ) -> Option<Self::TransformParamsFuture<'a>>
    where
        M: 'a,
    {
        Some(ready(self.apply_settings(options.params)))
    }
}

impl<M: LanguageModel> LanguageModelMiddleware<M> for AddToolInputExamplesMiddleware {
    type OverrideSupportedUrlsFuture<'a>
        = Pending<LanguageModelSupportedUrls>
    where
        Self: 'a,
        M: 'a;

    type TransformParamsFuture<'a>
        = Ready<LanguageModelCallOptions>
    where
        Self: 'a,
        M: 'a;

    type WrapGenerateFuture<'a>
        = Pending<LanguageModelGenerateResult>
    where
        Self: 'a,
        M: 'a;

    type WrapStreamFuture<'a>
        = Pending<LanguageModelStreamResult<M::Stream>>
    where
        Self: 'a,
        M: 'a;

    fn transform_params<'a>(
        &'a self,
        options: LanguageModelTransformParamsOptions<'a, M>,
    ) -> Option<Self::TransformParamsFuture<'a>>
    where
        M: 'a,
    {
        Some(ready(self.apply_examples(options.params)))
    }
}

impl<M> LanguageModelMiddleware<M> for ExtractJsonMiddleware
where
    M: LanguageModel<Stream = Vec<LanguageModelStreamPart>>,
{
    type OverrideSupportedUrlsFuture<'a>
        = Pending<LanguageModelSupportedUrls>
    where
        Self: 'a,
        M: 'a;

    type TransformParamsFuture<'a>
        = Pending<LanguageModelCallOptions>
    where
        Self: 'a,
        M: 'a;

    type WrapGenerateFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
    where
        Self: 'a,
        M: 'a;

    type WrapStreamFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelStreamResult<M::Stream>> + Send + 'a>>
    where
        Self: 'a,
        M: 'a;

    fn wrap_generate<'a>(
        &'a self,
        options: LanguageModelWrapGenerateOptions<'a, M>,
    ) -> Option<Self::WrapGenerateFuture<'a>>
    where
        M: 'a,
    {
        Some(Box::pin(async move {
            let mut result = (options.do_generate)().await;
            result.content = result
                .content
                .into_iter()
                .map(|content| self.transform_content(content))
                .collect();
            result
        }))
    }

    fn wrap_stream<'a>(
        &'a self,
        options: LanguageModelWrapStreamOptions<'a, M>,
    ) -> Option<Self::WrapStreamFuture<'a>>
    where
        M: 'a,
    {
        Some(Box::pin(async move {
            let mut result = (options.do_stream)().await;
            result.stream = self.transform_stream(result.stream);
            result
        }))
    }
}

impl<M> LanguageModelMiddleware<M> for ExtractReasoningMiddleware
where
    M: LanguageModel<Stream = Vec<LanguageModelStreamPart>>,
{
    type OverrideSupportedUrlsFuture<'a>
        = Pending<LanguageModelSupportedUrls>
    where
        Self: 'a,
        M: 'a;

    type TransformParamsFuture<'a>
        = Pending<LanguageModelCallOptions>
    where
        Self: 'a,
        M: 'a;

    type WrapGenerateFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
    where
        Self: 'a,
        M: 'a;

    type WrapStreamFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelStreamResult<M::Stream>> + Send + 'a>>
    where
        Self: 'a,
        M: 'a;

    fn wrap_generate<'a>(
        &'a self,
        options: LanguageModelWrapGenerateOptions<'a, M>,
    ) -> Option<Self::WrapGenerateFuture<'a>>
    where
        M: 'a,
    {
        Some(Box::pin(async move {
            let mut result = (options.do_generate)().await;
            result.content = result
                .content
                .into_iter()
                .flat_map(|content| self.transform_content(content))
                .collect();
            result
        }))
    }

    fn wrap_stream<'a>(
        &'a self,
        options: LanguageModelWrapStreamOptions<'a, M>,
    ) -> Option<Self::WrapStreamFuture<'a>>
    where
        M: 'a,
    {
        Some(Box::pin(async move {
            let mut result = (options.do_stream)().await;
            result.stream = self.transform_stream(result.stream);
            result
        }))
    }
}

impl<M> LanguageModelMiddleware<M> for SimulateStreamingMiddleware
where
    M: LanguageModel<Stream = Vec<LanguageModelStreamPart>>,
{
    type OverrideSupportedUrlsFuture<'a>
        = Pending<LanguageModelSupportedUrls>
    where
        Self: 'a,
        M: 'a;

    type TransformParamsFuture<'a>
        = Pending<LanguageModelCallOptions>
    where
        Self: 'a,
        M: 'a;

    type WrapGenerateFuture<'a>
        = Pending<LanguageModelGenerateResult>
    where
        Self: 'a,
        M: 'a;

    type WrapStreamFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelStreamResult<M::Stream>> + Send + 'a>>
    where
        Self: 'a,
        M: 'a;

    fn wrap_stream<'a>(
        &'a self,
        options: LanguageModelWrapStreamOptions<'a, M>,
    ) -> Option<Self::WrapStreamFuture<'a>>
    where
        M: 'a,
    {
        Some(Box::pin(async move {
            self.simulate_stream((options.do_generate)().await)
        }))
    }
}

fn merge_headers(
    default_headers: Option<&Headers>,
    params_headers: Option<Headers>,
) -> Option<Headers> {
    if default_headers.is_none() && params_headers.is_none() {
        return None;
    }

    let mut headers = default_headers.cloned().unwrap_or_default();

    if let Some(params_headers) = params_headers {
        headers.extend(params_headers);
    }

    Some(headers)
}

fn merge_provider_options_deep(
    default_provider_options: Option<&ProviderOptions>,
    params_provider_options: Option<ProviderOptions>,
) -> Option<ProviderOptions> {
    if default_provider_options.is_none() && params_provider_options.is_none() {
        return None;
    }

    let mut provider_options = default_provider_options.cloned().unwrap_or_default();

    if let Some(params_provider_options) = params_provider_options {
        for (provider, params_options) in params_provider_options {
            match provider_options.get_mut(&provider) {
                Some(default_options) => {
                    *default_options = merge_json_objects(default_options, &params_options);
                }
                None => {
                    provider_options.insert(provider, params_options);
                }
            }
        }
    }

    Some(provider_options)
}

fn merge_json_objects(default_object: &JsonObject, params_object: &JsonObject) -> JsonObject {
    let mut merged = default_object.clone();

    for (key, params_value) in params_object {
        if matches!(key.as_str(), "__proto__" | "constructor" | "prototype") {
            continue;
        }

        let merged_value = match (merged.get(key), params_value) {
            (Some(JsonValue::Object(default_nested)), JsonValue::Object(params_nested)) => {
                JsonValue::Object(merge_json_objects(default_nested, params_nested))
            }
            _ => params_value.clone(),
        };

        merged.insert(key.clone(), merged_value);
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::{
        AddToolInputExamplesMiddleware, ExtractJsonMiddleware, ExtractJsonTransformFunction,
        ExtractReasoningMiddleware, LanguageModelDefaultSettings, LanguageModelMiddleware,
        LanguageModelMiddlewareCallType, LanguageModelMiddlewareModelOptions,
        LanguageModelTransformParamsOptions, LanguageModelWrapGenerateOptions,
        LanguageModelWrapStreamOptions, add_tool_input_examples_middleware,
        default_extract_json_transform, default_settings_middleware, extract_json_middleware,
        extract_reasoning_middleware, simulate_streaming_middleware, wrap_language_model,
    };
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFinishReason, LanguageModelFunctionTool, LanguageModelGenerateResult,
        LanguageModelProviderTool, LanguageModelReasoning, LanguageModelReasoningDelta,
        LanguageModelReasoningEnd, LanguageModelReasoningStart, LanguageModelResponse,
        LanguageModelStreamFinish, LanguageModelStreamPart, LanguageModelStreamResponseMetadata,
        LanguageModelStreamResult, LanguageModelStreamStart, LanguageModelSupportedUrls,
        LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
        LanguageModelTool, LanguageModelToolCall, LanguageModelToolInputDelta,
        LanguageModelToolInputEnd, LanguageModelToolInputStart, LanguageModelUsage,
    };
    use crate::provider::SpecificationVersion;
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    struct StaticLanguageModel;

    impl LanguageModel for StaticLanguageModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a;

        type Stream = Vec<LanguageModelStreamPart>;

        type StreamFuture<'a>
            = Ready<LanguageModelStreamResult<Self::Stream>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "base-provider"
        }

        fn model_id(&self) -> &str {
            "language-base"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(BTreeMap::from([(
                "image/*".to_string(),
                vec!["^https://base\\.example/images/".to_string()],
            )]))
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(language_result("generated"))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
            ]))
        }
    }

    struct StaticLanguageMiddleware;

    impl LanguageModelMiddleware<StaticLanguageModel> for StaticLanguageMiddleware {
        type OverrideSupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type TransformParamsFuture<'a>
            = Ready<LanguageModelCallOptions>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type WrapGenerateFuture<'a>
            = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type WrapStreamFuture<'a>
            = Pin<
            Box<
                dyn Future<Output = LanguageModelStreamResult<Vec<LanguageModelStreamPart>>>
                    + Send
                    + 'a,
            >,
        >
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        fn override_provider(
            &self,
            options: LanguageModelMiddlewareModelOptions<'_, StaticLanguageModel>,
        ) -> Option<String> {
            Some(format!("{}-middleware", options.model.provider()))
        }

        fn override_model_id(
            &self,
            options: LanguageModelMiddlewareModelOptions<'_, StaticLanguageModel>,
        ) -> Option<String> {
            Some(format!("{}-wrapped", options.model.model_id()))
        }

        fn override_supported_urls<'a>(
            &'a self,
            options: LanguageModelMiddlewareModelOptions<'a, StaticLanguageModel>,
        ) -> Option<Self::OverrideSupportedUrlsFuture<'a>>
        where
            Self: 'a,
            StaticLanguageModel: 'a,
        {
            assert_eq!(options.model.model_id(), "language-base");
            Some(ready(BTreeMap::from([(
                "application/pdf".to_string(),
                vec!["\\.pdf$".to_string()],
            )])))
        }

        fn transform_params<'a>(
            &'a self,
            mut options: LanguageModelTransformParamsOptions<'a, StaticLanguageModel>,
        ) -> Option<Self::TransformParamsFuture<'a>>
        where
            Self: 'a,
            StaticLanguageModel: 'a,
        {
            assert_eq!(options.model.provider(), "base-provider");

            match options.call_type {
                LanguageModelMiddlewareCallType::Generate => {
                    options.params.max_output_tokens = Some(128);
                }
                LanguageModelMiddlewareCallType::Stream => {
                    options.params.include_raw_chunks = Some(true);
                }
            }

            Some(ready(options.params))
        }

        fn wrap_generate<'a>(
            &'a self,
            options: LanguageModelWrapGenerateOptions<'a, StaticLanguageModel>,
        ) -> Option<Self::WrapGenerateFuture<'a>>
        where
            Self: 'a,
            StaticLanguageModel: 'a,
        {
            assert_eq!(options.params.max_output_tokens, Some(32));
            assert_eq!(options.model.model_id(), "language-base");

            Some(Box::pin(async move {
                let mut result = (options.do_generate)().await;
                result.warnings.push(Warning::Other {
                    message: "wrapped-generate".to_string(),
                });
                result
            }))
        }

        fn wrap_stream<'a>(
            &'a self,
            options: LanguageModelWrapStreamOptions<'a, StaticLanguageModel>,
        ) -> Option<Self::WrapStreamFuture<'a>>
        where
            Self: 'a,
            StaticLanguageModel: 'a,
        {
            assert_eq!(options.params.include_raw_chunks, Some(true));
            assert_eq!(options.model.provider(), "base-provider");

            Some(Box::pin(async move {
                let mut result = (options.do_stream)().await;
                result.stream.push(LanguageModelStreamPart::StreamStart(
                    LanguageModelStreamStart::new(vec![Warning::Other {
                        message: "wrapped-stream".to_string(),
                    }]),
                ));
                result
            }))
        }
    }

    struct ParamEchoLanguageModel;

    impl LanguageModel for ParamEchoLanguageModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a;

        type Stream = Vec<LanguageModelStreamPart>;

        type StreamFuture<'a>
            = Ready<LanguageModelStreamResult<Self::Stream>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "echo-provider"
        }

        fn model_id(&self) -> &str {
            "echo-language"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(BTreeMap::new())
        }

        fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            let message = format!(
                "generated max {:?}",
                options.max_output_tokens.unwrap_or_default()
            );
            ready(language_result("echo").with_warning(Warning::Other { message }))
        }

        fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            let message = if let Some(max_output_tokens) = options.max_output_tokens {
                format!(
                    "stream max {} raw {:?}",
                    max_output_tokens,
                    options.include_raw_chunks.unwrap_or(false)
                )
            } else {
                format!(
                    "stream raw {:?}",
                    options.include_raw_chunks.unwrap_or(false)
                )
            };
            ready(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                    Warning::Other { message },
                ])),
            ]))
        }
    }

    struct TransformAndWrapLanguageMiddleware;

    impl LanguageModelMiddleware<ParamEchoLanguageModel> for TransformAndWrapLanguageMiddleware {
        type OverrideSupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a,
            ParamEchoLanguageModel: 'a;

        type TransformParamsFuture<'a>
            = Ready<LanguageModelCallOptions>
        where
            Self: 'a,
            ParamEchoLanguageModel: 'a;

        type WrapGenerateFuture<'a>
            = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
        where
            Self: 'a,
            ParamEchoLanguageModel: 'a;

        type WrapStreamFuture<'a>
            = Pin<
            Box<
                dyn Future<Output = LanguageModelStreamResult<Vec<LanguageModelStreamPart>>>
                    + Send
                    + 'a,
            >,
        >
        where
            Self: 'a,
            ParamEchoLanguageModel: 'a;

        fn transform_params<'a>(
            &'a self,
            mut options: LanguageModelTransformParamsOptions<'a, ParamEchoLanguageModel>,
        ) -> Option<Self::TransformParamsFuture<'a>>
        where
            Self: 'a,
            ParamEchoLanguageModel: 'a,
        {
            match options.call_type {
                LanguageModelMiddlewareCallType::Generate => {
                    options.params.max_output_tokens = Some(77);
                }
                LanguageModelMiddlewareCallType::Stream => {
                    options.params.include_raw_chunks = Some(true);
                }
            }

            Some(ready(options.params))
        }

        fn wrap_generate<'a>(
            &'a self,
            options: LanguageModelWrapGenerateOptions<'a, ParamEchoLanguageModel>,
        ) -> Option<Self::WrapGenerateFuture<'a>>
        where
            Self: 'a,
            ParamEchoLanguageModel: 'a,
        {
            assert_eq!(options.params.max_output_tokens, Some(77));

            Some(Box::pin(async move {
                let mut result = (options.do_generate)().await;
                result.warnings.push(Warning::Other {
                    message: "wrapped-generate".to_string(),
                });
                result
            }))
        }

        fn wrap_stream<'a>(
            &'a self,
            options: LanguageModelWrapStreamOptions<'a, ParamEchoLanguageModel>,
        ) -> Option<Self::WrapStreamFuture<'a>>
        where
            Self: 'a,
            ParamEchoLanguageModel: 'a,
        {
            assert_eq!(options.params.include_raw_chunks, Some(true));

            Some(Box::pin(async move {
                let mut result = (options.do_stream)().await;
                result.stream.push(LanguageModelStreamPart::StreamStart(
                    LanguageModelStreamStart::new(vec![Warning::Other {
                        message: "wrapped-stream".to_string(),
                    }]),
                ));
                result
            }))
        }
    }

    #[derive(Clone, Copy)]
    struct NoopLanguageMiddleware;

    impl<M> LanguageModelMiddleware<M> for NoopLanguageMiddleware
    where
        M: LanguageModel,
        M::Stream: Send,
    {
        type OverrideSupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a,
            M: 'a;

        type TransformParamsFuture<'a>
            = Ready<LanguageModelCallOptions>
        where
            Self: 'a,
            M: 'a;

        type WrapGenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a,
            M: 'a;

        type WrapStreamFuture<'a>
            = Ready<LanguageModelStreamResult<M::Stream>>
        where
            Self: 'a,
            M: 'a;
    }

    #[derive(Clone, Copy)]
    struct AddMaxOutputTokensMiddleware(u64);

    impl<M> LanguageModelMiddleware<M> for AddMaxOutputTokensMiddleware
    where
        M: LanguageModel,
        M::Stream: Send,
    {
        type OverrideSupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a,
            M: 'a;

        type TransformParamsFuture<'a>
            = Ready<LanguageModelCallOptions>
        where
            Self: 'a,
            M: 'a;

        type WrapGenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a,
            M: 'a;

        type WrapStreamFuture<'a>
            = Ready<LanguageModelStreamResult<M::Stream>>
        where
            Self: 'a,
            M: 'a;

        fn transform_params<'a>(
            &'a self,
            mut options: LanguageModelTransformParamsOptions<'a, M>,
        ) -> Option<Self::TransformParamsFuture<'a>>
        where
            Self: 'a,
            M: 'a,
        {
            options.params.max_output_tokens =
                Some(options.params.max_output_tokens.unwrap_or_default() + self.0);
            Some(ready(options.params))
        }
    }

    #[derive(Clone, Copy)]
    struct AppendGenerateWarningMiddleware(&'static str);

    impl<M> LanguageModelMiddleware<M> for AppendGenerateWarningMiddleware
    where
        M: LanguageModel,
        M::Stream: Send,
    {
        type OverrideSupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a,
            M: 'a;

        type TransformParamsFuture<'a>
            = Ready<LanguageModelCallOptions>
        where
            Self: 'a,
            M: 'a;

        type WrapGenerateFuture<'a>
            = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
        where
            Self: 'a,
            M: 'a;

        type WrapStreamFuture<'a>
            = Ready<LanguageModelStreamResult<M::Stream>>
        where
            Self: 'a,
            M: 'a;

        fn wrap_generate<'a>(
            &'a self,
            options: LanguageModelWrapGenerateOptions<'a, M>,
        ) -> Option<Self::WrapGenerateFuture<'a>>
        where
            Self: 'a,
            M: 'a,
        {
            Some(Box::pin(async move {
                let mut result = (options.do_generate)().await;
                result.warnings.push(Warning::Other {
                    message: self.0.to_string(),
                });
                result
            }))
        }
    }

    #[derive(Clone, Copy)]
    struct AppendStreamWarningMiddleware(&'static str);

    impl<M> LanguageModelMiddleware<M> for AppendStreamWarningMiddleware
    where
        M: LanguageModel<Stream = Vec<LanguageModelStreamPart>>,
    {
        type OverrideSupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a,
            M: 'a;

        type TransformParamsFuture<'a>
            = Ready<LanguageModelCallOptions>
        where
            Self: 'a,
            M: 'a;

        type WrapGenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a,
            M: 'a;

        type WrapStreamFuture<'a>
            = Pin<
            Box<
                dyn Future<Output = LanguageModelStreamResult<Vec<LanguageModelStreamPart>>>
                    + Send
                    + 'a,
            >,
        >
        where
            Self: 'a,
            M: 'a;

        fn wrap_stream<'a>(
            &'a self,
            options: LanguageModelWrapStreamOptions<'a, M>,
        ) -> Option<Self::WrapStreamFuture<'a>>
        where
            Self: 'a,
            M: 'a,
        {
            Some(Box::pin(async move {
                let mut result = (options.do_stream)().await;
                result.stream.push(LanguageModelStreamPart::StreamStart(
                    LanguageModelStreamStart::new(vec![Warning::Other {
                        message: self.0.to_string(),
                    }]),
                ));
                result
            }))
        }
    }

    struct StatefulSupportedUrlsLanguageModel {
        urls: LanguageModelSupportedUrls,
    }

    impl LanguageModel for StatefulSupportedUrlsLanguageModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a;

        type Stream = Vec<LanguageModelStreamPart>;

        type StreamFuture<'a>
            = Ready<LanguageModelStreamResult<Self::Stream>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "stateful-provider"
        }

        fn model_id(&self) -> &str {
            "stateful-language"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(self.urls.clone())
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(language_result("stateful"))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(Vec::new()))
        }
    }

    fn language_result(text: &str) -> LanguageModelGenerateResult {
        LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(text))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: None,
            },
            LanguageModelUsage::default(),
        )
    }

    fn object_schema() -> crate::json::JsonSchema {
        json_object(json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        }))
    }

    fn json_object(value: serde_json::Value) -> crate::json::JsonObject {
        value.as_object().expect("value is a JSON object").clone()
    }

    fn transform_default_settings(
        settings: LanguageModelDefaultSettings,
        params: LanguageModelCallOptions,
    ) -> LanguageModelCallOptions {
        let middleware = default_settings_middleware(settings);
        let transformed = middleware
            .transform_params(LanguageModelTransformParamsOptions::new(
                LanguageModelMiddlewareCallType::Generate,
                params,
                &StaticLanguageModel,
            ))
            .expect("default settings transform exists");

        poll_ready(transformed)
    }

    fn provider_options(value: serde_json::Value) -> crate::provider::ProviderOptions {
        serde_json::from_value(value).expect("provider options deserialize")
    }

    fn transform_tool_examples(
        middleware: &AddToolInputExamplesMiddleware,
        tools: Option<Vec<LanguageModelTool>>,
    ) -> Option<Vec<LanguageModelTool>> {
        let mut params = LanguageModelCallOptions::new(Vec::new());
        params.tools = tools;
        let transformed = middleware
            .transform_params(LanguageModelTransformParamsOptions::new(
                LanguageModelMiddlewareCallType::Generate,
                params,
                &StaticLanguageModel,
            ))
            .expect("tool examples transform exists");

        poll_ready(transformed).tools
    }

    fn transformed_function_tool(
        middleware: &AddToolInputExamplesMiddleware,
        tool: LanguageModelFunctionTool,
    ) -> LanguageModelFunctionTool {
        let tools =
            transform_tool_examples(middleware, Some(vec![LanguageModelTool::Function(tool)]))
                .expect("tools are retained");

        let LanguageModelTool::Function(tool) = tools.into_iter().next().expect("one tool") else {
            panic!("function tool should remain a function tool");
        };

        tool
    }

    fn poll_ready<T>(mut future: Ready<T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("std::future::Ready never returns pending"),
        }
    }

    fn poll_boxed<T>(mut future: Pin<Box<dyn Future<Output = T> + Send + '_>>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test future only awaits ready futures"),
        }
    }

    fn strip_prefix_suffix(text: &str) -> String {
        text.replace("PREFIX", "").replace("SUFFIX", "")
    }

    fn extract_json_generate_content(
        content: Vec<LanguageModelContent>,
        transform: Option<ExtractJsonTransformFunction>,
    ) -> Vec<LanguageModelContent> {
        let model = StaticLanguageModel;
        let middleware = if let Some(transform) = transform {
            ExtractJsonMiddleware::new().with_transform(transform)
        } else {
            extract_json_middleware()
        };
        let wrapped_generate = middleware
            .wrap_generate(LanguageModelWrapGenerateOptions::new(
                Box::new(move || {
                    Box::pin(ready(LanguageModelGenerateResult::new(
                        content,
                        LanguageModelFinishReason {
                            unified: FinishReason::Stop,
                            raw: None,
                        },
                        LanguageModelUsage::default(),
                    )))
                }),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract JSON wrap-generate exists");

        poll_boxed(wrapped_generate).content
    }

    fn extract_json_generate_text(
        text: impl Into<String>,
        transform: Option<ExtractJsonTransformFunction>,
    ) -> String {
        let content = extract_json_generate_content(
            vec![LanguageModelContent::Text(LanguageModelText::new(text))],
            transform,
        );

        match &content[0] {
            LanguageModelContent::Text(text) => text.text.clone(),
            other => panic!("expected text content, got {other:?}"),
        }
    }

    fn extract_json_stream_parts(
        stream: Vec<LanguageModelStreamPart>,
        transform: Option<ExtractJsonTransformFunction>,
    ) -> Vec<LanguageModelStreamPart> {
        let model = StaticLanguageModel;
        let middleware = if let Some(transform) = transform {
            ExtractJsonMiddleware::new().with_transform(transform)
        } else {
            extract_json_middleware()
        };
        let wrapped_stream = middleware
            .wrap_stream(LanguageModelWrapStreamOptions::new(
                Box::new(|| Box::pin(ready(language_result("unused")))),
                Box::new(move || Box::pin(ready(LanguageModelStreamResult::new(stream)))),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract JSON wrap-stream exists");

        poll_boxed(wrapped_stream).stream
    }

    fn text_stream(id: &str, deltas: &[&str]) -> Vec<LanguageModelStreamPart> {
        let mut stream = vec![LanguageModelStreamPart::TextStart(
            LanguageModelTextStart::new(id),
        )];
        stream.extend(deltas.iter().map(|delta| {
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(id, *delta))
        }));
        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
            id,
        )));
        stream
    }

    fn collect_text_deltas(stream: &[LanguageModelStreamPart]) -> String {
        stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::TextDelta(delta) => Some(delta.delta.as_str()),
                _ => None,
            })
            .collect::<String>()
    }

    fn extract_reasoning_generate_content(
        text: impl Into<String>,
        start_with_reasoning: bool,
    ) -> Vec<LanguageModelContent> {
        let model = StaticLanguageModel;
        let source_text = text.into();
        let middleware = ExtractReasoningMiddleware::new("think")
            .with_start_with_reasoning(start_with_reasoning);
        let wrapped_generate = middleware
            .wrap_generate(LanguageModelWrapGenerateOptions::new(
                Box::new(move || Box::pin(ready(language_result(&source_text)))),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract reasoning wrap-generate exists");

        poll_boxed(wrapped_generate).content
    }

    fn extract_reasoning_stream_parts(
        deltas: &[&str],
        start_with_reasoning: bool,
    ) -> Vec<LanguageModelStreamPart> {
        let model = StaticLanguageModel;
        let middleware = ExtractReasoningMiddleware::new("think")
            .with_start_with_reasoning(start_with_reasoning);
        let stream = text_stream("1", deltas);
        let wrapped_stream = middleware
            .wrap_stream(LanguageModelWrapStreamOptions::new(
                Box::new(|| Box::pin(ready(language_result("unused")))),
                Box::new(move || Box::pin(ready(LanguageModelStreamResult::new(stream)))),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract reasoning wrap-stream exists");

        poll_boxed(wrapped_stream).stream
    }

    fn collect_reasoning_deltas(stream: &[LanguageModelStreamPart]) -> String {
        stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ReasoningDelta(delta) => Some(delta.delta.as_str()),
                _ => None,
            })
            .collect::<String>()
    }

    #[test]
    fn language_model_middleware_call_type_uses_upstream_literals() {
        assert_eq!(
            serde_json::to_value(LanguageModelMiddlewareCallType::Generate)
                .expect("call type serializes"),
            json!("generate")
        );
        assert_eq!(
            serde_json::from_value::<LanguageModelMiddlewareCallType>(json!("stream"))
                .expect("call type deserializes"),
            LanguageModelMiddlewareCallType::Stream
        );
    }

    #[test]
    fn language_model_default_settings_serialize_as_upstream_partial_call_options() {
        let settings = LanguageModelDefaultSettings::new()
            .with_max_output_tokens(100)
            .with_temperature(0.7)
            .with_stop_sequence("stop")
            .with_top_p(0.9)
            .with_top_k(40)
            .with_presence_penalty(0.1)
            .with_frequency_penalty(0.2)
            .with_seed(42)
            .with_header("x-default", "default");

        assert_eq!(
            serde_json::to_value(settings).expect("default settings serialize"),
            json!({
                "maxOutputTokens": 100,
                "temperature": 0.7,
                "stopSequences": ["stop"],
                "topP": 0.9,
                "topK": 40,
                "presencePenalty": 0.1,
                "frequencyPenalty": 0.2,
                "seed": 42,
                "headers": {
                    "x-default": "default"
                }
            })
        );
    }

    #[test]
    fn default_settings_middleware_applies_defaults_without_overriding_params() {
        let model = StaticLanguageModel;
        let middleware = default_settings_middleware(
            LanguageModelDefaultSettings::new()
                .with_max_output_tokens(100)
                .with_temperature(0.7)
                .with_stop_sequence("stop")
                .with_header("x-default", "default")
                .with_header("x-shared", "default"),
        );

        let mut params = LanguageModelCallOptions::new(Vec::new())
            .with_temperature(0.5)
            .with_header("x-shared", "param");
        params.top_p = Some(0.9);

        let transformed = middleware
            .transform_params(LanguageModelTransformParamsOptions::new(
                LanguageModelMiddlewareCallType::Generate,
                params,
                &model,
            ))
            .expect("default settings transform exists");
        let transformed = poll_ready(transformed);

        assert_eq!(transformed.max_output_tokens, Some(100));
        assert_eq!(transformed.temperature, Some(0.5));
        assert_eq!(transformed.stop_sequences, Some(vec!["stop".to_string()]));
        assert_eq!(transformed.top_p, Some(0.9));
        assert_eq!(
            transformed.headers,
            Some(BTreeMap::from([
                ("x-default".to_string(), "default".to_string()),
                ("x-shared".to_string(), "param".to_string()),
            ]))
        );
    }

    #[test]
    fn default_settings_middleware_deep_merges_provider_options() {
        let model = StaticLanguageModel;
        let default_provider_options: crate::provider::ProviderOptions =
            serde_json::from_value(json!({
                "anthropic": {
                    "cacheControl": { "type": "ephemeral" },
                    "tools": {
                        "retrieval": { "enabled": true },
                        "math": { "enabled": true }
                    }
                },
                "openai": {
                    "logit_bias": { "50256": -100 }
                }
            }))
            .expect("default provider options deserialize");
        let params_provider_options: crate::provider::ProviderOptions =
            serde_json::from_value(json!({
                "anthropic": {
                    "tools": {
                        "retrieval": { "enabled": false },
                        "code": { "enabled": true }
                    },
                    "otherSetting": "value"
                }
            }))
            .expect("params provider options deserialize");
        let middleware = default_settings_middleware(
            LanguageModelDefaultSettings::new().with_provider_options(default_provider_options),
        );

        let params = LanguageModelCallOptions::new(Vec::new())
            .with_provider_options(params_provider_options);

        let transformed = middleware
            .transform_params(LanguageModelTransformParamsOptions::new(
                LanguageModelMiddlewareCallType::Stream,
                params,
                &model,
            ))
            .expect("default settings transform exists");
        let transformed = poll_ready(transformed);

        assert_eq!(
            serde_json::to_value(transformed.provider_options)
                .expect("merged provider options serialize"),
            json!({
                "anthropic": {
                    "cacheControl": { "type": "ephemeral" },
                    "tools": {
                        "retrieval": { "enabled": false },
                        "math": { "enabled": true },
                        "code": { "enabled": true }
                    },
                    "otherSetting": "value"
                },
                "openai": {
                    "logit_bias": { "50256": -100 }
                }
            })
        );
    }

    #[test]
    fn default_settings_middleware_should_apply_default_settings() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_temperature(0.7),
            LanguageModelCallOptions::new(Vec::new()),
        );

        assert_eq!(transformed.temperature, Some(0.7));
    }

    #[test]
    fn default_settings_middleware_should_give_precedence_to_user_provided_settings() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_temperature(0.7),
            LanguageModelCallOptions::new(Vec::new()).with_temperature(0.5),
        );

        assert_eq!(transformed.temperature, Some(0.5));
    }

    #[test]
    fn default_settings_middleware_should_merge_provider_metadata_with_default_settings() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new()
                .with_temperature(0.7)
                .with_provider_options(provider_options(json!({
                    "anthropic": {
                        "cacheControl": { "type": "ephemeral" }
                    }
                }))),
            LanguageModelCallOptions::new(Vec::new()),
        );

        assert_eq!(transformed.temperature, Some(0.7));
        assert_eq!(
            serde_json::to_value(transformed.provider_options).expect("provider options serialize"),
            json!({
                "anthropic": {
                    "cacheControl": { "type": "ephemeral" }
                }
            })
        );
    }

    #[test]
    fn default_settings_middleware_should_merge_complex_provider_metadata_objects() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_provider_options(provider_options(json!({
                "anthropic": {
                    "cacheControl": { "type": "ephemeral" },
                    "feature": { "enabled": true }
                },
                "openai": {
                    "logit_bias": { "50256": -100 }
                }
            }))),
            LanguageModelCallOptions::new(Vec::new()).with_provider_options(provider_options(
                json!({
                    "anthropic": {
                        "feature": { "enabled": false },
                        "otherSetting": "value"
                    }
                }),
            )),
        );

        assert_eq!(
            serde_json::to_value(transformed.provider_options).expect("provider options serialize"),
            json!({
                "anthropic": {
                    "cacheControl": { "type": "ephemeral" },
                    "feature": { "enabled": false },
                    "otherSetting": "value"
                },
                "openai": {
                    "logit_bias": { "50256": -100 }
                }
            })
        );
    }

    #[test]
    fn default_settings_middleware_should_keep_zero_temperature_when_default_is_not_set() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new(),
            LanguageModelCallOptions::new(Vec::new()).with_temperature(0.0),
        );

        assert_eq!(transformed.temperature, Some(0.0));
    }

    #[test]
    fn default_settings_middleware_should_apply_default_max_output_tokens() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_max_output_tokens(100),
            LanguageModelCallOptions::new(Vec::new()),
        );

        assert_eq!(transformed.max_output_tokens, Some(100));
    }

    #[test]
    fn default_settings_middleware_should_prioritize_param_max_output_tokens() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_max_output_tokens(100),
            LanguageModelCallOptions::new(Vec::new()).with_max_output_tokens(50),
        );

        assert_eq!(transformed.max_output_tokens, Some(50));
    }

    #[test]
    fn default_settings_middleware_should_apply_default_stop_sequences() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_stop_sequence("stop"),
            LanguageModelCallOptions::new(Vec::new()),
        );

        assert_eq!(transformed.stop_sequences, Some(vec!["stop".to_string()]));
    }

    #[test]
    fn default_settings_middleware_should_prioritize_param_stop_sequences() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_stop_sequence("stop"),
            LanguageModelCallOptions::new(Vec::new()).with_stop_sequence("end"),
        );

        assert_eq!(transformed.stop_sequences, Some(vec!["end".to_string()]));
    }

    #[test]
    fn default_settings_middleware_should_apply_default_top_p() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_top_p(0.9),
            LanguageModelCallOptions::new(Vec::new()),
        );

        assert_eq!(transformed.top_p, Some(0.9));
    }

    #[test]
    fn default_settings_middleware_should_prioritize_param_top_p() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_top_p(0.9),
            LanguageModelCallOptions::new(Vec::new()).with_top_p(0.5),
        );

        assert_eq!(transformed.top_p, Some(0.5));
    }

    #[test]
    fn default_settings_middleware_should_merge_headers() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new()
                .with_header("X-Custom-Header", "test")
                .with_header("X-Another-Header", "test2"),
            LanguageModelCallOptions::new(Vec::new()).with_header("X-Custom-Header", "test2"),
        );

        assert_eq!(
            transformed.headers,
            Some(BTreeMap::from([
                ("X-Custom-Header".to_string(), "test2".to_string()),
                ("X-Another-Header".to_string(), "test2".to_string()),
            ]))
        );
    }

    #[test]
    fn default_settings_middleware_should_handle_empty_default_headers() {
        let mut settings = LanguageModelDefaultSettings::new();
        settings.headers = Some(BTreeMap::new());
        let transformed = transform_default_settings(
            settings,
            LanguageModelCallOptions::new(Vec::new()).with_header("X-Param-Header", "param"),
        );

        assert_eq!(
            transformed.headers,
            Some(BTreeMap::from([(
                "X-Param-Header".to_string(),
                "param".to_string()
            )]))
        );
    }

    #[test]
    fn default_settings_middleware_should_handle_empty_param_headers() {
        let mut params = LanguageModelCallOptions::new(Vec::new());
        params.headers = Some(BTreeMap::new());
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_header("X-Default-Header", "default"),
            params,
        );

        assert_eq!(
            transformed.headers,
            Some(BTreeMap::from([(
                "X-Default-Header".to_string(),
                "default".to_string()
            )]))
        );
    }

    #[test]
    fn default_settings_middleware_should_handle_both_headers_being_undefined() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new(),
            LanguageModelCallOptions::new(Vec::new()),
        );

        assert_eq!(transformed.headers, None);
    }

    #[test]
    fn default_settings_middleware_should_handle_empty_default_provider_options() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_provider_options(provider_options(json!({}))),
            LanguageModelCallOptions::new(Vec::new()).with_provider_options(provider_options(
                json!({ "openai": { "user": "param-user" } }),
            )),
        );

        assert_eq!(
            serde_json::to_value(transformed.provider_options).expect("provider options serialize"),
            json!({ "openai": { "user": "param-user" } })
        );
    }

    #[test]
    fn default_settings_middleware_should_handle_empty_param_provider_options() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new().with_provider_options(provider_options(
                json!({ "anthropic": { "user": "default-user" } }),
            )),
            LanguageModelCallOptions::new(Vec::new())
                .with_provider_options(provider_options(json!({}))),
        );

        assert_eq!(
            serde_json::to_value(transformed.provider_options).expect("provider options serialize"),
            json!({ "anthropic": { "user": "default-user" } })
        );
    }

    #[test]
    fn default_settings_middleware_should_handle_both_provider_options_being_undefined() {
        let transformed = transform_default_settings(
            LanguageModelDefaultSettings::new(),
            LanguageModelCallOptions::new(Vec::new()),
        );

        assert_eq!(transformed.provider_options, None);
    }

    #[test]
    fn add_tool_input_examples_middleware_appends_examples_and_removes_them_by_default() {
        let model = StaticLanguageModel;
        let middleware = add_tool_input_examples_middleware();
        let params =
            LanguageModelCallOptions::new(Vec::new()).with_tool(LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", object_schema())
                    .with_description("Get the weather")
                    .with_input_example(json_object(json!({ "city": "Brisbane" })))
                    .with_input_example(json_object(json!({ "city": "Sydney" }))),
            ));

        let transformed = middleware
            .transform_params(LanguageModelTransformParamsOptions::new(
                LanguageModelMiddlewareCallType::Generate,
                params,
                &model,
            ))
            .expect("tool examples transform exists");
        let transformed = poll_ready(transformed);

        let tools = transformed.tools.expect("tools are retained");
        let LanguageModelTool::Function(tool) = &tools[0] else {
            panic!("function tool should remain a function tool");
        };

        assert_eq!(
            tool.description.as_deref(),
            Some(
                "Get the weather\n\nInput Examples:\n{\"city\":\"Brisbane\"}\n{\"city\":\"Sydney\"}"
            )
        );
        assert_eq!(tool.input_examples, None);
    }

    #[test]
    fn add_tool_input_examples_middleware_supports_custom_options() {
        fn format_city(
            example: &crate::language_model::LanguageModelToolInputExample,
            index: usize,
        ) -> String {
            let city = example
                .input
                .get("city")
                .and_then(|value| value.as_str())
                .expect("city example is a string");

            format!("{}: {city}", index + 1)
        }

        let model = StaticLanguageModel;
        let middleware = AddToolInputExamplesMiddleware::new()
            .with_prefix("Examples:")
            .with_remove(false)
            .with_format(format_city);
        let params =
            LanguageModelCallOptions::new(Vec::new()).with_tool(LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", object_schema())
                    .with_input_example(json_object(json!({ "city": "Brisbane" }))),
            ));

        let transformed = middleware
            .transform_params(LanguageModelTransformParamsOptions::new(
                LanguageModelMiddlewareCallType::Stream,
                params,
                &model,
            ))
            .expect("tool examples transform exists");
        let transformed = poll_ready(transformed);

        let tools = transformed.tools.expect("tools are retained");
        let LanguageModelTool::Function(tool) = &tools[0] else {
            panic!("function tool should remain a function tool");
        };

        assert_eq!(middleware.prefix(), "Examples:");
        assert!(!middleware.remove());
        assert_eq!(tool.description.as_deref(), Some("Examples:\n1: Brisbane"));
        assert_eq!(tool.input_examples.as_ref().map(Vec::len), Some(1));
    }

    #[test]
    fn add_tool_input_examples_middleware_handles_tool_without_existing_description() {
        let tool = transformed_function_tool(
            &add_tool_input_examples_middleware(),
            LanguageModelFunctionTool::new("weather", object_schema())
                .with_input_example(json_object(json!({ "city": "Berlin" }))),
        );

        assert_eq!(
            tool.description.as_deref(),
            Some("Input Examples:\n{\"city\":\"Berlin\"}")
        );
        assert_eq!(tool.input_examples, None);
    }

    #[test]
    fn add_tool_input_examples_middleware_uses_default_json_stringify_format() {
        let tool = transformed_function_tool(
            &add_tool_input_examples_middleware(),
            LanguageModelFunctionTool::new("search", object_schema())
                .with_description("Search for items")
                .with_input_example(json_object(json!({ "city": "test", "limit": 10 }))),
        );

        assert_eq!(
            tool.description.as_deref(),
            Some("Search for items\n\nInput Examples:\n{\"city\":\"test\",\"limit\":10}")
        );
    }

    #[test]
    fn add_tool_input_examples_middleware_passes_through_tools_without_input_examples() {
        let tool = LanguageModelFunctionTool::new("weather", object_schema())
            .with_description("Get the weather");
        let transformed =
            transformed_function_tool(&add_tool_input_examples_middleware(), tool.clone());

        assert_eq!(transformed, tool);
    }

    #[test]
    fn add_tool_input_examples_middleware_passes_through_tools_with_empty_input_examples() {
        let mut tool = LanguageModelFunctionTool::new("weather", object_schema())
            .with_description("Get the weather");
        tool.input_examples = Some(Vec::new());

        let transformed =
            transformed_function_tool(&add_tool_input_examples_middleware(), tool.clone());

        assert_eq!(transformed, tool);
    }

    #[test]
    fn add_tool_input_examples_middleware_passes_through_provider_tools_unchanged() {
        let provider_tool = LanguageModelProviderTool::new(
            "anthropic.web_search_20250305",
            "web_search",
            json_object(json!({ "maxUses": 5 })),
        );

        let tools = transform_tool_examples(
            &add_tool_input_examples_middleware(),
            Some(vec![LanguageModelTool::Provider(provider_tool.clone())]),
        )
        .expect("provider tool is retained");

        assert_eq!(tools, vec![LanguageModelTool::Provider(provider_tool)]);
    }

    #[test]
    fn add_tool_input_examples_middleware_handles_multiple_tools_with_mixed_examples() {
        let tools = transform_tool_examples(
            &add_tool_input_examples_middleware(),
            Some(vec![
                LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("weather", object_schema())
                        .with_description("Get the weather")
                        .with_input_example(json_object(json!({ "city": "NYC" }))),
                ),
                LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("time", object_schema())
                        .with_description("Get the current time"),
                ),
            ]),
        )
        .expect("tools are retained");

        let LanguageModelTool::Function(weather) = &tools[0] else {
            panic!("first tool should be a function tool");
        };
        let LanguageModelTool::Function(time) = &tools[1] else {
            panic!("second tool should be a function tool");
        };

        assert_eq!(
            weather.description.as_deref(),
            Some("Get the weather\n\nInput Examples:\n{\"city\":\"NYC\"}")
        );
        assert_eq!(weather.input_examples, None);
        assert_eq!(time.description.as_deref(), Some("Get the current time"));
        assert_eq!(time.input_examples, None);
    }

    #[test]
    fn add_tool_input_examples_middleware_handles_empty_tools_array() {
        let tools =
            transform_tool_examples(&add_tool_input_examples_middleware(), Some(Vec::new()));

        assert_eq!(tools, Some(Vec::new()));
    }

    #[test]
    fn add_tool_input_examples_middleware_handles_undefined_tools() {
        let tools = transform_tool_examples(&add_tool_input_examples_middleware(), None);

        assert_eq!(tools, None);
    }

    #[test]
    fn extract_json_middleware_default_transform_strips_markdown_fences() {
        assert_eq!(
            default_extract_json_transform("```json\n{\"ok\":true}\n```"),
            "{\"ok\":true}"
        );
        assert_eq!(
            default_extract_json_transform("```\n{\"ok\":true}\n```"),
            "{\"ok\":true}"
        );
        assert_eq!(
            default_extract_json_transform("{\"ok\":true}"),
            "{\"ok\":true}"
        );
    }

    #[test]
    fn extract_json_middleware_wrap_generate_should_strip_markdown_json_fence_from_text_content() {
        assert_eq!(
            extract_json_generate_text("```json\n{\"value\": \"test\"}\n```", None),
            "{\"value\": \"test\"}"
        );
    }

    #[test]
    fn extract_json_middleware_wrap_generate_should_strip_markdown_fence_without_json_tag() {
        assert_eq!(
            extract_json_generate_text("```\n{\"value\": \"test\"}\n```", None),
            "{\"value\": \"test\"}"
        );
    }

    #[test]
    fn extract_json_middleware_wrap_generate_should_leave_text_without_fences_unchanged() {
        assert_eq!(
            extract_json_generate_text("{\"value\": \"test\"}", None),
            "{\"value\": \"test\"}"
        );
    }

    #[test]
    fn extract_json_middleware_wrap_generate_should_use_custom_transform_function_when_provided() {
        assert_eq!(
            extract_json_generate_text(
                "PREFIX{\"value\": \"test\"}SUFFIX",
                Some(strip_prefix_suffix),
            ),
            "{\"value\": \"test\"}"
        );
    }

    #[test]
    fn extract_json_middleware_wrap_generate_should_preserve_non_text_content_parts() {
        let content = extract_json_generate_content(
            vec![
                LanguageModelContent::Text(LanguageModelText::new(
                    "```json\n{\"value\": \"test\"}\n```",
                )),
                LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    "{\"foo\": \"bar\"}",
                )),
            ],
            None,
        );

        assert_eq!(
            content,
            vec![
                LanguageModelContent::Text(LanguageModelText::new("{\"value\": \"test\"}")),
                LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    "{\"foo\": \"bar\"}",
                )),
            ]
        );
    }

    #[test]
    fn extract_json_middleware_transforms_generate_text_parts() {
        fn uppercase(text: &str) -> String {
            text.to_uppercase()
        }

        let model = StaticLanguageModel;
        let middleware = ExtractJsonMiddleware::new().with_transform(uppercase);
        let wrapped_generate = middleware
            .wrap_generate(LanguageModelWrapGenerateOptions::new(
                Box::new(|| Box::pin(ready(language_result("json text")))),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract JSON wrap-generate exists");
        let result = poll_boxed(wrapped_generate);

        assert_eq!(
            result.content,
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "JSON TEXT"
            ))]
        );
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_strip_markdown_json_fence_from_streamed_text() {
        let result = extract_json_stream_parts(
            text_stream("1", &["```json\n", "{\"value\": \"test\"}", "\n```"]),
            None,
        );

        assert_eq!(collect_text_deltas(&result), "{\"value\": \"test\"}");
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_strip_markdown_fence_without_json_tag() {
        let result = extract_json_stream_parts(
            text_stream("1", &["```\n", "{\"value\": \"test\"}", "\n```"]),
            None,
        );

        assert_eq!(collect_text_deltas(&result), "{\"value\": \"test\"}");
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_leave_text_without_fences_unchanged_in_stream() {
        let result = extract_json_stream_parts(text_stream("1", &["{\"value\": \"test\"}"]), None);

        assert_eq!(collect_text_deltas(&result), "{\"value\": \"test\"}");
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_handle_fence_split_across_multiple_deltas() {
        let result = extract_json_stream_parts(
            text_stream(
                "1",
                &["`", "``", "json\n", "{\"value\": \"test\"}", "\n`", "``"],
            ),
            None,
        );

        assert_eq!(collect_text_deltas(&result), "{\"value\": \"test\"}");
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_handle_content_that_starts_with_backtick_but_is_not_a_fence()
     {
        let result = extract_json_stream_parts(text_stream("1", &["`code`"]), None);

        assert_eq!(collect_text_deltas(&result), "`code`");
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_pass_through_non_text_chunks_unchanged() {
        let mut stream = text_stream("1", &["```json\n", "{\"value\": \"test\"}", "\n```"]);
        stream.push(LanguageModelStreamPart::ToolInputStart(
            LanguageModelToolInputStart::new("tool-1", "testTool"),
        ));
        stream.push(LanguageModelStreamPart::ToolInputDelta(
            LanguageModelToolInputDelta::new("tool-1", "{\"arg\": \"value\"}"),
        ));
        stream.push(LanguageModelStreamPart::ToolInputEnd(
            LanguageModelToolInputEnd::new("tool-1"),
        ));

        let result = extract_json_stream_parts(stream, None);

        assert_eq!(collect_text_deltas(&result), "{\"value\": \"test\"}");
        assert!(result.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ToolInputStart(start)
                if start.id == "tool-1" && start.tool_name == "testTool"
        )));
        assert!(result.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ToolInputDelta(delta)
                if delta.id == "tool-1" && delta.delta == "{\"arg\": \"value\"}"
        )));
        assert!(result.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ToolInputEnd(end) if end.id == "tool-1"
        )));
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_handle_multiple_text_blocks_with_different_ids() {
        let mut stream = text_stream("1", &["```json\n", "{\"first\": true}", "\n```"]);
        stream.extend(text_stream(
            "2",
            &["```json\n", "{\"second\": true}", "\n```"],
        ));

        let result = extract_json_stream_parts(stream, None);
        let all_text = collect_text_deltas(&result);

        assert!(all_text.contains("{\"first\": true}"));
        assert!(all_text.contains("{\"second\": true}"));
        assert!(!all_text.contains("```"));
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_handle_text_delta_without_prior_text_start() {
        let stream = vec![LanguageModelStreamPart::TextDelta(
            LanguageModelTextDelta::new("unknown", "some text"),
        )];

        let result = extract_json_stream_parts(stream.clone(), None);

        assert_eq!(result, stream);
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_emit_text_start_when_stream_ends_while_still_in_prefix_phase()
     {
        let result = extract_json_stream_parts(text_stream("1", &["``"]), None);

        assert_eq!(
            result,
            vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "``")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_apply_custom_transform_to_streamed_content() {
        let result = extract_json_stream_parts(
            text_stream("1", &["PREFIX", "{\"value\": \"test\"}", "SUFFIX"]),
            Some(strip_prefix_suffix),
        );

        assert_eq!(collect_text_deltas(&result), "{\"value\": \"test\"}");
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_handle_large_content_exceeding_suffix_buffer() {
        let large_json = format!(
            "{{\"data\":\"{}\",\"nested\":{{\"values\":[0,1,2,3,4,5,6,7,8,9]}}}}",
            "x".repeat(100)
        );
        let result =
            extract_json_stream_parts(text_stream("1", &["```json\n", &large_json, "\n```"]), None);

        assert_eq!(collect_text_deltas(&result), large_json);
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_handle_content_arriving_character_by_character() {
        let source = "```json\n{\"value\": \"test\"}\n```";
        let deltas = source
            .chars()
            .map(|char| char.to_string())
            .collect::<Vec<_>>();
        let delta_refs = deltas.iter().map(String::as_str).collect::<Vec<_>>();

        let result = extract_json_stream_parts(text_stream("1", &delta_refs), None);

        assert_eq!(collect_text_deltas(&result), "{\"value\": \"test\"}");
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_handle_fence_with_extra_whitespace() {
        let result = extract_json_stream_parts(
            text_stream("1", &["```json  \n", "{\"value\": \"test\"}", "\n```  "]),
            None,
        );

        assert_eq!(collect_text_deltas(&result), "{\"value\": \"test\"}");
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_verify_stream_output_matches_expected_structure()
    {
        let result = extract_json_stream_parts(
            text_stream("1", &["```json\n", "{\"value\": \"test\"}", "\n```"]),
            None,
        );

        assert!(matches!(
            result.first(),
            Some(LanguageModelStreamPart::TextStart(start)) if start.id == "1"
        ));
        assert!(matches!(
            result.last(),
            Some(LanguageModelStreamPart::TextEnd(end)) if end.id == "1"
        ));
        assert_eq!(collect_text_deltas(&result), "{\"value\": \"test\"}");
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_handle_empty_content_between_fences() {
        let result = extract_json_stream_parts(text_stream("1", &["```json\n```"]), None);

        assert_eq!(collect_text_deltas(&result), "");
        assert_eq!(
            result,
            vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn extract_json_middleware_wrap_stream_should_handle_content_starting_without_backtick_quickly_switching_to_streaming()
     {
        let result =
            extract_json_stream_parts(text_stream("1", &["{", "\"value\": \"test\"", "}"]), None);

        assert_eq!(collect_text_deltas(&result), "{\"value\": \"test\"}");
    }

    #[test]
    fn extract_json_middleware_transforms_vec_stream_text_blocks() {
        let model = StaticLanguageModel;
        let middleware = extract_json_middleware();
        let wrapped_stream = middleware
            .wrap_stream(LanguageModelWrapStreamOptions::new(
                Box::new(|| Box::pin(ready(language_result("unused")))),
                Box::new(|| {
                    Box::pin(ready(LanguageModelStreamResult::new(vec![
                        LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("0")),
                        LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                            "0",
                            "```json\n",
                        )),
                        LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                            "0",
                            "{\"ok\":true}\n```",
                        )),
                        LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("0")),
                    ])))
                }),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract JSON wrap-stream exists");
        let result = poll_boxed(wrapped_stream);

        assert_eq!(
            result.stream,
            vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("0")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "0",
                    "{\"ok\":true}"
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("0")),
            ]
        );
    }

    #[test]
    fn extract_reasoning_middleware_extracts_generate_text_tags() {
        let model = StaticLanguageModel;
        let middleware = extract_reasoning_middleware("think");
        let wrapped_generate = middleware
            .wrap_generate(LanguageModelWrapGenerateOptions::new(
                Box::new(|| {
                    Box::pin(ready(LanguageModelGenerateResult::new(
                        vec![LanguageModelContent::Text(LanguageModelText::new(
                            "<think>analyzing</think>Here<think>more</think>done",
                        ))],
                        LanguageModelFinishReason {
                            unified: FinishReason::Stop,
                            raw: None,
                        },
                        LanguageModelUsage::default(),
                    )))
                }),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract reasoning wrap-generate exists");
        let result = poll_boxed(wrapped_generate);

        assert_eq!(
            result.content,
            vec![
                LanguageModelContent::Reasoning(LanguageModelReasoning::new("analyzing\nmore")),
                LanguageModelContent::Text(LanguageModelText::new("Here\ndone")),
            ]
        );
    }

    #[test]
    fn extract_reasoning_middleware_wrap_generate_should_extract_reasoning_from_think_tags() {
        assert_eq!(
            extract_reasoning_generate_content(
                "<think>analyzing the request</think>Here is the response",
                false,
            ),
            vec![
                LanguageModelContent::Reasoning(LanguageModelReasoning::new(
                    "analyzing the request"
                )),
                LanguageModelContent::Text(LanguageModelText::new("Here is the response")),
            ]
        );
    }

    #[test]
    fn extract_reasoning_middleware_wrap_generate_should_extract_reasoning_from_think_tags_when_there_is_no_text()
     {
        assert_eq!(
            extract_reasoning_generate_content("<think>analyzing the request\n</think>", false),
            vec![
                LanguageModelContent::Reasoning(LanguageModelReasoning::new(
                    "analyzing the request\n"
                )),
                LanguageModelContent::Text(LanguageModelText::new("")),
            ]
        );
    }

    #[test]
    fn extract_reasoning_middleware_wrap_generate_should_extract_reasoning_from_multiple_think_tags()
     {
        assert_eq!(
            extract_reasoning_generate_content(
                "<think>analyzing the request</think>Here is the response<think>thinking about the response</think>more",
                false,
            ),
            vec![
                LanguageModelContent::Reasoning(LanguageModelReasoning::new(
                    "analyzing the request\nthinking about the response"
                )),
                LanguageModelContent::Text(LanguageModelText::new("Here is the response\nmore")),
            ]
        );
    }

    #[test]
    fn extract_reasoning_middleware_wrap_generate_should_prepend_think_tag_iff_start_with_reasoning_is_true()
     {
        assert_eq!(
            extract_reasoning_generate_content(
                "analyzing the request</think>Here is the response",
                true
            ),
            vec![
                LanguageModelContent::Reasoning(LanguageModelReasoning::new(
                    "analyzing the request"
                )),
                LanguageModelContent::Text(LanguageModelText::new("Here is the response")),
            ]
        );
        assert_eq!(
            extract_reasoning_generate_content(
                "analyzing the request</think>Here is the response",
                false,
            ),
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "analyzing the request</think>Here is the response"
            ))]
        );
    }

    #[test]
    fn extract_reasoning_middleware_wrap_generate_should_preserve_reasoning_property_even_when_rest_contains_other_properties()
     {
        let content = extract_reasoning_generate_content(
            "<think>analyzing the request</think>Here is the response",
            false,
        );

        assert!(matches!(content[0], LanguageModelContent::Reasoning(_)));
        assert!(matches!(content[1], LanguageModelContent::Text(_)));
    }

    #[test]
    fn extract_reasoning_middleware_supports_start_with_reasoning_for_generate() {
        let model = StaticLanguageModel;
        let middleware = ExtractReasoningMiddleware::new("think").with_start_with_reasoning(true);
        let wrapped_generate = middleware
            .wrap_generate(LanguageModelWrapGenerateOptions::new(
                Box::new(|| {
                    Box::pin(ready(LanguageModelGenerateResult::new(
                        vec![LanguageModelContent::Text(LanguageModelText::new(
                            "analyzing</think>Here",
                        ))],
                        LanguageModelFinishReason {
                            unified: FinishReason::Stop,
                            raw: None,
                        },
                        LanguageModelUsage::default(),
                    )))
                }),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract reasoning wrap-generate exists");
        let result = poll_boxed(wrapped_generate);

        assert_eq!(
            result.content,
            vec![
                LanguageModelContent::Reasoning(LanguageModelReasoning::new("analyzing")),
                LanguageModelContent::Text(LanguageModelText::new("Here")),
            ]
        );

        let middleware = extract_reasoning_middleware("think");
        let wrapped_generate = middleware
            .wrap_generate(LanguageModelWrapGenerateOptions::new(
                Box::new(|| Box::pin(ready(language_result("analyzing</think>Here")))),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract reasoning wrap-generate exists");
        let result = poll_boxed(wrapped_generate);

        assert_eq!(
            result.content,
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "analyzing</think>Here"
            ))]
        );
    }

    #[test]
    fn extract_reasoning_middleware_wrap_stream_should_extract_reasoning_from_split_think_tags() {
        let result = extract_reasoning_stream_parts(
            &[
                "<think>",
                "ana",
                "lyzing the request",
                "</think>",
                "Here",
                " is the response",
            ],
            false,
        );

        assert_eq!(collect_reasoning_deltas(&result), "analyzing the request");
        assert_eq!(collect_text_deltas(&result), "Here is the response");
    }

    #[test]
    fn extract_reasoning_middleware_wrap_stream_should_extract_reasoning_from_single_chunk_with_multiple_think_tags()
     {
        let result = extract_reasoning_stream_parts(
            &[
                "<think>analyzing the request</think>Here is the response<think>thinking about the response</think>more",
            ],
            false,
        );

        assert_eq!(
            collect_reasoning_deltas(&result),
            "analyzing the request\nthinking about the response"
        );
        assert_eq!(collect_text_deltas(&result), "Here is the response\nmore");
    }

    #[test]
    fn extract_reasoning_middleware_wrap_stream_should_extract_reasoning_from_think_when_there_is_no_text()
     {
        let result = extract_reasoning_stream_parts(
            &["<think>", "ana", "lyzing the request\n", "</think>"],
            false,
        );

        assert_eq!(collect_reasoning_deltas(&result), "analyzing the request\n");
        assert_eq!(collect_text_deltas(&result), "");
        assert!(
            result
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::TextStart(_)))
        );
        assert!(
            result
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::TextEnd(_)))
        );
    }

    #[test]
    fn extract_reasoning_middleware_wrap_stream_should_prepend_think_tag_if_start_with_reasoning_is_true()
     {
        let result_true = extract_reasoning_stream_parts(
            &[
                "ana",
                "lyzing the request\n",
                "</think>",
                "this is the response",
            ],
            true,
        );
        let result_false = extract_reasoning_stream_parts(
            &[
                "ana",
                "lyzing the request\n",
                "</think>",
                "this is the response",
            ],
            false,
        );

        assert_eq!(
            collect_reasoning_deltas(&result_true),
            "analyzing the request\n"
        );
        assert_eq!(collect_text_deltas(&result_true), "this is the response");
        assert_eq!(
            collect_text_deltas(&result_false),
            "analyzing the request\n</think>this is the response"
        );
        assert_eq!(collect_reasoning_deltas(&result_false), "");
    }

    #[test]
    fn extract_reasoning_middleware_extracts_split_stream_tags() {
        let model = StaticLanguageModel;
        let middleware = extract_reasoning_middleware("think");
        let wrapped_stream = middleware
            .wrap_stream(LanguageModelWrapStreamOptions::new(
                Box::new(|| Box::pin(ready(language_result("unused")))),
                Box::new(|| {
                    Box::pin(ready(LanguageModelStreamResult::new(vec![
                        LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                        LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                            "1", "<thi",
                        )),
                        LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                            "1", "nk>ana",
                        )),
                        LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                            "1",
                            "lyzing</think>Here",
                        )),
                        LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                    ])))
                }),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract reasoning wrap-stream exists");
        let result = poll_boxed(wrapped_stream);

        assert_eq!(
            result.stream,
            vec![
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new(
                    "reasoning-0"
                )),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "reasoning-0",
                    "ana"
                )),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "reasoning-0",
                    "lyzing"
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new(
                    "reasoning-0"
                )),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Here")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn extract_reasoning_middleware_wrap_stream_should_keep_original_text_when_think_tag_is_not_present()
     {
        let result = extract_reasoning_stream_parts(&["this is the response"], false);

        assert_eq!(collect_text_deltas(&result), "this is the response");
        assert_eq!(collect_reasoning_deltas(&result), "");
    }

    #[test]
    fn extract_reasoning_middleware_wrap_stream_should_handle_empty_think_tags_without_crashing() {
        let result =
            extract_reasoning_stream_parts(&["<think></think>", " This is the answer."], false);

        let reasoning_start_index = result.iter().position(|part| {
            matches!(
                part,
                LanguageModelStreamPart::ReasoningStart(start) if start.id == "reasoning-0"
            )
        });
        let reasoning_end_index = result.iter().position(|part| {
            matches!(
                part,
                LanguageModelStreamPart::ReasoningEnd(end) if end.id == "reasoning-0"
            )
        });

        assert!(reasoning_start_index.is_some());
        assert!(reasoning_end_index.is_some());
        assert!(reasoning_end_index > reasoning_start_index);
        assert_eq!(collect_text_deltas(&result), " This is the answer.");
    }

    #[test]
    fn extract_reasoning_middleware_separates_multiple_stream_tags() {
        let model = StaticLanguageModel;
        let middleware = extract_reasoning_middleware("think");
        let wrapped_stream = middleware
            .wrap_stream(LanguageModelWrapStreamOptions::new(
                Box::new(|| Box::pin(ready(language_result("unused")))),
                Box::new(|| {
                    Box::pin(ready(LanguageModelStreamResult::new(vec![
                        LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                        LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                            "1",
                            "<think>first</think>text<think>second</think>more",
                        )),
                        LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                    ])))
                }),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("extract reasoning wrap-stream exists");
        let result = poll_boxed(wrapped_stream);

        assert_eq!(
            result.stream,
            vec![
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new(
                    "reasoning-0"
                )),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "reasoning-0",
                    "first"
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new(
                    "reasoning-0"
                )),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "text")),
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new(
                    "reasoning-1"
                )),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "reasoning-1",
                    "\nsecond"
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new(
                    "reasoning-1"
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\nmore")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn simulate_streaming_middleware_turns_generate_result_into_vec_stream() {
        let model = StaticLanguageModel;
        let middleware = simulate_streaming_middleware();
        let finish_reason = LanguageModelFinishReason {
            unified: FinishReason::Stop,
            raw: None,
        };
        let response = LanguageModelResponse::new()
            .with_id("response-id")
            .with_model_id("model-id")
            .with_header("x-test", "yes");

        let wrapped_stream = middleware
            .wrap_stream(LanguageModelWrapStreamOptions::new(
                Box::new(|| {
                    Box::pin(ready(
                        LanguageModelGenerateResult::new(
                            vec![
                                LanguageModelContent::Text(LanguageModelText::new("hello")),
                                LanguageModelContent::Reasoning(LanguageModelReasoning::new(
                                    "thinking",
                                )),
                            ],
                            finish_reason.clone(),
                            LanguageModelUsage::default(),
                        )
                        .with_response(response.clone())
                        .with_warning(Warning::Other {
                            message: "simulated".to_string(),
                        }),
                    ))
                }),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("simulate streaming wrap-stream exists");
        let result = poll_boxed(wrapped_stream);

        assert_eq!(
            result.stream,
            vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                    Warning::Other {
                        message: "simulated".to_string()
                    }
                ])),
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("response-id")
                        .with_model_id("model-id")
                ),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("0")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("0", "hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("0")),
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "1", "thinking"
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    LanguageModelUsage::default(),
                    finish_reason,
                )),
            ]
        );
        assert_eq!(
            result.response.expect("stream response exists").headers,
            Some(BTreeMap::from([("x-test".to_string(), "yes".to_string())]))
        );
    }

    #[test]
    fn language_model_middleware_exposes_upstream_v4_hooks() {
        let model = StaticLanguageModel;
        let middleware = StaticLanguageMiddleware;

        assert_eq!(middleware.specification_version(), SpecificationVersion::V4);
        assert_eq!(
            middleware.override_provider(LanguageModelMiddlewareModelOptions::new(&model)),
            Some("base-provider-middleware".to_string())
        );
        assert_eq!(
            middleware.override_model_id(LanguageModelMiddlewareModelOptions::new(&model)),
            Some("language-base-wrapped".to_string())
        );

        let supported_urls = middleware
            .override_supported_urls(LanguageModelMiddlewareModelOptions::new(&model))
            .expect("supported URLs hook is implemented");
        assert_eq!(
            poll_ready(supported_urls),
            BTreeMap::from([("application/pdf".to_string(), vec!["\\.pdf$".to_string()])])
        );

        let transformed_generate = middleware
            .transform_params(LanguageModelTransformParamsOptions::new(
                LanguageModelMiddlewareCallType::Generate,
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("generate transform hook is implemented");
        assert_eq!(
            poll_ready(transformed_generate).max_output_tokens,
            Some(128)
        );

        let transformed_stream = middleware
            .transform_params(LanguageModelTransformParamsOptions::new(
                LanguageModelMiddlewareCallType::Stream,
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("stream transform hook is implemented");
        assert_eq!(
            poll_ready(transformed_stream).include_raw_chunks,
            Some(true)
        );

        let wrapped_generate = middleware
            .wrap_generate(LanguageModelWrapGenerateOptions::new(
                Box::new(|| Box::pin(ready(language_result("wrapped")))),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()).with_max_output_tokens(32),
                &model,
            ))
            .expect("wrap-generate hook is implemented");
        let generate_result = poll_boxed(wrapped_generate);

        assert_eq!(
            generate_result.warnings,
            vec![Warning::Other {
                message: "wrapped-generate".to_string(),
            }]
        );

        let wrapped_stream = middleware
            .wrap_stream(LanguageModelWrapStreamOptions::new(
                Box::new(|| Box::pin(ready(language_result("fallback")))),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()).with_include_raw_chunks(true),
                &model,
            ))
            .expect("wrap-stream hook is implemented");
        let stream_result = poll_boxed(wrapped_stream);

        assert_eq!(stream_result.stream.len(), 1);
        assert_eq!(
            stream_result.stream,
            vec![LanguageModelStreamPart::StreamStart(
                LanguageModelStreamStart::new(vec![Warning::Other {
                    message: "wrapped-stream".to_string(),
                }],)
            )]
        );
    }

    #[test]
    fn language_model_middleware_hooks_are_optional_by_default() {
        let model = StaticLanguageModel;
        let middleware = NoopLanguageMiddleware;

        assert_eq!(
            middleware.override_provider(LanguageModelMiddlewareModelOptions::new(&model)),
            None
        );
        assert_eq!(
            middleware.override_model_id(LanguageModelMiddlewareModelOptions::new(&model)),
            None
        );
        assert!(
            middleware
                .override_supported_urls(LanguageModelMiddlewareModelOptions::new(&model))
                .is_none()
        );
        assert!(
            middleware
                .transform_params(LanguageModelTransformParamsOptions::new(
                    LanguageModelMiddlewareCallType::Generate,
                    LanguageModelCallOptions::new(Vec::new()),
                    &model,
                ))
                .is_none()
        );
        assert!(
            middleware
                .wrap_generate(LanguageModelWrapGenerateOptions::new(
                    Box::new(|| Box::pin(ready(language_result("base")))),
                    Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                    LanguageModelCallOptions::new(Vec::new()),
                    &model,
                ))
                .is_none()
        );
        assert!(
            middleware
                .wrap_stream(LanguageModelWrapStreamOptions::new(
                    Box::new(|| Box::pin(ready(language_result("base")))),
                    Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                    LanguageModelCallOptions::new(Vec::new()),
                    &model,
                ))
                .is_none()
        );
    }

    #[test]
    fn wrap_language_model_applies_identity_and_supported_url_overrides() {
        let wrapped = wrap_language_model(StaticLanguageModel, StaticLanguageMiddleware);

        assert_eq!(wrapped.specification_version(), SpecificationVersion::V4);
        assert_eq!(wrapped.provider(), "base-provider-middleware");
        assert_eq!(wrapped.model_id(), "language-base-wrapped");
        assert_eq!(
            poll_boxed(wrapped.supported_urls()),
            BTreeMap::from([("application/pdf".to_string(), vec!["\\.pdf$".to_string()])])
        );

        let explicit = wrap_language_model(StaticLanguageModel, StaticLanguageMiddleware)
            .with_provider_id("explicit-provider")
            .with_model_id("explicit-language");

        assert_eq!(explicit.provider(), "explicit-provider");
        assert_eq!(explicit.model_id(), "explicit-language");
    }

    #[test]
    fn wrap_language_model_model_property_should_pass_through_by_default() {
        let wrapped = wrap_language_model(StaticLanguageModel, NoopLanguageMiddleware);

        assert_eq!(wrapped.model_id(), "language-base");
    }

    #[test]
    fn wrap_language_model_model_property_should_use_middleware_override_model_id_if_provided() {
        let wrapped = wrap_language_model(StaticLanguageModel, StaticLanguageMiddleware);

        assert_eq!(wrapped.model_id(), "language-base-wrapped");
    }

    #[test]
    fn wrap_language_model_model_property_should_use_model_id_parameter_if_provided() {
        let wrapped = wrap_language_model(StaticLanguageModel, NoopLanguageMiddleware)
            .with_model_id("override-model");

        assert_eq!(wrapped.model_id(), "override-model");
    }

    #[test]
    fn wrap_language_model_provider_property_should_pass_through_by_default() {
        let wrapped = wrap_language_model(StaticLanguageModel, NoopLanguageMiddleware);

        assert_eq!(wrapped.provider(), "base-provider");
    }

    #[test]
    fn wrap_language_model_provider_property_should_use_middleware_override_provider_if_provided() {
        let wrapped = wrap_language_model(StaticLanguageModel, StaticLanguageMiddleware);

        assert_eq!(wrapped.provider(), "base-provider-middleware");
    }

    #[test]
    fn wrap_language_model_provider_property_should_use_provider_id_parameter_if_provided() {
        let wrapped = wrap_language_model(StaticLanguageModel, NoopLanguageMiddleware)
            .with_provider_id("override-provider");

        assert_eq!(wrapped.provider(), "override-provider");
    }

    #[test]
    fn wrap_language_model_supported_urls_property_should_pass_through_by_default() {
        let wrapped = wrap_language_model(StaticLanguageModel, NoopLanguageMiddleware);

        assert_eq!(
            poll_boxed(wrapped.supported_urls()),
            BTreeMap::from([(
                "image/*".to_string(),
                vec!["^https://base\\.example/images/".to_string()]
            )])
        );
    }

    #[test]
    fn wrap_language_model_supported_urls_property_should_use_middleware_override_if_provided() {
        let wrapped = wrap_language_model(StaticLanguageModel, StaticLanguageMiddleware);

        assert_eq!(
            poll_boxed(wrapped.supported_urls()),
            BTreeMap::from([("application/pdf".to_string(), vec!["\\.pdf$".to_string()])])
        );
    }

    #[test]
    fn wrap_language_model_should_call_transform_params_middleware_for_do_generate() {
        let wrapped = wrap_language_model(ParamEchoLanguageModel, AddMaxOutputTokensMiddleware(5));

        let result = poll_boxed(wrapped.do_generate(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "generated max 5".to_string(),
            }]
        );
    }

    #[test]
    fn wrap_language_model_should_call_wrap_generate_middleware() {
        let wrapped = wrap_language_model(
            ParamEchoLanguageModel,
            AppendGenerateWarningMiddleware("wrapped generate"),
        );

        let result = poll_boxed(wrapped.do_generate(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "generated max 0".to_string(),
                },
                Warning::Other {
                    message: "wrapped generate".to_string(),
                },
            ]
        );
    }

    #[test]
    fn wrap_language_model_should_call_transform_params_middleware_for_do_stream() {
        let wrapped = wrap_language_model(ParamEchoLanguageModel, AddMaxOutputTokensMiddleware(7));

        let result = poll_boxed(wrapped.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(
            result.stream,
            vec![LanguageModelStreamPart::StreamStart(
                LanguageModelStreamStart::new(vec![Warning::Other {
                    message: "stream max 7 raw false".to_string(),
                }])
            )]
        );
    }

    #[test]
    fn wrap_language_model_should_call_wrap_stream_middleware() {
        let wrapped = wrap_language_model(
            ParamEchoLanguageModel,
            AppendStreamWarningMiddleware("wrapped stream"),
        );

        let result = poll_boxed(wrapped.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(
            result.stream,
            vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                    Warning::Other {
                        message: "stream raw false".to_string(),
                    }
                ])),
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                    Warning::Other {
                        message: "wrapped stream".to_string(),
                    }
                ])),
            ]
        );
    }

    #[test]
    fn wrap_language_model_should_support_models_that_use_context_in_supported_urls() {
        let supported_urls = BTreeMap::from([(
            "image/*".to_string(),
            vec!["^https://stateful\\.example/".to_string()],
        )]);
        let wrapped = wrap_language_model(
            StatefulSupportedUrlsLanguageModel {
                urls: supported_urls.clone(),
            },
            NoopLanguageMiddleware,
        );

        assert_eq!(poll_boxed(wrapped.supported_urls()), supported_urls);
    }

    #[test]
    fn wrap_language_model_should_call_multiple_transform_params_middlewares_in_sequence_for_do_generate()
     {
        let first = wrap_language_model(ParamEchoLanguageModel, AddMaxOutputTokensMiddleware(2));
        let second = wrap_language_model(first, AddMaxOutputTokensMiddleware(3));

        let result = poll_boxed(second.do_generate(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "generated max 5".to_string(),
            }]
        );
    }

    #[test]
    fn wrap_language_model_should_call_multiple_transform_params_middlewares_in_sequence_for_do_stream()
     {
        let first = wrap_language_model(ParamEchoLanguageModel, AddMaxOutputTokensMiddleware(2));
        let second = wrap_language_model(first, AddMaxOutputTokensMiddleware(3));

        let result = poll_boxed(second.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(
            result.stream,
            vec![LanguageModelStreamPart::StreamStart(
                LanguageModelStreamStart::new(vec![Warning::Other {
                    message: "stream max 5 raw false".to_string(),
                }])
            )]
        );
    }

    #[test]
    fn wrap_language_model_should_chain_multiple_wrap_generate_middlewares_in_the_correct_order() {
        let first = wrap_language_model(
            ParamEchoLanguageModel,
            AppendGenerateWarningMiddleware("wrap1"),
        );
        let second = wrap_language_model(first, AppendGenerateWarningMiddleware("wrap2"));

        let result = poll_boxed(second.do_generate(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "generated max 0".to_string(),
                },
                Warning::Other {
                    message: "wrap1".to_string(),
                },
                Warning::Other {
                    message: "wrap2".to_string(),
                },
            ]
        );
    }

    #[test]
    fn wrap_language_model_should_chain_multiple_wrap_stream_middlewares_in_the_correct_order() {
        let first = wrap_language_model(
            ParamEchoLanguageModel,
            AppendStreamWarningMiddleware("wrap1"),
        );
        let second = wrap_language_model(first, AppendStreamWarningMiddleware("wrap2"));

        let result = poll_boxed(second.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(
            result.stream,
            vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                    Warning::Other {
                        message: "stream raw false".to_string(),
                    }
                ])),
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                    Warning::Other {
                        message: "wrap1".to_string(),
                    }
                ])),
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                    Warning::Other {
                        message: "wrap2".to_string(),
                    }
                ])),
            ]
        );
    }

    #[test]
    fn wrap_language_model_transforms_params_before_wrapping_generate_and_stream() {
        let wrapped =
            wrap_language_model(ParamEchoLanguageModel, TransformAndWrapLanguageMiddleware);

        let generate_result =
            poll_boxed(wrapped.do_generate(LanguageModelCallOptions::new(Vec::new())));
        assert_eq!(
            generate_result.warnings,
            vec![
                Warning::Other {
                    message: "generated max 77".to_string()
                },
                Warning::Other {
                    message: "wrapped-generate".to_string()
                }
            ]
        );

        let stream_result =
            poll_boxed(wrapped.do_stream(LanguageModelCallOptions::new(Vec::new())));
        assert_eq!(
            stream_result.stream,
            vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                    Warning::Other {
                        message: "stream raw true".to_string()
                    }
                ])),
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                    Warning::Other {
                        message: "wrapped-stream".to_string()
                    }
                ]))
            ]
        );
    }
}
