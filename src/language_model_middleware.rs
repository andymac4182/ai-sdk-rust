use std::future::{Future, Pending, Ready, ready};
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    LanguageModel, LanguageModelCallOptions, LanguageModelGenerateResult,
    LanguageModelResponseFormat, LanguageModelStreamResult, LanguageModelSupportedUrls,
    LanguageModelTool, LanguageModelToolChoice, LanguageModelToolInputExample,
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
        AddToolInputExamplesMiddleware, LanguageModelDefaultSettings, LanguageModelMiddleware,
        LanguageModelMiddlewareCallType, LanguageModelMiddlewareModelOptions,
        LanguageModelTransformParamsOptions, LanguageModelWrapGenerateOptions,
        LanguageModelWrapStreamOptions, add_tool_input_examples_middleware,
        default_settings_middleware, wrap_language_model,
    };
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFinishReason, LanguageModelFunctionTool, LanguageModelGenerateResult,
        LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelStreamStart,
        LanguageModelSupportedUrls, LanguageModelText, LanguageModelTool, LanguageModelUsage,
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
            let message = format!(
                "stream raw {:?}",
                options.include_raw_chunks.unwrap_or(false)
            );
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
        struct NoopMiddleware;

        impl LanguageModelMiddleware<StaticLanguageModel> for NoopMiddleware {
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
                = Ready<LanguageModelGenerateResult>
            where
                Self: 'a,
                StaticLanguageModel: 'a;

            type WrapStreamFuture<'a>
                = Ready<LanguageModelStreamResult<Vec<LanguageModelStreamPart>>>
            where
                Self: 'a,
                StaticLanguageModel: 'a;
        }

        let model = StaticLanguageModel;
        let middleware = NoopMiddleware;

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
