//! Anthropic provider helpers for the Rust port of upstream `@ai-sdk/anthropic`.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_openai_compatible::{OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel};
use ai_sdk_provider::{
    FileData, FileDataContent, Files, FilesUploadFileCallOptions, FilesUploadFileData,
    FilesUploadFileResult, FinishReason, Headers, InputTokenUsage, InvalidArgumentError,
    JsonObject, JsonSchema, JsonValue, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelCustomContent,
    LanguageModelErrorStreamPart, LanguageModelFilePart, LanguageModelFinishReason,
    LanguageModelFunctionTool, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelProviderTool, LanguageModelRawStreamPart, LanguageModelReasoning,
    LanguageModelReasoningDelta, LanguageModelReasoningEffort, LanguageModelReasoningEnd,
    LanguageModelReasoningStart, LanguageModelRequest, LanguageModelResponse,
    LanguageModelResponseFormat, LanguageModelSource, LanguageModelStreamFinish,
    LanguageModelStreamPart, LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextPart,
    LanguageModelTextStart, LanguageModelTool, LanguageModelToolCall, LanguageModelToolCallPart,
    LanguageModelToolChoice, LanguageModelToolContentPart, LanguageModelToolInputDelta,
    LanguageModelToolInputEnd, LanguageModelToolInputStart, LanguageModelToolResult,
    LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUsage,
    LanguageModelUserContentPart, ModelType, NoSuchModelError, NonNullJsonValue, OutputTokenUsage,
    Provider, ProviderMetadata, ProviderOptions, ProviderReference, ProviderWithFiles,
    ProviderWithSkills, Skills, SkillsFileData, SkillsUploadSkillCallOptions,
    SkillsUploadSkillResult, Warning,
};
use ai_sdk_provider_utils::{
    FetchErrorInfo, FormData, FormDataValue, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ReasoningLevel, convert_base64_to_bytes,
    convert_to_base64, get_top_level_media_type, map_reasoning_to_provider_budget,
    map_reasoning_to_provider_effort, prepare_post_form_data_to_api_request,
    prepare_post_json_to_api_request, with_user_agent_suffix, without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// Upstream package covered by this crate.
pub const UPSTREAM_PACKAGE: &str = "@ai-sdk/anthropic";

/// Upstream package directory in `vercel/ai`.
pub const UPSTREAM_PACKAGE_DIR: &str = "packages/anthropic";

/// Upstream commit used for the checked-in AI-01 inventory.
pub const UPSTREAM_COMMIT: &str = "ab6d66482d31afe15f4973a51c5f7cfa09c92ea6";

/// Checked-in row-level inventory document for this package.
pub const INVENTORY_DOCUMENT: &str = "docs/ai-foundational-provider-inventory.md";

/// Current upstream test files under `packages/anthropic/src`.
pub const UPSTREAM_TEST_FILES: usize = 13;

/// Current detected upstream `it`/`test` cases under `packages/anthropic/src`.
pub const UPSTREAM_TEST_CASES: usize = 424;

/// Current explicit TypeScript type-system exceptions.
pub const TYPE_SYSTEM_IMPOSSIBLE_CASES: usize = 6;

/// Current explicit JavaScript runtime exceptions.
pub const JS_ONLY_DOCUMENTED_CASES: usize = 0;

/// Current portable cases mapped to named Rust tests in this crate.
pub const PORTABLE_MAPPED_CASES: usize =
    UPSTREAM_TEST_CASES - TYPE_SYSTEM_IMPOSSIBLE_CASES - JS_ONLY_DOCUMENTED_CASES;

/// Current portable cases still requiring named Rust tests.
pub const PORTABLE_UNMAPPED_CASES: usize = 0;

/// The crate version compiled into Anthropic request user-agent suffixes.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default base URL used by upstream `@ai-sdk/anthropic`.
pub const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Future returned by an injected Anthropic HTTP transport.
pub type AnthropicTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Anthropic models, files, and skills.
pub type AnthropicTransport =
    Arc<dyn Fn(ProviderApiRequest) -> AnthropicTransportFuture + Send + Sync>;

/// Settings for creating an Anthropic provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicProviderSettings {
    /// Base URL for Anthropic API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// API key sent with the `x-api-key` header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Auth token sent with the `Authorization: Bearer` header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    /// Custom provider name. Defaults to `anthropic.messages`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl AnthropicProviderSettings {
    /// Creates empty Anthropic provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Anthropic API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Anthropic API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the Anthropic auth token.
    pub fn with_auth_token(mut self, auth_token: impl Into<String>) -> Self {
        self.auth_token = Some(auth_token.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Sets the provider name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// Upstream Anthropic provider facade.
#[derive(Clone)]
pub struct AnthropicProvider {
    settings: AnthropicProviderSettings,
    transport: AnthropicTransport,
}

impl AnthropicProvider {
    /// Creates an Anthropic provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(AnthropicProviderSettings::new())
    }

    /// Creates a provider from settings, panicking only when both auth methods are set.
    pub fn from_settings(settings: AnthropicProviderSettings) -> Self {
        Self::try_from_settings(settings).expect("Anthropic provider settings are valid")
    }

    /// Creates a provider from settings and reports mutually exclusive auth methods.
    pub fn try_from_settings(
        settings: AnthropicProviderSettings,
    ) -> Result<Self, InvalidArgumentError> {
        validate_auth_settings(&settings)?;
        Ok(Self {
            settings,
            transport: default_anthropic_transport(),
        })
    }

    /// Replaces the HTTP transport. This is primarily useful for deterministic tests.
    pub fn with_transport(mut self, transport: AnthropicTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Creates an Anthropic language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> AnthropicLanguageModel {
        self.chat(model_id)
    }

    /// Creates an Anthropic chat/messages model.
    pub fn chat(&self, model_id: impl Into<String>) -> AnthropicLanguageModel {
        self.messages(model_id)
    }

    /// Creates an Anthropic Messages API language model.
    pub fn messages(&self, model_id: impl Into<String>) -> AnthropicLanguageModel {
        AnthropicLanguageModel::new(
            model_id.into(),
            AnthropicLanguageModelConfig {
                provider: self.provider_name(),
                base_url: anthropic_base_url(&self.settings),
                headers: anthropic_headers(&self.settings),
                transport: Arc::clone(&self.transport),
                supports_native_structured_output: true,
                supports_strict_tools: true,
            },
        )
    }

    /// Reports that Anthropic does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`AnthropicProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Anthropic does not expose image-generation models here.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    /// Returns the Anthropic files interface.
    pub fn files(&self) -> AnthropicFiles {
        AnthropicFiles {
            provider: self.provider_name(),
            base_url: anthropic_base_url(&self.settings),
            headers: anthropic_headers(&self.settings),
            transport: Arc::clone(&self.transport),
        }
    }

    /// Returns the Anthropic skills interface.
    pub fn skills(&self) -> AnthropicSkills {
        AnthropicSkills {
            provider: self.provider_name().replace(".messages", ".skills"),
            base_url: anthropic_base_url(&self.settings),
            headers: anthropic_headers(&self.settings),
            transport: Arc::clone(&self.transport),
        }
    }

    /// Returns factories for Anthropic provider-defined tools.
    pub fn tools(&self) -> AnthropicTools {
        AnthropicTools
    }

    fn provider_name(&self) -> String {
        self.settings
            .name
            .clone()
            .unwrap_or_else(|| "anthropic.messages".to_string())
    }
}

impl Default for AnthropicProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for AnthropicProvider {
    type LanguageModel = AnthropicLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(AnthropicProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        AnthropicProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        AnthropicProvider::image_model(self, model_id)
    }
}

impl ProviderWithFiles for AnthropicProvider {
    type Files = AnthropicFiles;

    fn files(&self) -> Self::Files {
        AnthropicProvider::files(self)
    }
}

impl ProviderWithSkills for AnthropicProvider {
    type Skills = AnthropicSkills;

    fn skills(&self) -> Self::Skills {
        AnthropicProvider::skills(self)
    }
}

/// Creates an Anthropic provider with explicit settings.
pub fn create_anthropic(
    settings: AnthropicProviderSettings,
) -> Result<AnthropicProvider, InvalidArgumentError> {
    AnthropicProvider::try_from_settings(settings)
}

/// Creates an Anthropic language model using default provider settings.
pub fn anthropic(model_id: impl Into<String>) -> AnthropicLanguageModel {
    AnthropicProvider::new().language_model(model_id)
}

fn validate_auth_settings(
    settings: &AnthropicProviderSettings,
) -> Result<(), InvalidArgumentError> {
    if settings.api_key.is_some() && settings.auth_token.is_some() {
        return Err(InvalidArgumentError::new(
            "apiKey/authToken",
            "Both apiKey and authToken were provided. Please use only one authentication method.",
        ));
    }

    Ok(())
}

fn anthropic_base_url(settings: &AnthropicProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .or_else(|| non_empty_optional_setting(env::var("ANTHROPIC_BASE_URL").ok()))
        .unwrap_or_else(|| DEFAULT_ANTHROPIC_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn anthropic_headers(settings: &AnthropicProviderSettings) -> Headers {
    let mut entries = Vec::<(String, Option<String>)>::new();
    entries.push((
        "anthropic-version".to_string(),
        Some(ANTHROPIC_VERSION.to_string()),
    ));

    if let Some(auth_token) = non_empty_optional_setting(settings.auth_token.clone())
        .or_else(|| non_empty_optional_setting(env::var("ANTHROPIC_AUTH_TOKEN").ok()))
    {
        entries.push((
            "authorization".to_string(),
            Some(format!("Bearer {auth_token}")),
        ));
    } else if let Some(api_key) = non_empty_optional_setting(settings.api_key.clone())
        .or_else(|| non_empty_optional_setting(env::var("ANTHROPIC_API_KEY").ok()))
    {
        entries.push(("x-api-key".to_string(), Some(api_key)));
    }

    for (name, value) in &settings.headers {
        entries.push((name.clone(), Some(value.clone())));
    }

    with_user_agent_suffix(Some(entries), [format!("ai-sdk/anthropic/{VERSION}")])
}

fn optional_headers(headers: &Headers) -> Vec<(String, Option<String>)> {
    headers
        .iter()
        .map(|(name, value)| (name.clone(), Some(value.clone())))
        .collect()
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[derive(Clone)]
struct AnthropicLanguageModelConfig {
    provider: String,
    base_url: String,
    headers: Headers,
    transport: AnthropicTransport,
    supports_native_structured_output: bool,
    supports_strict_tools: bool,
}

/// Anthropic Messages API language model.
#[derive(Clone)]
pub struct AnthropicLanguageModel {
    model_id: String,
    config: AnthropicLanguageModelConfig,
}

impl AnthropicLanguageModel {
    /// Creates a model with explicit config.
    fn new(model_id: String, config: AnthropicLanguageModelConfig) -> Self {
        Self { model_id, config }
    }

    /// Returns whether the model accepts the supplied URL natively.
    pub fn supports_url(&self, url: &url::Url) -> bool {
        url.scheme() == "https"
    }

    /// Builds the Anthropic JSON request without performing HTTP.
    pub fn request_plan(
        &self,
        options: &LanguageModelCallOptions,
        stream: bool,
    ) -> AnthropicRequestPlan {
        let mut betas = betas_from_headers(&self.config.headers);
        if let Some(headers) = &options.headers {
            betas.extend(betas_from_headers(headers));
        }

        let mut warnings = Vec::new();
        let anthropic_options = merged_anthropic_options(
            options.provider_options.as_ref(),
            &provider_options_name(&self.config.provider),
        );
        if options.frequency_penalty.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "frequencyPenalty".to_string(),
                details: None,
            });
        }
        if options.presence_penalty.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "presencePenalty".to_string(),
                details: None,
            });
        }
        if options.seed.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "seed".to_string(),
                details: None,
            });
        }

        let capabilities = get_model_capabilities(&self.model_id);
        let prompt_plan = convert_to_anthropic_prompt(AnthropicPromptConversionOptions {
            prompt: &options.prompt,
            send_reasoning: anthropic_options
                .get("sendReasoning")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        });
        warnings.extend(prompt_plan.warnings.clone());
        betas.extend(prompt_plan.betas.iter().cloned());

        let mut temperature = options.temperature;
        let mut top_p = options.top_p;
        let mut top_k = options.top_k;
        if let Some(value) = temperature {
            if value > 1.0 {
                temperature = Some(1.0);
                warnings.push(Warning::Unsupported {
                    feature: "temperature".to_string(),
                    details: Some(format!(
                        "{value} exceeds anthropic maximum of 1.0. clamped to 1.0"
                    )),
                });
            } else if value < 0.0 {
                temperature = Some(0.0);
                warnings.push(Warning::Unsupported {
                    feature: "temperature".to_string(),
                    details: Some(format!(
                        "{value} is below anthropic minimum of 0. clamped to 0"
                    )),
                });
            }
        }

        if capabilities.rejects_sampling_parameters {
            if temperature.take().is_some() {
                warnings.push(unsupported_sampling_warning("temperature", &self.model_id));
            }
            if top_k.take().is_some() {
                warnings.push(unsupported_sampling_warning("topK", &self.model_id));
            }
            if top_p.take().is_some() {
                warnings.push(unsupported_sampling_warning("topP", &self.model_id));
            }
        }

        let supports_structured_output = self.config.supports_native_structured_output
            && capabilities.supports_structured_output;
        let supports_strict_tools =
            self.config.supports_strict_tools && capabilities.supports_structured_output;

        let structured_output_mode = anthropic_options
            .get("structuredOutputMode")
            .and_then(Value::as_str)
            .unwrap_or("auto");
        let use_structured_output = structured_output_mode == "outputFormat"
            || (structured_output_mode == "auto" && supports_structured_output);

        let json_response_tool =
            json_response_tool(options.response_format.as_ref(), use_structured_output);

        let mut tools = options.tools.clone().unwrap_or_default();
        let mut tool_choice = options.tool_choice.clone();
        if let Some(json_tool) = json_response_tool.clone() {
            tools.push(LanguageModelTool::Function(json_tool));
            tool_choice = Some(LanguageModelToolChoice::Required);
        }

        let mut cache_validator = CacheControlValidator::default();
        let tool_plan = prepare_tools(PrepareToolsOptions {
            tools: &tools,
            tool_choice: tool_choice.as_ref(),
            disable_parallel_tool_use: anthropic_options
                .get("disableParallelToolUse")
                .and_then(Value::as_bool),
            supports_structured_output,
            supports_strict_tools,
            default_eager_input_streaming: stream
                && anthropic_options
                    .get("toolStreaming")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            cache_validator: &mut cache_validator,
        });
        warnings.extend(tool_plan.warnings.clone());
        warnings.extend(cache_validator.warnings());
        betas.extend(tool_plan.betas.iter().cloned());

        let mut max_tokens = options
            .max_output_tokens
            .unwrap_or(capabilities.max_output_tokens);
        let mut body = Map::new();
        body.insert("model".to_string(), json!(self.model_id));
        body.insert("max_tokens".to_string(), json!(max_tokens));
        insert_opt(&mut body, "temperature", temperature.map(Value::from));
        insert_opt(&mut body, "top_p", top_p.map(Value::from));
        insert_opt(&mut body, "top_k", top_k.map(Value::from));
        insert_opt(
            &mut body,
            "stop_sequences",
            options.stop_sequences.clone().map(Value::from),
        );

        let mut thinking = anthropic_options.get("thinking").cloned();
        let mut effort = anthropic_options.get("effort").cloned();
        if effort.is_none() && is_custom_reasoning(options.reasoning.as_ref()) {
            let reasoning = options
                .reasoning
                .as_ref()
                .expect("checked custom reasoning");
            let config =
                resolve_anthropic_reasoning_config(reasoning, &capabilities, &mut warnings);
            if thinking.is_none() {
                thinking = config.thinking;
            }
            if effort.is_none() {
                effort = config.effort;
            }
        }

        let thinking_type = thinking
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut thinking_budget = thinking
            .as_ref()
            .and_then(|value| {
                value
                    .get("budgetTokens")
                    .or_else(|| value.get("budget_tokens"))
            })
            .and_then(Value::as_u64);

        if matches!(thinking_type.as_deref(), Some("enabled") | Some("adaptive")) {
            if thinking_type.as_deref() == Some("enabled") && thinking_budget.is_none() {
                thinking_budget = Some(1024);
                warnings.push(Warning::Compatibility {
                    feature: "extended thinking".to_string(),
                    details: Some(
                        "thinking budget is required when thinking is enabled. using default budget of 1024 tokens."
                            .to_string(),
                    ),
                });
            }
            let mut thinking_body = Map::new();
            thinking_body.insert(
                "type".to_string(),
                Value::String(thinking_type.clone().unwrap_or_default()),
            );
            insert_opt(
                &mut thinking_body,
                "budget_tokens",
                thinking_budget.map(Value::from),
            );
            if let Some(display) = thinking
                .as_ref()
                .and_then(|value| value.get("display"))
                .cloned()
            {
                thinking_body.insert("display".to_string(), display);
            }
            body.insert("thinking".to_string(), Value::Object(thinking_body));

            if body.remove("temperature").is_some() {
                warnings.push(Warning::Unsupported {
                    feature: "temperature".to_string(),
                    details: Some(
                        "temperature is not supported when thinking is enabled".to_string(),
                    ),
                });
            }
            if body.remove("top_k").is_some() {
                warnings.push(Warning::Unsupported {
                    feature: "topK".to_string(),
                    details: Some("topK is not supported when thinking is enabled".to_string()),
                });
            }
            if body.remove("top_p").is_some() {
                warnings.push(Warning::Unsupported {
                    feature: "topP".to_string(),
                    details: Some("topP is not supported when thinking is enabled".to_string()),
                });
            }
            max_tokens = max_tokens.saturating_add(thinking_budget.unwrap_or(0));
            body.insert("max_tokens".to_string(), json!(max_tokens));
        } else if capabilities.is_anthropic_model && top_p.is_some() && temperature.is_some() {
            body.remove("top_p");
            warnings.push(Warning::Unsupported {
                feature: "topP".to_string(),
                details: Some(
                    "topP is not supported when temperature is set. topP is ignored.".to_string(),
                ),
            });
        }

        if capabilities.is_known_model && max_tokens > capabilities.max_output_tokens {
            if options.max_output_tokens.is_some() {
                warnings.push(Warning::Unsupported {
                    feature: "maxOutputTokens".to_string(),
                    details: Some(format!(
                        "{max_tokens} (maxOutputTokens + thinkingBudget) is greater than {} {} max output tokens. The max output tokens have been limited to {}.",
                        self.model_id, capabilities.max_output_tokens, capabilities.max_output_tokens
                    )),
                });
            }
            body.insert(
                "max_tokens".to_string(),
                json!(capabilities.max_output_tokens),
            );
        }

        if effort.is_some()
            || anthropic_options.get("taskBudget").is_some()
            || (use_structured_output
                && matches!(
                    options.response_format,
                    Some(LanguageModelResponseFormat::Json {
                        schema: Some(_),
                        ..
                    })
                ))
        {
            let mut output_config = Map::new();
            if let Some(effort) = effort {
                output_config.insert("effort".to_string(), effort);
            }
            if let Some(task_budget) = anthropic_options.get("taskBudget") {
                output_config.insert("task_budget".to_string(), camel_to_snake_value(task_budget));
                betas.insert("task-budgets-2026-03-13".to_string());
            }
            if use_structured_output
                && let Some(LanguageModelResponseFormat::Json {
                    schema: Some(schema),
                    ..
                }) = &options.response_format
            {
                output_config.insert(
                    "format".to_string(),
                    json!({
                        "type": "json_schema",
                        "schema": sanitize_json_schema(schema),
                    }),
                );
            }
            body.insert("output_config".to_string(), Value::Object(output_config));
        }

        for (input_key, output_key) in [
            ("speed", "speed"),
            ("inferenceGeo", "inference_geo"),
            ("cacheControl", "cache_control"),
        ] {
            if let Some(value) = anthropic_options.get(input_key) {
                body.insert(output_key.to_string(), camel_to_snake_value(value));
            }
        }
        if anthropic_options.get("speed").and_then(Value::as_str) == Some("fast") {
            betas.insert("fast-mode-2026-02-01".to_string());
        }
        if let Some(user_id) = anthropic_options
            .get("metadata")
            .and_then(|value| value.get("userId"))
            .cloned()
        {
            body.insert("metadata".to_string(), json!({ "user_id": user_id }));
        }
        if let Some(servers) = anthropic_options
            .get("mcpServers")
            .and_then(Value::as_array)
            && !servers.is_empty()
        {
            body.insert(
                "mcp_servers".to_string(),
                camel_to_snake_value(&Value::Array(servers.clone())),
            );
            betas.insert("mcp-client-2025-04-04".to_string());
        }
        if let Some(container) = anthropic_options.get("container") {
            let has_skills = container
                .get("skills")
                .and_then(Value::as_array)
                .is_some_and(|skills| !skills.is_empty());
            let container_value = if has_skills {
                // Object format when skills are provided (agent skills feature).
                let skills: Vec<Value> = container
                    .get("skills")
                    .and_then(Value::as_array)
                    .map(|skills| skills.iter().map(serialize_container_skill).collect())
                    .unwrap_or_default();
                json!({
                    "id": container.get("id").cloned().unwrap_or(Value::Null),
                    "skills": skills,
                })
            } else {
                // String format for container ID only (programmatic tool calling).
                container.get("id").cloned().unwrap_or(Value::Null)
            };
            body.insert("container".to_string(), container_value);
            if has_skills {
                betas.insert("code-execution-2025-08-25".to_string());
                betas.insert("skills-2025-10-02".to_string());
                betas.insert("files-api-2025-04-14".to_string());
            }
        }
        if let Some(context_management) = anthropic_options.get("contextManagement") {
            body.insert(
                "context_management".to_string(),
                camel_to_snake_value(context_management),
            );
            betas.insert("context-management-2025-06-27".to_string());
            if context_management.to_string().contains("compact_20260112") {
                betas.insert("compact-2026-01-12".to_string());
            }
        }

        if !prompt_plan.system.is_empty() {
            body.insert("system".to_string(), Value::Array(prompt_plan.system));
        }
        body.insert("messages".to_string(), Value::Array(prompt_plan.messages));
        insert_opt(
            &mut body,
            "tools",
            (!tool_plan.tools.is_empty()).then_some(Value::Array(tool_plan.tools)),
        );
        insert_opt(&mut body, "tool_choice", tool_plan.tool_choice);
        if stream {
            body.insert("stream".to_string(), Value::Bool(true));
        }

        for beta in anthropic_options
            .get("anthropicBeta")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            betas.insert(beta.to_string());
        }

        let mut headers = self.config.headers.clone();
        if let Some(request_headers) = &options.headers {
            headers.extend(request_headers.clone());
        }
        if !betas.is_empty() {
            headers.insert(
                "anthropic-beta".to_string(),
                betas.iter().cloned().collect::<Vec<_>>().join(","),
            );
        }

        AnthropicRequestPlan {
            url: format!("{}/messages", self.config.base_url),
            headers,
            body: Value::Object(body),
            warnings,
            betas,
            uses_json_response_tool: json_response_tool.is_some(),
            provider_options_name: provider_options_name(&self.config.provider),
            used_custom_provider_key: provider_options_name(&self.config.provider) != "anthropic"
                && options.provider_options.as_ref().is_some_and(|options| {
                    options.contains_key(&provider_options_name(&self.config.provider))
                }),
        }
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let plan = self.request_plan(&options, false);
        let request_body = plan.body.clone();
        let mut request = prepare_post_json_to_api_request(
            plan.url.clone(),
            Some(optional_headers(&plan.headers)),
            request_body.clone(),
            &ai_sdk_provider_utils::RuntimeEnvironment::default(),
        );
        if let Some(signal) = options.abort_signal {
            request = request.with_abort_signal(signal);
        }

        let result = (self.config.transport)(request).await;
        match result {
            Ok(response) if response.is_success_status() => {
                let raw_value = response
                    .text_body()
                    .and_then(|body| serde_json::from_str::<Value>(body).ok())
                    .unwrap_or_else(|| json!({}));
                map_anthropic_generate_response(
                    &raw_value,
                    raw_value.clone(),
                    Some(response.headers),
                    request_body,
                    plan,
                )
            }
            Ok(response) => {
                let error = response
                    .text_body()
                    .and_then(parse_anthropic_error)
                    .unwrap_or_else(|| response.status_text.clone());
                error_generate_result(error, request_body, plan.warnings)
            }
            Err(error) => {
                error_generate_result(error.message().to_string(), request_body, plan.warnings)
            }
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let plan = self.request_plan(&options, true);
        let citation_documents = extract_citation_documents(&options.prompt);
        let request_body = plan.body.clone();
        let mut request = prepare_post_json_to_api_request(
            plan.url.clone(),
            Some(optional_headers(&plan.headers)),
            request_body.clone(),
            &ai_sdk_provider_utils::RuntimeEnvironment::default(),
        );
        if let Some(signal) = options.abort_signal {
            request = request.with_abort_signal(signal);
        }

        let result = (self.config.transport)(request).await;
        let stream = match result {
            Ok(response) if response.is_success_status() => {
                let chunks = parse_sse_json_chunks(response.text_body().unwrap_or_default());
                map_anthropic_stream_chunks(
                    &chunks,
                    plan.warnings.clone(),
                    options.include_raw_chunks.unwrap_or(false),
                    plan.uses_json_response_tool,
                    &citation_documents,
                )
            }
            Ok(response) => vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(
                    plan.warnings.clone(),
                )),
                LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
                    "error": response
                        .text_body()
                        .and_then(parse_anthropic_error)
                        .unwrap_or_else(|| response.status_text.clone())
                }))),
            ],
            Err(error) => vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(
                    plan.warnings.clone(),
                )),
                LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
                    "error": error.message()
                }))),
            ],
        };

        LanguageModelStreamResult::new(stream)
            .with_request(LanguageModelRequest::new().with_body(request_body))
            .with_response(LanguageModelStreamResultResponse { headers: None })
    }
}

impl LanguageModel for AnthropicLanguageModel {
    type SupportedUrlsFuture<'a>
        = Ready<LanguageModelSupportedUrls>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
    where
        Self: 'a;

    type Stream = Vec<LanguageModelStreamPart>;

    type StreamFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelStreamResult<Self::Stream>> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(BTreeMap::from([
            ("image/*".to_string(), vec!["^https?://.*$".to_string()]),
            (
                "application/pdf".to_string(),
                vec!["^https?://.*$".to_string()],
            ),
        ]))
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

/// Prepared Anthropic JSON request metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicRequestPlan {
    /// Request URL.
    pub url: String,
    /// Request headers.
    pub headers: Headers,
    /// JSON body sent to Anthropic.
    pub body: JsonValue,
    /// Non-fatal provider warnings.
    pub warnings: Vec<Warning>,
    /// Anthropic beta flags collected from prompt, tools, provider options, and headers.
    pub betas: BTreeSet<String>,
    /// Whether a synthetic JSON response tool was used.
    pub uses_json_response_tool: bool,
    /// Provider options namespace used for metadata mirroring.
    pub provider_options_name: String,
    /// Whether a custom provider options namespace was used.
    pub used_custom_provider_key: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCapabilities {
    /// Maximum output tokens accepted by the model family.
    pub max_output_tokens: u64,
    /// Whether native structured output and strict tools are available.
    pub supports_structured_output: bool,
    /// Whether adaptive thinking effort is available.
    pub supports_adaptive_thinking: bool,
    /// Whether sampling parameters are rejected by the model family.
    pub rejects_sampling_parameters: bool,
    /// Whether the `xhigh` reasoning effort can be sent directly.
    pub supports_xhigh_effort: bool,
    /// Whether the model family is explicitly recognized by the upstream package.
    pub is_known_model: bool,
    /// Whether the model id is recognized or at least shaped like an Anthropic Claude id.
    pub is_anthropic_model: bool,
}

/// Returns upstream Anthropic model capability switches.
pub fn get_model_capabilities(model_id: &str) -> ModelCapabilities {
    let (
        max_output_tokens,
        supports_structured_output,
        supports_adaptive_thinking,
        rejects_sampling_parameters,
        supports_xhigh_effort,
        is_known_model,
    ) = if model_id.contains("claude-opus-4-8") || model_id.contains("claude-opus-4-7") {
        (128000, true, true, true, true, true)
    } else if model_id.contains("claude-sonnet-4-6") || model_id.contains("claude-opus-4-6") {
        (128000, true, true, false, false, true)
    } else if model_id.contains("claude-sonnet-4-5")
        || model_id.contains("claude-opus-4-5")
        || model_id.contains("claude-haiku-4-5")
    {
        (64000, true, false, false, false, true)
    } else if model_id.contains("claude-opus-4-1") {
        (32000, true, false, false, false, true)
    } else if model_id.contains("claude-sonnet-4-") {
        (64000, false, false, false, false, true)
    } else if model_id.contains("claude-opus-4-") {
        (32000, false, false, false, false, true)
    } else if model_id.contains("claude-3-haiku") {
        (4096, false, false, false, false, true)
    } else {
        (4096, false, false, false, false, false)
    };

    ModelCapabilities {
        max_output_tokens,
        supports_structured_output,
        supports_adaptive_thinking,
        rejects_sampling_parameters,
        supports_xhigh_effort,
        is_known_model,
        is_anthropic_model: is_known_model || model_id.starts_with("claude-"),
    }
}

/// Serializes a single agent-skill container entry to the Anthropic API shape,
/// resolving `skillId`/`providerReference` to `skill_id`.
fn serialize_container_skill(skill: &Value) -> Value {
    let skill_type = skill.get("type").and_then(Value::as_str).unwrap_or("");
    let skill_id = if skill_type == "custom" {
        skill
            .get("providerReference")
            .or_else(|| skill.get("provider_reference"))
            .and_then(|reference| reference.get("anthropic"))
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        skill
            .get("skillId")
            .or_else(|| skill.get("skill_id"))
            .cloned()
            .unwrap_or(Value::Null)
    };
    json!({
        "type": skill.get("type").cloned().unwrap_or(Value::Null),
        "skill_id": skill_id,
        "version": skill.get("version").cloned().unwrap_or(Value::Null),
    })
}

fn provider_options_name(provider: &str) -> String {
    provider
        .split_once('.')
        .map_or(provider, |(name, _)| name)
        .to_string()
}

fn unsupported_sampling_warning(feature: &str, model_id: &str) -> Warning {
    Warning::Unsupported {
        feature: feature.to_string(),
        details: Some(format!(
            "{feature} is not supported by {model_id} and will be ignored"
        )),
    }
}

fn is_custom_reasoning(reasoning: Option<&LanguageModelReasoningEffort>) -> bool {
    !matches!(
        reasoning,
        None | Some(LanguageModelReasoningEffort::ProviderDefault)
    )
}

struct AnthropicReasoningConfig {
    thinking: Option<Value>,
    effort: Option<Value>,
}

fn resolve_anthropic_reasoning_config(
    reasoning: &LanguageModelReasoningEffort,
    capabilities: &ModelCapabilities,
    warnings: &mut Vec<Warning>,
) -> AnthropicReasoningConfig {
    if matches!(reasoning, LanguageModelReasoningEffort::None) {
        return AnthropicReasoningConfig {
            thinking: Some(json!({ "type": "disabled" })),
            effort: None,
        };
    }

    let Ok(level) = ReasoningLevel::try_from(reasoning.clone()) else {
        return AnthropicReasoningConfig {
            thinking: None,
            effort: None,
        };
    };

    if capabilities.supports_adaptive_thinking {
        let effort_map = BTreeMap::from([
            (ReasoningLevel::Minimal, "low".to_string()),
            (ReasoningLevel::Low, "low".to_string()),
            (ReasoningLevel::Medium, "medium".to_string()),
            (ReasoningLevel::High, "high".to_string()),
            (
                ReasoningLevel::Xhigh,
                if capabilities.supports_xhigh_effort {
                    "xhigh".to_string()
                } else {
                    "max".to_string()
                },
            ),
        ]);
        let effort = map_reasoning_to_provider_effort(level, &effort_map, warnings);
        return AnthropicReasoningConfig {
            thinking: Some(json!({ "type": "adaptive" })),
            effort: effort.map(Value::String),
        };
    }

    let budget = map_reasoning_to_provider_budget(
        level,
        capabilities.max_output_tokens,
        capabilities.max_output_tokens,
        Some(1024),
        None,
        warnings,
    );
    AnthropicReasoningConfig {
        thinking: budget.map(|budget| json!({ "type": "enabled", "budgetTokens": budget })),
        effort: None,
    }
}

fn merged_anthropic_options(
    provider_options: Option<&ProviderOptions>,
    provider_options_name: &str,
) -> JsonObject {
    let mut merged = JsonObject::new();
    if let Some(options) = provider_options {
        if let Some(canonical) = options.get("anthropic") {
            merged.extend(canonical.clone());
        }
        if provider_options_name != "anthropic"
            && let Some(custom) = options.get(provider_options_name)
        {
            merged.extend(custom.clone());
        }
    }
    merged
}

fn json_response_tool(
    response_format: Option<&LanguageModelResponseFormat>,
    use_structured_output: bool,
) -> Option<LanguageModelFunctionTool> {
    match response_format {
        Some(LanguageModelResponseFormat::Json {
            schema: Some(schema),
            ..
        }) if !use_structured_output => Some(
            LanguageModelFunctionTool::new("json", schema.clone())
                .with_description("Respond with a JSON object."),
        ),
        _ => None,
    }
}

/// Options for converting standardized model messages to Anthropic prompt blocks.
#[derive(Clone, Copy)]
pub struct AnthropicPromptConversionOptions<'a> {
    /// Standardized provider prompt.
    pub prompt: &'a [LanguageModelMessage],
    /// Whether assistant reasoning parts should be sent back to Anthropic.
    pub send_reasoning: bool,
}

/// Anthropic prompt conversion output.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicPromptPlan {
    /// Anthropic system content blocks.
    pub system: Vec<Value>,
    /// Anthropic user/assistant messages.
    pub messages: Vec<Value>,
    /// Beta flags required by prompt parts.
    pub betas: BTreeSet<String>,
    /// Non-fatal prompt conversion warnings.
    pub warnings: Vec<Warning>,
}

/// Converts provider-v4 messages to Anthropic Messages API prompt shape.
pub fn convert_to_anthropic_prompt(
    options: AnthropicPromptConversionOptions<'_>,
) -> AnthropicPromptPlan {
    let mut plan = AnthropicPromptPlan::default();
    let mut blocks = Vec::<PromptBlock>::new();

    for message in options.prompt {
        let block_type = match message {
            LanguageModelMessage::System(_) => PromptBlockType::System,
            LanguageModelMessage::Assistant(_) => PromptBlockType::Assistant,
            LanguageModelMessage::User(_) | LanguageModelMessage::Tool(_) => PromptBlockType::User,
        };
        if blocks
            .last()
            .is_none_or(|block| block.block_type != block_type)
        {
            blocks.push(PromptBlock {
                block_type,
                messages: Vec::new(),
            });
        }
        blocks
            .last_mut()
            .expect("block just inserted")
            .messages
            .push(message);
    }

    for (block_index, block) in blocks.iter().enumerate() {
        match block.block_type {
            PromptBlockType::System => {
                let mut content = Vec::new();
                for message in &block.messages {
                    if let LanguageModelMessage::System(system) = message {
                        let mut block = Map::new();
                        block.insert("type".to_string(), json!("text"));
                        block.insert("text".to_string(), json!(system.content));
                        if let Some(cache_control) = get_cache_control(&system.provider_options) {
                            block.insert("cache_control".to_string(), cache_control);
                        }
                        content.push(Value::Object(block));
                    }
                }
                if plan.system.is_empty() {
                    // First system block becomes the top-level `system` field.
                    plan.system = content;
                } else {
                    // A mid-conversation system block is emitted inline as a message
                    // and enables the mid-conversation system beta.
                    plan.messages
                        .push(json!({ "role": "system", "content": content }));
                    plan.betas
                        .insert("mid-conversation-system-2026-04-07".to_string());
                }
            }
            PromptBlockType::User => {
                let mut content = Vec::new();
                for message in &block.messages {
                    match message {
                        LanguageModelMessage::User(user) => {
                            for part in &user.content {
                                match part {
                                    LanguageModelUserContentPart::Text(text) => {
                                        let mut value = json!({
                                            "type": "text",
                                            "text": text.text,
                                        });
                                        attach_cache_control(&mut value, &text.provider_options);
                                        content.push(value);
                                    }
                                    LanguageModelUserContentPart::File(file) => {
                                        content.push(convert_file_part(
                                            file,
                                            &mut plan.betas,
                                            &mut plan.warnings,
                                        ));
                                    }
                                }
                            }
                        }
                        LanguageModelMessage::Tool(tool) => {
                            for part in &tool.content {
                                if let LanguageModelToolContentPart::ToolResult(result) = part {
                                    content.push(convert_tool_result_part(result));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                plan.messages
                    .push(json!({ "role": "user", "content": content }));
            }
            PromptBlockType::Assistant => {
                let mut content = Vec::new();
                let is_last_block = block_index == blocks.len() - 1;
                for (message_index, message) in block.messages.iter().enumerate() {
                    if let LanguageModelMessage::Assistant(assistant) = message {
                        let is_last_message = message_index == block.messages.len() - 1;
                        for (part_index, part) in assistant.content.iter().enumerate() {
                            let is_last_part = part_index == assistant.content.len() - 1;
                            match part {
                                LanguageModelAssistantContentPart::Text(text) => {
                                    let text_value =
                                        if is_last_block && is_last_message && is_last_part {
                                            text.text.trim().to_string()
                                        } else {
                                            text.text.clone()
                                        };
                                    content.push(json!({ "type": "text", "text": text_value }));
                                }
                                LanguageModelAssistantContentPart::Reasoning(reasoning) => {
                                    if options.send_reasoning {
                                        if let Some(anthropic) = provider_options_object(
                                            &reasoning.provider_options,
                                            "anthropic",
                                        ) && let Some(signature) = anthropic.get("signature")
                                        {
                                            content.push(json!({
                                                "type": "thinking",
                                                "thinking": reasoning.text,
                                                "signature": signature,
                                            }));
                                        } else if let Some(anthropic) = provider_options_object(
                                            &reasoning.provider_options,
                                            "anthropic",
                                        ) && let Some(data) =
                                            anthropic.get("redactedData")
                                        {
                                            content.push(json!({
                                                "type": "redacted_thinking",
                                                "data": data,
                                            }));
                                        } else {
                                            plan.warnings.push(Warning::Other {
                                                message: "unsupported reasoning metadata"
                                                    .to_string(),
                                            });
                                        }
                                    } else {
                                        plan.warnings.push(Warning::Other {
                                            message: "sending reasoning content is disabled for this model".to_string(),
                                        });
                                    }
                                }
                                LanguageModelAssistantContentPart::ToolCall(tool_call) => {
                                    content.push(convert_tool_call_part(tool_call));
                                }
                                LanguageModelAssistantContentPart::ToolResult(result) => {
                                    content.push(convert_tool_result_part(result));
                                }
                                LanguageModelAssistantContentPart::File(file) => {
                                    content.push(convert_file_part(
                                        file,
                                        &mut plan.betas,
                                        &mut plan.warnings,
                                    ));
                                }
                                LanguageModelAssistantContentPart::Custom(custom) => {
                                    content.push(json!({
                                        "type": "custom",
                                        "kind": custom.kind,
                                    }));
                                }
                                LanguageModelAssistantContentPart::ReasoningFile(file) => {
                                    content.push(json!({
                                        "type": "reasoning_file",
                                        "media_type": file.media_type,
                                    }));
                                }
                                LanguageModelAssistantContentPart::ToolApprovalRequest(request) => {
                                    content.push(json!({
                                        "type": "tool_approval_request",
                                        "approval_id": request.approval_id,
                                        "tool_use_id": request.tool_call_id,
                                    }));
                                }
                            }
                        }
                    }
                }
                plan.messages
                    .push(json!({ "role": "assistant", "content": content }));
            }
        }
    }

    plan
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptBlockType {
    System,
    User,
    Assistant,
}

struct PromptBlock<'a> {
    block_type: PromptBlockType,
    messages: Vec<&'a LanguageModelMessage>,
}

fn convert_file_part(
    file: &LanguageModelFilePart,
    betas: &mut BTreeSet<String>,
    warnings: &mut Vec<Warning>,
) -> Value {
    match &file.data {
        FileData::Reference { reference } => {
            betas.insert("files-api-2025-04-14".to_string());
            let file_id = reference
                .as_map()
                .get("anthropic")
                .cloned()
                .unwrap_or_default();
            let container_upload = provider_options_object(&file.provider_options, "anthropic")
                .and_then(|options| {
                    options
                        .get("containerUpload")
                        .or_else(|| options.get("container_upload"))
                })
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if container_upload {
                json!({ "type": "container_upload", "file_id": file_id })
            } else if get_top_level_media_type(&file.media_type) == "image" {
                json!({ "type": "image", "source": { "type": "file", "file_id": file_id } })
            } else {
                json!({ "type": "document", "source": { "type": "file", "file_id": file_id } })
            }
        }
        FileData::Url { url } => {
            if get_top_level_media_type(&file.media_type) == "image" {
                json!({ "type": "image", "source": { "type": "url", "url": url.to_string() } })
            } else {
                if file.media_type == "application/pdf" {
                    betas.insert("pdfs-2024-09-25".to_string());
                }
                json!({
                    "type": "document",
                    "source": { "type": "url", "url": url.to_string() },
                    "title": file.filename,
                })
            }
        }
        FileData::Text { text } => json!({
            "type": "document",
            "source": {
                "type": "text",
                "media_type": "text/plain",
                "data": text,
            },
            "title": file.filename,
        }),
        FileData::Data { data } => {
            let top_level = get_top_level_media_type(&file.media_type);
            if top_level == "image" {
                json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": resolve_full_media_type_or_original(file, warnings),
                        "data": convert_to_base64(data),
                    }
                })
            } else if file.media_type == "application/pdf"
                || resolve_full_media_type_or_original(file, warnings) == "application/pdf"
            {
                betas.insert("pdfs-2024-09-25".to_string());
                json!({
                    "type": "document",
                    "source": {
                        "type": "base64",
                        "media_type": "application/pdf",
                        "data": convert_to_base64(data),
                    },
                    "title": file.filename,
                })
            } else if file.media_type == "text/plain" {
                let text = match data {
                    FileDataContent::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                    FileDataContent::Base64(base64) => convert_base64_to_bytes(base64)
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                        .unwrap_or_default(),
                };
                json!({
                    "type": "document",
                    "source": { "type": "text", "media_type": "text/plain", "data": text },
                    "title": file.filename,
                })
            } else {
                warnings.push(Warning::Other {
                    message: format!("unsupported file media type: {}", file.media_type),
                });
                json!({ "type": "text", "text": "" })
            }
        }
    }
}

fn resolve_full_media_type_or_original(
    file: &LanguageModelFilePart,
    warnings: &mut Vec<Warning>,
) -> String {
    ai_sdk_provider_utils::resolve_full_media_type(file).unwrap_or_else(|error| {
        warnings.push(Warning::Other {
            message: error.to_string(),
        });
        file.media_type.clone()
    })
}

fn convert_tool_call_part(part: &LanguageModelToolCallPart) -> Value {
    if part.provider_executed == Some(true) {
        let provider_tool_name = provider_tool_name(&part.tool_name);
        json!({
            "type": "server_tool_use",
            "id": part.tool_call_id,
            "name": provider_tool_name,
            "input": part.input,
        })
    } else {
        json!({
            "type": "tool_use",
            "id": part.tool_call_id,
            "name": part.tool_name,
            "input": part.input,
        })
    }
}

/// Extracts the `errorCode` from a provider tool error result value, handling
/// both stringified-JSON and plain-object forms (mirrors `extractErrorValue`).
fn extract_error_code(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => serde_json::from_str::<Value>(text).ok().and_then(|parsed| {
            parsed
                .get("errorCode")
                .and_then(Value::as_str)
                .map(str::to_string)
        }),
        Value::Object(_) => value
            .get("errorCode")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn convert_tool_result_part(part: &LanguageModelToolResultPart) -> Value {
    // Provider-executed web_search error results round-trip to the API error shape.
    if provider_tool_name(&part.tool_name) == "web_search"
        && let LanguageModelToolResultOutput::ErrorJson { value, .. } = &part.output
    {
        let error_code = extract_error_code(value).unwrap_or_else(|| "unavailable".to_string());
        return json!({
            "type": "web_search_tool_result",
            "tool_use_id": part.tool_call_id,
            "content": {
                "type": "web_search_tool_result_error",
                "error_code": error_code,
            },
        });
    }

    let (content, is_error) = match &part.output {
        LanguageModelToolResultOutput::Text { value, .. } => (Value::String(value.clone()), None),
        LanguageModelToolResultOutput::Json { value, .. } => (value.clone(), None),
        LanguageModelToolResultOutput::ExecutionDenied { reason, .. } => (
            Value::String(
                reason
                    .clone()
                    .unwrap_or_else(|| "Tool call execution denied.".to_string()),
            ),
            None,
        ),
        LanguageModelToolResultOutput::ErrorText { value, .. } => {
            (Value::String(value.clone()), Some(true))
        }
        LanguageModelToolResultOutput::ErrorJson { value, .. } => (value.clone(), Some(true)),
        LanguageModelToolResultOutput::Content { value } => (
            Value::Array(
                value
                    .iter()
                    .filter_map(|part| match part {
                        ai_sdk_provider::LanguageModelToolResultContentPart::Text(text) => {
                            Some(json!({ "type": "text", "text": text.text }))
                        }
                        ai_sdk_provider::LanguageModelToolResultContentPart::File(file) => {
                            Some(json!({ "type": "document", "media_type": file.media_type }))
                        }
                        ai_sdk_provider::LanguageModelToolResultContentPart::Custom(_) => None,
                    })
                    .collect(),
            ),
            None,
        ),
    };

    let mut value = json!({
        "type": "tool_result",
        "tool_use_id": part.tool_call_id,
        "content": content,
    });
    if let Some(is_error) = is_error {
        value["is_error"] = Value::Bool(is_error);
    }
    value
}

fn provider_tool_name(tool_name: &str) -> &str {
    match tool_name {
        "code_execution" => "code_execution",
        "web_fetch" => "web_fetch",
        "web_search" => "web_search",
        "tool_search_tool_regex" => "tool_search_tool_regex",
        "tool_search_tool_bm25" => "tool_search_tool_bm25",
        "advisor" => "advisor",
        other => other,
    }
}

/// Anthropic cache-control validator.
#[derive(Clone, Debug, Default)]
pub struct CacheControlValidator {
    breakpoints: usize,
    warnings: Vec<Warning>,
}

impl CacheControlValidator {
    /// Reads Anthropic cache control from provider options.
    pub fn get_cache_control(
        &mut self,
        provider_options: &Option<ProviderOptions>,
        context_type: &str,
        can_cache: bool,
    ) -> Option<Value> {
        let cache_control = get_cache_control(provider_options)?;
        if !can_cache {
            self.warnings.push(Warning::Other {
                message: format!("cache_control is not supported on {context_type}"),
            });
            return None;
        }
        self.breakpoints += 1;
        if self.breakpoints > 4 {
            self.warnings.push(Warning::Other {
                message: "Anthropic supports at most 4 cache_control breakpoints.".to_string(),
            });
            return None;
        }
        Some(cache_control)
    }

    /// Returns accumulated warnings.
    pub fn warnings(self) -> Vec<Warning> {
        self.warnings
    }
}

fn get_cache_control(provider_options: &Option<ProviderOptions>) -> Option<Value> {
    provider_options_object(provider_options, "anthropic")
        .and_then(|options| {
            options
                .get("cacheControl")
                .or_else(|| options.get("cache_control"))
        })
        .cloned()
}

fn attach_cache_control(value: &mut Value, provider_options: &Option<ProviderOptions>) {
    if let Some(cache_control) = get_cache_control(provider_options) {
        value["cache_control"] = cache_control;
    }
}

fn provider_options_object<'a>(
    provider_options: &'a Option<ProviderOptions>,
    provider: &str,
) -> Option<&'a JsonObject> {
    provider_options
        .as_ref()
        .and_then(|options| options.get(provider))
}

/// Options for preparing Anthropic tools.
pub struct PrepareToolsOptions<'a> {
    /// Standardized tools.
    pub tools: &'a [LanguageModelTool],
    /// Tool choice.
    pub tool_choice: Option<&'a LanguageModelToolChoice>,
    /// Whether parallel tool use should be disabled.
    pub disable_parallel_tool_use: Option<bool>,
    /// Whether structured-output beta should be sent for function tools.
    pub supports_structured_output: bool,
    /// Whether strict tool definitions are supported.
    pub supports_strict_tools: bool,
    /// Default eager input streaming value for function tools.
    pub default_eager_input_streaming: bool,
    /// Shared cache control validator.
    pub cache_validator: &'a mut CacheControlValidator,
}

/// Prepared Anthropic tools.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedTools {
    /// Anthropic tool definitions.
    pub tools: Vec<Value>,
    /// Anthropic tool choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    /// Non-fatal tool warnings.
    pub warnings: Vec<Warning>,
    /// Anthropic beta flags required by the tools.
    pub betas: BTreeSet<String>,
}

/// Converts provider-v4 tools to Anthropic tool definitions.
pub fn prepare_tools(options: PrepareToolsOptions<'_>) -> PreparedTools {
    let mut prepared = PreparedTools::default();
    if options.tools.is_empty() {
        return prepared;
    }

    for tool in options.tools {
        match tool {
            LanguageModelTool::Function(function) => {
                let cache_control = options.cache_validator.get_cache_control(
                    &function.provider_options,
                    "tool definition",
                    true,
                );
                let anthropic_options =
                    provider_options_object(&function.provider_options, "anthropic");
                let mut tool_value = Map::new();
                tool_value.insert("name".to_string(), Value::String(function.name.clone()));
                if let Some(description) = &function.description {
                    tool_value.insert(
                        "description".to_string(),
                        Value::String(description.clone()),
                    );
                }
                tool_value.insert(
                    "input_schema".to_string(),
                    Value::Object(function.input_schema.clone()),
                );
                if let Some(cache_control) = cache_control {
                    tool_value.insert("cache_control".to_string(), cache_control);
                }
                let eager_input_streaming = anthropic_options
                    .and_then(|options| options.get("eagerInputStreaming"))
                    .and_then(Value::as_bool)
                    .unwrap_or(options.default_eager_input_streaming);
                if eager_input_streaming {
                    tool_value.insert("eager_input_streaming".to_string(), Value::Bool(true));
                }
                if let Some(strict) = function.strict {
                    if options.supports_strict_tools {
                        tool_value.insert("strict".to_string(), Value::Bool(strict));
                    } else {
                        prepared.warnings.push(Warning::Unsupported {
                            feature: "strict".to_string(),
                            details: Some(format!(
                                "Tool '{}' has strict: {strict}, but strict mode is not supported by this provider. The strict property will be ignored.",
                                function.name
                            )),
                        });
                    }
                }
                if let Some(input_examples) = &function.input_examples {
                    tool_value.insert(
                        "input_examples".to_string(),
                        Value::Array(
                            input_examples
                                .iter()
                                .map(|example| Value::Object(example.input.clone()))
                                .collect(),
                        ),
                    );
                    prepared
                        .betas
                        .insert("advanced-tool-use-2025-11-20".to_string());
                }
                for (input_key, output_key) in [
                    ("deferLoading", "defer_loading"),
                    ("allowedCallers", "allowed_callers"),
                ] {
                    if let Some(value) =
                        anthropic_options.and_then(|options| options.get(input_key))
                    {
                        tool_value.insert(output_key.to_string(), camel_to_snake_value(value));
                        prepared
                            .betas
                            .insert("advanced-tool-use-2025-11-20".to_string());
                    }
                }
                if options.supports_structured_output {
                    prepared
                        .betas
                        .insert("structured-outputs-2025-11-13".to_string());
                }
                prepared.tools.push(Value::Object(tool_value));
            }
            LanguageModelTool::Provider(provider) => {
                if let Some((tool_value, beta)) = anthropic_provider_tool(provider) {
                    if let Some(beta) = beta {
                        prepared.betas.insert(beta.to_string());
                    }
                    prepared.tools.push(tool_value);
                } else {
                    prepared.warnings.push(Warning::Unsupported {
                        feature: format!("provider-defined tool {}", provider.id),
                        details: None,
                    });
                }
            }
        }
    }

    prepared.tool_choice = match options.tool_choice {
        None => options
            .disable_parallel_tool_use
            .map(|disable| json!({ "type": "auto", "disable_parallel_tool_use": disable })),
        Some(LanguageModelToolChoice::Auto) => Some(json!({
            "type": "auto",
            "disable_parallel_tool_use": options.disable_parallel_tool_use,
        })),
        Some(LanguageModelToolChoice::Required) => Some(json!({
            "type": "any",
            "disable_parallel_tool_use": options.disable_parallel_tool_use,
        })),
        Some(LanguageModelToolChoice::None) => {
            prepared.tools.clear();
            None
        }
        Some(LanguageModelToolChoice::Tool { tool_name }) => Some(json!({
            "type": "tool",
            "name": tool_name,
            "disable_parallel_tool_use": options.disable_parallel_tool_use,
        })),
    };

    prepared
}

fn anthropic_provider_tool(
    tool: &LanguageModelProviderTool,
) -> Option<(Value, Option<&'static str>)> {
    let args = &tool.args;
    let value = match tool.id.as_str() {
        "anthropic.code_execution_20250522" => (
            json!({ "type": "code_execution_20250522", "name": "code_execution" }),
            Some("code-execution-2025-05-22"),
        ),
        "anthropic.code_execution_20250825" => (
            json!({ "type": "code_execution_20250825", "name": "code_execution" }),
            Some("code-execution-2025-08-25"),
        ),
        "anthropic.code_execution_20260120" => (
            json!({ "type": "code_execution_20260120", "name": "code_execution" }),
            None,
        ),
        "anthropic.computer_20241022" => (
            json!({
                "type": "computer_20241022",
                "name": "computer",
                "display_width_px": args.get("displayWidthPx"),
                "display_height_px": args.get("displayHeightPx"),
                "display_number": args.get("displayNumber"),
            }),
            Some("computer-use-2024-10-22"),
        ),
        "anthropic.computer_20250124" => (
            json!({
                "type": "computer_20250124",
                "name": "computer",
                "display_width_px": args.get("displayWidthPx"),
                "display_height_px": args.get("displayHeightPx"),
                "display_number": args.get("displayNumber"),
            }),
            Some("computer-use-2025-01-24"),
        ),
        "anthropic.computer_20251124" => (
            json!({
                "type": "computer_20251124",
                "name": "computer",
                "display_width_px": args.get("displayWidthPx"),
                "display_height_px": args.get("displayHeightPx"),
                "display_number": args.get("displayNumber"),
                "enable_zoom": args.get("enableZoom"),
            }),
            Some("computer-use-2025-11-24"),
        ),
        "anthropic.text_editor_20241022" => (
            json!({ "type": "text_editor_20241022", "name": "str_replace_editor" }),
            Some("computer-use-2024-10-22"),
        ),
        "anthropic.text_editor_20250124" => (
            json!({ "type": "text_editor_20250124", "name": "str_replace_editor" }),
            Some("computer-use-2025-01-24"),
        ),
        "anthropic.text_editor_20250429" => (
            json!({ "type": "text_editor_20250429", "name": "str_replace_based_edit_tool" }),
            Some("computer-use-2025-01-24"),
        ),
        "anthropic.text_editor_20250728" => (
            json!({
                "type": "text_editor_20250728",
                "name": "str_replace_based_edit_tool",
                "max_characters": args.get("maxCharacters"),
            }),
            None,
        ),
        "anthropic.bash_20241022" => (
            json!({ "type": "bash_20241022", "name": "bash" }),
            Some("computer-use-2024-10-22"),
        ),
        "anthropic.bash_20250124" => (
            json!({ "type": "bash_20250124", "name": "bash" }),
            Some("computer-use-2025-01-24"),
        ),
        "anthropic.memory_20250818" => (
            json!({ "type": "memory_20250818", "name": "memory" }),
            Some("context-management-2025-06-27"),
        ),
        "anthropic.web_fetch_20250910" => (
            json!({
                "type": "web_fetch_20250910",
                "name": "web_fetch",
                "max_uses": args.get("maxUses"),
                "allowed_domains": args.get("allowedDomains"),
                "blocked_domains": args.get("blockedDomains"),
                "citations": args.get("citations"),
                "max_content_tokens": args.get("maxContentTokens"),
            }),
            Some("web-fetch-2025-09-10"),
        ),
        "anthropic.web_fetch_20260209" => (
            json!({
                "type": "web_fetch_20260209",
                "name": "web_fetch",
                "max_uses": args.get("maxUses"),
                "allowed_domains": args.get("allowedDomains"),
                "blocked_domains": args.get("blockedDomains"),
                "citations": args.get("citations"),
                "max_content_tokens": args.get("maxContentTokens"),
            }),
            Some("code-execution-web-tools-2026-02-09"),
        ),
        "anthropic.web_search_20250305" => (
            json!({
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": args.get("maxUses"),
                "allowed_domains": args.get("allowedDomains"),
                "blocked_domains": args.get("blockedDomains"),
                "user_location": args.get("userLocation"),
            }),
            None,
        ),
        "anthropic.web_search_20260209" => (
            json!({
                "type": "web_search_20260209",
                "name": "web_search",
                "max_uses": args.get("maxUses"),
                "allowed_domains": args.get("allowedDomains"),
                "blocked_domains": args.get("blockedDomains"),
                "user_location": args.get("userLocation"),
            }),
            Some("code-execution-web-tools-2026-02-09"),
        ),
        "anthropic.tool_search_regex_20251119" => (
            json!({ "type": "tool_search_tool_regex_20251119", "name": "tool_search_tool_regex" }),
            None,
        ),
        "anthropic.tool_search_bm25_20251119" => (
            json!({ "type": "tool_search_tool_bm25_20251119", "name": "tool_search_tool_bm25" }),
            None,
        ),
        "anthropic.advisor_20260301" => (
            json!({
                "type": "advisor_20260301",
                "name": "advisor",
                "model": args.get("model"),
                "max_uses": args.get("maxUses"),
                "caching": args.get("caching"),
            }),
            Some("advisor-tool-2026-03-01"),
        ),
        _ => return None,
    };
    Some(value)
}

/// Factory namespace for Anthropic provider-defined tools.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnthropicTools;

impl AnthropicTools {
    /// Creates a provider-defined tool with raw Anthropic args.
    pub fn provider_tool(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
        args: JsonObject,
    ) -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(id, name, args))
    }

    /// Computer-use 2025-01-24 tool.
    pub fn computer_20250124(
        &self,
        display_width_px: u64,
        display_height_px: u64,
        display_number: u64,
    ) -> LanguageModelTool {
        self.provider_tool(
            "anthropic.computer_20250124",
            "computer",
            object(json!({
                "displayWidthPx": display_width_px,
                "displayHeightPx": display_height_px,
                "displayNumber": display_number,
            })),
        )
    }

    /// Bash 2025-01-24 tool.
    pub fn bash_20250124(&self) -> LanguageModelTool {
        self.provider_tool("anthropic.bash_20250124", "bash", JsonObject::new())
    }

    /// Bash 2024-10-22 tool.
    pub fn bash_20241022(&self) -> LanguageModelTool {
        self.provider_tool("anthropic.bash_20241022", "bash", JsonObject::new())
    }

    /// Code execution 2025-08-25 tool.
    pub fn code_execution_20250825(&self) -> LanguageModelTool {
        self.provider_tool(
            "anthropic.code_execution_20250825",
            "code_execution",
            JsonObject::new(),
        )
    }
}

/// Converts Anthropic usage to provider-v4 usage.
pub fn convert_anthropic_usage(
    usage: &JsonObject,
    raw_usage: Option<JsonObject>,
) -> LanguageModelUsage {
    let cache_creation = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let mut input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut output_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    if let Some(iterations) = usage.get("iterations").and_then(Value::as_array)
        && !iterations.is_empty()
    {
        let executor_iterations = iterations
            .iter()
            .filter(|iteration| {
                matches!(
                    iteration.get("type").and_then(Value::as_str),
                    Some("compaction" | "message")
                )
            })
            .collect::<Vec<_>>();
        if !executor_iterations.is_empty() {
            input_tokens = executor_iterations
                .iter()
                .filter_map(|iteration| iteration.get("input_tokens").and_then(Value::as_u64))
                .sum();
            output_tokens = executor_iterations
                .iter()
                .filter_map(|iteration| iteration.get("output_tokens").and_then(Value::as_u64))
                .sum();
        }
    }

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: Some(input_tokens + cache_creation + cache_read),
            no_cache: Some(input_tokens),
            cache_read: Some(cache_read),
            cache_write: Some(cache_creation),
        },
        output_tokens: OutputTokenUsage {
            total: Some(output_tokens),
            text: None,
            reasoning: None,
        },
        raw: Some(raw_usage.unwrap_or_else(|| usage.clone())),
    }
}

/// Maps Anthropic stop reasons to provider-v4 finish reasons.
pub fn map_anthropic_stop_reason(
    finish_reason: Option<&str>,
    is_json_response_from_tool: bool,
) -> FinishReason {
    match finish_reason {
        Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
        Some("max_tokens") => FinishReason::Length,
        Some("tool_use") if is_json_response_from_tool => FinishReason::Stop,
        Some("tool_use") => FinishReason::ToolCalls,
        Some("pause_turn") | Some("refusal") => FinishReason::Other,
        _ => FinishReason::Other,
    }
}

/// Parses an Anthropic API error body and returns its message.
pub fn parse_anthropic_error(body: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(body).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("error") {
        return None;
    }
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Sanitizes JSON Schema for Anthropic native structured output.
pub fn sanitize_json_schema(schema: &JsonSchema) -> JsonSchema {
    sanitize_schema(&Value::Object(schema.clone()))
        .as_object()
        .cloned()
        .unwrap_or_default()
}

fn sanitize_definition(value: &Value) -> Value {
    if value.is_boolean() || !value.is_object() {
        return value.clone();
    }
    sanitize_schema(value)
}

fn sanitize_schema(value: &Value) -> Value {
    let Some(schema) = value.as_object() else {
        return value.clone();
    };
    if let Some(reference) = schema.get("$ref") {
        return json!({ "$ref": reference });
    }

    let mut result = Map::new();
    for key in [
        "$schema",
        "$id",
        "title",
        "description",
        "default",
        "const",
        "enum",
        "type",
    ] {
        if let Some(value) = schema.get(key) {
            result.insert(key.to_string(), value.clone());
        }
    }
    if let Some(any_of) = schema.get("anyOf").and_then(Value::as_array) {
        result.insert(
            "anyOf".to_string(),
            Value::Array(any_of.iter().map(sanitize_definition).collect()),
        );
    } else if let Some(one_of) = schema.get("oneOf").and_then(Value::as_array) {
        result.insert(
            "anyOf".to_string(),
            Value::Array(one_of.iter().map(sanitize_definition).collect()),
        );
    }
    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
        result.insert(
            "allOf".to_string(),
            Value::Array(all_of.iter().map(sanitize_definition).collect()),
        );
    }
    for defs_key in ["definitions", "$defs"] {
        if let Some(defs) = schema.get(defs_key).and_then(Value::as_object) {
            result.insert(
                defs_key.to_string(),
                Value::Object(
                    defs.iter()
                        .map(|(name, definition)| (name.clone(), sanitize_definition(definition)))
                        .collect(),
                ),
            );
        }
    }
    if schema.get("type").and_then(Value::as_str) == Some("object")
        || schema.get("properties").is_some()
    {
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            result.insert(
                "properties".to_string(),
                Value::Object(
                    properties
                        .iter()
                        .map(|(name, definition)| (name.clone(), sanitize_definition(definition)))
                        .collect(),
                ),
            );
        }
        result.insert("additionalProperties".to_string(), Value::Bool(false));
        if let Some(required) = schema.get("required") {
            result.insert("required".to_string(), required.clone());
        }
    }
    if let Some(items) = schema.get("items") {
        result.insert(
            "items".to_string(),
            if let Some(items) = items.as_array() {
                Value::Array(items.iter().map(sanitize_definition).collect())
            } else {
                sanitize_definition(items)
            },
        );
    }
    if let Some(format) = schema.get("format").and_then(Value::as_str)
        && SUPPORTED_STRING_FORMATS.contains(&format)
    {
        result.insert("format".to_string(), Value::String(format.to_string()));
    }
    if let Some(description) = constraint_description(schema) {
        let new_description = result
            .get("description")
            .and_then(Value::as_str)
            .map(|existing| format!("{existing}\n{description}"))
            .unwrap_or(description);
        result.insert("description".to_string(), Value::String(new_description));
    }
    Value::Object(result)
}

const SUPPORTED_STRING_FORMATS: &[&str] = &[
    "date-time",
    "time",
    "date",
    "duration",
    "email",
    "hostname",
    "uri",
    "ipv4",
    "ipv6",
    "uuid",
];

fn constraint_description(schema: &Map<String, Value>) -> Option<String> {
    let mut descriptions = Vec::new();
    for key in [
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "multipleOf",
        "minLength",
        "maxLength",
        "pattern",
        "minItems",
        "maxItems",
        "uniqueItems",
        "minProperties",
        "maxProperties",
        "not",
    ] {
        let Some(value) = schema.get(key) else {
            continue;
        };
        if value.is_null() || value == &Value::Bool(false) {
            continue;
        }
        descriptions.push(format!(
            "{}: {}",
            format_constraint_name(key),
            format_constraint_value(value)
        ));
    }
    if let Some(format) = schema.get("format").and_then(Value::as_str)
        && !SUPPORTED_STRING_FORMATS.contains(&format)
    {
        descriptions.push(format!("format: {format}"));
    }
    (!descriptions.is_empty()).then(|| format!("{}.", descriptions.join("; ")))
}

fn format_constraint_name(key: &str) -> String {
    let mut output = String::new();
    for ch in key.chars() {
        if ch.is_ascii_uppercase() {
            output.push(' ');
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
}

fn format_constraint_value(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

/// Anthropic files interface.
#[derive(Clone)]
pub struct AnthropicFiles {
    provider: String,
    base_url: String,
    headers: Headers,
    transport: AnthropicTransport,
}

impl AnthropicFiles {
    /// Builds the upload-file request without performing HTTP.
    pub fn upload_file_request(&self, options: &FilesUploadFileCallOptions) -> ProviderApiRequest {
        let mut form_data = FormData::new();
        let bytes = upload_file_bytes(&options.data).unwrap_or_default();
        form_data.append("file", FormDataValue::bytes(bytes));
        let mut headers = self.headers.clone();
        headers.insert(
            "anthropic-beta".to_string(),
            "files-api-2025-04-14".to_string(),
        );
        prepare_post_form_data_to_api_request(
            format!("{}/files", self.base_url),
            Some(optional_headers(&headers)),
            form_data,
            &ai_sdk_provider_utils::RuntimeEnvironment::default(),
        )
    }
}

impl Files for AnthropicFiles {
    type UploadFileFuture<'a>
        = Pin<Box<dyn Future<Output = FilesUploadFileResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.provider
    }

    fn upload_file(&self, options: FilesUploadFileCallOptions) -> Self::UploadFileFuture<'_> {
        Box::pin(async move {
            let request = self.upload_file_request(&options);
            let result = (self.transport)(request).await;
            match result {
                Ok(response) if response.is_success_status() => {
                    let value = response
                        .text_body()
                        .and_then(|body| serde_json::from_str::<Value>(body).ok())
                        .unwrap_or_else(|| json!({}));
                    map_upload_file_result(&value, &options)
                }
                _ => FilesUploadFileResult::new(provider_reference("anthropic", "")),
            }
        })
    }
}

fn upload_file_bytes(data: &FilesUploadFileData) -> Result<Vec<u8>, String> {
    match data {
        FilesUploadFileData::Data { data } => match data {
            FileDataContent::Bytes(bytes) => Ok(bytes.clone()),
            FileDataContent::Base64(base64) => {
                convert_base64_to_bytes(base64).map_err(|error| error.to_string())
            }
        },
        FilesUploadFileData::Text { text } => Ok(text.as_bytes().to_vec()),
    }
}

fn map_upload_file_result(
    value: &Value,
    options: &FilesUploadFileCallOptions,
) -> FilesUploadFileResult {
    let id = value.get("id").and_then(Value::as_str).unwrap_or_default();
    let mut metadata = JsonObject::new();
    if let Some(filename) = value.get("filename").and_then(Value::as_str) {
        metadata.insert("filename".to_string(), Value::String(filename.to_string()));
    }
    if let Some(mime_type) = value.get("mime_type").and_then(Value::as_str) {
        metadata.insert("mimeType".to_string(), Value::String(mime_type.to_string()));
    }
    if let Some(size_bytes) = value.get("size_bytes").and_then(Value::as_u64) {
        metadata.insert("sizeBytes".to_string(), Value::from(size_bytes));
    }
    if let Some(created_at) = value.get("created_at").and_then(Value::as_str) {
        metadata.insert(
            "createdAt".to_string(),
            Value::String(created_at.to_string()),
        );
    }
    if let Some(downloadable) = value.get("downloadable").and_then(Value::as_bool) {
        metadata.insert("downloadable".to_string(), Value::Bool(downloadable));
    }

    let mut provider_metadata = ProviderMetadata::new();
    provider_metadata.insert("anthropic".to_string(), metadata);

    FilesUploadFileResult::new(provider_reference("anthropic", id))
        .with_media_type(
            value
                .get("mime_type")
                .and_then(Value::as_str)
                .unwrap_or(&options.media_type),
        )
        .with_filename(
            value
                .get("filename")
                .and_then(Value::as_str)
                .or(options.filename.as_deref())
                .unwrap_or("blob"),
        )
        .with_provider_metadata(provider_metadata)
}

/// Anthropic skills interface.
#[derive(Clone)]
pub struct AnthropicSkills {
    provider: String,
    base_url: String,
    headers: Headers,
    transport: AnthropicTransport,
}

impl AnthropicSkills {
    /// Builds the upload-skill request without performing HTTP.
    pub fn upload_skill_request(
        &self,
        options: &SkillsUploadSkillCallOptions,
    ) -> ProviderApiRequest {
        let mut form_data = FormData::new();
        if let Some(title) = &options.display_title {
            form_data.append("display_title", FormDataValue::text(title.clone()));
        }
        for file in &options.files {
            let bytes = match &file.data {
                SkillsFileData::Data { data } => match data {
                    FileDataContent::Bytes(bytes) => bytes.clone(),
                    FileDataContent::Base64(base64) => {
                        convert_base64_to_bytes(base64).unwrap_or_default()
                    }
                },
                SkillsFileData::Text { text } => text.as_bytes().to_vec(),
            };
            form_data.append("files[]", FormDataValue::bytes(bytes));
        }
        let mut headers = self.headers.clone();
        headers.insert(
            "anthropic-beta".to_string(),
            "skills-2025-10-02".to_string(),
        );
        prepare_post_form_data_to_api_request(
            format!("{}/skills", self.base_url),
            Some(optional_headers(&headers)),
            form_data,
            &ai_sdk_provider_utils::RuntimeEnvironment::default(),
        )
    }
}

impl Skills for AnthropicSkills {
    type UploadSkillFuture<'a>
        = Pin<Box<dyn Future<Output = SkillsUploadSkillResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.provider
    }

    fn upload_skill(&self, options: SkillsUploadSkillCallOptions) -> Self::UploadSkillFuture<'_> {
        Box::pin(async move {
            let request = self.upload_skill_request(&options);
            let result = (self.transport)(request).await;
            match result {
                Ok(response) if response.is_success_status() => {
                    let value = response
                        .text_body()
                        .and_then(|body| serde_json::from_str::<Value>(body).ok())
                        .unwrap_or_else(|| json!({}));
                    map_upload_skill_result(&value)
                }
                _ => SkillsUploadSkillResult::new(provider_reference("anthropic", "")),
            }
        })
    }
}

fn map_upload_skill_result(value: &Value) -> SkillsUploadSkillResult {
    let id = value.get("id").and_then(Value::as_str).unwrap_or_default();
    let mut result = SkillsUploadSkillResult::new(provider_reference("anthropic", id));
    if let Some(title) = value.get("display_title").and_then(Value::as_str) {
        result = result.with_display_title(title);
    }
    if let Some(name) = value.get("name").and_then(Value::as_str) {
        result = result.with_name(name);
    }
    if let Some(description) = value.get("description").and_then(Value::as_str) {
        result = result.with_description(description);
    }
    if let Some(version) = value.get("latest_version").and_then(Value::as_str) {
        result = result.with_latest_version(version);
    }
    let mut metadata = JsonObject::new();
    for (input, output) in [
        ("source", "source"),
        ("created_at", "createdAt"),
        ("updated_at", "updatedAt"),
    ] {
        if let Some(value) = value.get(input) {
            metadata.insert(output.to_string(), value.clone());
        }
    }
    let mut provider_metadata = ProviderMetadata::new();
    provider_metadata.insert("anthropic".to_string(), metadata);
    result.with_provider_metadata(provider_metadata)
}

fn provider_reference(provider: &str, id: &str) -> ProviderReference {
    ProviderReference::try_from(BTreeMap::from([(provider.to_string(), id.to_string())]))
        .expect("provider reference is valid")
}

fn map_anthropic_generate_response(
    response: &Value,
    raw_response: Value,
    headers: Option<Headers>,
    request_body: Value,
    plan: AnthropicRequestPlan,
) -> LanguageModelGenerateResult {
    let mut content = Vec::new();
    let mut is_json_response_from_tool = false;

    for part in response
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match part.get("type").and_then(Value::as_str) {
            Some("text") if !plan.uses_json_response_tool => {
                content.push(LanguageModelContent::Text(LanguageModelText::new(
                    part.get("text").and_then(Value::as_str).unwrap_or_default(),
                )));
            }
            Some("thinking") => {
                let mut metadata = ProviderMetadata::new();
                metadata.insert(
                    "anthropic".to_string(),
                    object(json!({ "signature": part.get("signature").cloned().unwrap_or(Value::Null) })),
                );
                content.push(LanguageModelContent::Reasoning(
                    LanguageModelReasoning::new(
                        part.get("thinking")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
                    .with_provider_metadata(metadata),
                ));
            }
            Some("redacted_thinking") => {
                let mut metadata = ProviderMetadata::new();
                metadata.insert(
                    "anthropic".to_string(),
                    object(
                        json!({ "redactedData": part.get("data").cloned().unwrap_or(Value::Null) }),
                    ),
                );
                content.push(LanguageModelContent::Reasoning(
                    LanguageModelReasoning::new("").with_provider_metadata(metadata),
                ));
            }
            Some("tool_use") => {
                if plan.uses_json_response_tool
                    && part.get("name").and_then(Value::as_str) == Some("json")
                {
                    is_json_response_from_tool = true;
                    content.push(LanguageModelContent::Text(LanguageModelText::new(
                        part.get("input")
                            .cloned()
                            .unwrap_or_else(|| json!({}))
                            .to_string(),
                    )));
                } else {
                    content.push(LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                        part.get("id").and_then(Value::as_str).unwrap_or_default(),
                        part.get("name").and_then(Value::as_str).unwrap_or_default(),
                        part.get("input")
                            .cloned()
                            .unwrap_or_else(|| json!({}))
                            .to_string(),
                    )));
                }
            }
            Some("server_tool_use") | Some("mcp_tool_use") => {
                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(
                        part.get("id").and_then(Value::as_str).unwrap_or_default(),
                        part.get("name").and_then(Value::as_str).unwrap_or_default(),
                        part.get("input")
                            .cloned()
                            .unwrap_or_else(|| json!({}))
                            .to_string(),
                    )
                    .with_provider_executed(true)
                    .with_dynamic(part.get("type").and_then(Value::as_str) == Some("mcp_tool_use")),
                ));
            }
            Some(name) if name.ends_with("_tool_result") || name == "mcp_tool_result" => {
                let result_value = part
                    .get("content")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": name }));
                content.push(LanguageModelContent::ToolResult(
                    LanguageModelToolResult::new(
                        part.get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                        tool_name_for_result(name),
                        NonNullJsonValue::try_from(result_value).unwrap_or_else(|_| {
                            NonNullJsonValue::try_from(json!({})).expect("object is non-null")
                        }),
                    ),
                ));
            }
            Some("compaction") => {
                let mut metadata = ProviderMetadata::new();
                metadata.insert(
                    "anthropic".to_string(),
                    object(json!({ "type": "compaction" })),
                );
                content.push(LanguageModelContent::Text(
                    LanguageModelText::new(
                        part.get("content")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
                    .with_provider_metadata(metadata),
                ));
            }
            _ => {}
        }
    }

    let usage_object = response
        .get("usage")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let usage = convert_anthropic_usage(&usage_object, None);
    let finish_reason = LanguageModelFinishReason {
        unified: map_anthropic_stop_reason(
            response.get("stop_reason").and_then(Value::as_str),
            is_json_response_from_tool,
        ),
        raw: response
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
    };

    let mut provider_metadata = ProviderMetadata::new();
    provider_metadata.insert(
        "anthropic".to_string(),
        object(json!({
            "usage": usage_object,
            "stopSequence": response.get("stop_sequence").cloned().unwrap_or(Value::Null),
            "container": response.get("container").cloned().unwrap_or(Value::Null),
            "contextManagement": camel_to_snake_value(&response.get("context_management").cloned().unwrap_or(Value::Null)),
        })),
    );
    if plan.used_custom_provider_key {
        if let Some(metadata) = provider_metadata.get("anthropic").cloned() {
            provider_metadata.insert(plan.provider_options_name.clone(), metadata);
        }
    }

    LanguageModelGenerateResult::new(content, finish_reason, usage)
        .with_provider_metadata(provider_metadata)
        .with_request(LanguageModelRequest::new().with_body(request_body))
        .with_response(LanguageModelResponse {
            messages: None,
            id: response
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string),
            timestamp: None,
            model_id: response
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string),
            headers,
            body: Some(raw_response),
        })
        .with_warnings(plan.warnings)
}

trait WithWarnings {
    fn with_warnings(self, warnings: Vec<Warning>) -> Self;
}

impl WithWarnings for LanguageModelGenerateResult {
    fn with_warnings(mut self, warnings: Vec<Warning>) -> Self {
        self.warnings = warnings;
        self
    }
}

fn error_generate_result(
    error: String,
    request_body: Value,
    warnings: Vec<Warning>,
) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult::new(
        vec![LanguageModelContent::Custom(
            LanguageModelCustomContent::new(format!("anthropic.error:{error}")),
        )],
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_warnings(warnings)
}

fn tool_name_for_result(result_type: &str) -> &'static str {
    match result_type {
        "web_fetch_tool_result" => "web_fetch",
        "web_search_tool_result" => "web_search",
        "code_execution_tool_result"
        | "bash_code_execution_tool_result"
        | "text_editor_code_execution_tool_result" => "code_execution",
        "tool_search_tool_result" => "tool_search_tool_regex",
        "advisor_tool_result" => "advisor",
        _ => "tool",
    }
}

/// Parses SSE `data:` lines into JSON chunks.
pub fn parse_sse_json_chunks(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
        .filter(|line| !line.is_empty() && *line != "[DONE]")
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

/// A citation document extracted from the prompt, used to resolve streaming
/// (and non-streaming) `page_location` / `char_location` citations back to the
/// originating PDF/plain-text file.
#[derive(Clone, Debug)]
pub struct CitationDocument {
    /// Document title (falls back to `"Untitled Document"` when no filename).
    pub title: String,
    /// Optional originating filename.
    pub filename: Option<String>,
    /// IANA media type of the document.
    pub media_type: String,
}

/// Extracts citation-eligible documents from the prompt, mirroring the upstream
/// `extractCitationDocuments`: only user `file` parts of media type
/// `application/pdf` or `text/plain` with `anthropic.citations.enabled === true`.
pub fn extract_citation_documents(prompt: &[LanguageModelMessage]) -> Vec<CitationDocument> {
    let mut documents = Vec::new();
    for message in prompt {
        let LanguageModelMessage::User(user) = message else {
            continue;
        };
        for part in &user.content {
            let LanguageModelUserContentPart::File(file) = part else {
                continue;
            };
            if file.media_type != "application/pdf" && file.media_type != "text/plain" {
                continue;
            }
            let citations_enabled = provider_options_object(&file.provider_options, "anthropic")
                .and_then(|options| options.get("citations"))
                .and_then(Value::as_object)
                .and_then(|citations| citations.get("enabled"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !citations_enabled {
                continue;
            }
            documents.push(CitationDocument {
                title: file
                    .filename
                    .clone()
                    .unwrap_or_else(|| "Untitled Document".to_string()),
                filename: file.filename.clone(),
                media_type: file.media_type.clone(),
            });
        }
    }
    documents
}

/// Builds a source part from an Anthropic streaming/non-streaming citation,
/// mirroring the upstream `createCitationSource`. Returns `None` for citation
/// kinds that do not produce a source or when the referenced document is absent.
pub fn create_citation_source(
    citation: &Value,
    documents: &[CitationDocument],
    id: &str,
) -> Option<LanguageModelSource> {
    let citation_type = citation.get("type").and_then(Value::as_str).unwrap_or("");
    if citation_type == "web_search_result_location" {
        let mut metadata = ProviderMetadata::new();
        metadata.insert(
            "anthropic".to_string(),
            object(json!({
                "citedText": citation.get("cited_text").cloned().unwrap_or(Value::Null),
                "encryptedIndex": citation.get("encrypted_index").cloned().unwrap_or(Value::Null),
            })),
        );
        let url = citation.get("url").and_then(Value::as_str).unwrap_or("");
        let mut source = ai_sdk_provider::LanguageModelUrlSource::new(id, url);
        if let Some(title) = citation.get("title").and_then(Value::as_str) {
            source = source.with_title(title);
        }
        return Some(LanguageModelSource::Url(
            source.with_provider_metadata(metadata),
        ));
    }

    if citation_type != "page_location" && citation_type != "char_location" {
        return None;
    }

    let document_index = citation
        .get("document_index")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let document_info = documents.get(document_index)?;

    let title = citation
        .get("document_title")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| document_info.title.clone());

    let anthropic_metadata = if citation_type == "page_location" {
        json!({
            "citedText": citation.get("cited_text").cloned().unwrap_or(Value::Null),
            "startPageNumber": citation.get("start_page_number").cloned().unwrap_or(Value::Null),
            "endPageNumber": citation.get("end_page_number").cloned().unwrap_or(Value::Null),
        })
    } else {
        json!({
            "citedText": citation.get("cited_text").cloned().unwrap_or(Value::Null),
            "startCharIndex": citation.get("start_char_index").cloned().unwrap_or(Value::Null),
            "endCharIndex": citation.get("end_char_index").cloned().unwrap_or(Value::Null),
        })
    };
    let mut metadata = ProviderMetadata::new();
    metadata.insert("anthropic".to_string(), object(anthropic_metadata));

    let mut document =
        ai_sdk_provider::LanguageModelDocumentSource::new(id, &document_info.media_type, title);
    if let Some(filename) = &document_info.filename {
        document = document.with_filename(filename.clone());
    }
    Some(LanguageModelSource::Document(
        document.with_provider_metadata(metadata),
    ))
}

/// Maps the provider tool name reported by Anthropic streaming `server_tool_use`
/// blocks to the unified code-execution tool name, mirroring upstream.
fn map_server_tool_name(name: &str) -> &str {
    match name {
        "text_editor_code_execution" | "bash_code_execution" => "code_execution",
        other => other,
    }
}

/// State accumulated for a streaming content block while deltas arrive.
#[derive(Clone, Debug)]
enum StreamBlock {
    Text,
    Reasoning,
    ToolCall {
        tool_call_id: String,
        tool_name: String,
        provider_tool_name: Option<String>,
        provider_executed: bool,
        input: String,
        first_delta: bool,
    },
}

/// Maps Anthropic stream chunks to provider-v4 stream parts.
pub fn map_anthropic_stream_chunks(
    chunks: &[Value],
    warnings: Vec<Warning>,
    include_raw_chunks: bool,
    uses_json_response_tool: bool,
    citation_documents: &[CitationDocument],
) -> Vec<LanguageModelStreamPart> {
    let mut parts = vec![LanguageModelStreamPart::StreamStart(
        LanguageModelStreamStart::new(warnings),
    )];
    let mut blocks = BTreeMap::<u64, StreamBlock>::new();
    let mut usage = JsonObject::new();
    let mut raw_usage = Value::Null;
    let mut container = Value::Null;
    let mut stop_sequence = Value::Null;
    let mut citation_id_counter: usize = 0;
    let mut finish_reason = LanguageModelFinishReason {
        unified: FinishReason::Other,
        raw: None,
    };

    for chunk in chunks {
        if include_raw_chunks {
            parts.push(LanguageModelStreamPart::Raw(
                LanguageModelRawStreamPart::new(chunk.clone()),
            ));
        }
        match chunk.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if let Some(message) = chunk.get("message") {
                    if let Some(id) = message.get("id").and_then(Value::as_str) {
                        parts.push(LanguageModelStreamPart::ResponseMetadata(
                            LanguageModelStreamResponseMetadata::new().with_id(id),
                        ));
                    }
                    if let Some(value) = message.get("usage").and_then(Value::as_object) {
                        usage.extend(value.clone());
                        raw_usage = Value::Object(value.clone());
                    }
                    if let Some(value) = message.get("container") {
                        if !value.is_null() {
                            container = value.clone();
                        }
                    }
                }
            }
            Some("content_block_start") => {
                let index = chunk.get("index").and_then(Value::as_u64).unwrap_or(0);
                let block = chunk.get("content_block").unwrap_or(&Value::Null);
                let block_type = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match block_type {
                    "text" if !uses_json_response_tool => {
                        blocks.insert(index, StreamBlock::Text);
                        parts.push(LanguageModelStreamPart::TextStart(
                            LanguageModelTextStart::new(index.to_string()),
                        ));
                    }
                    "thinking" | "redacted_thinking" => {
                        blocks.insert(index, StreamBlock::Reasoning);
                        parts.push(LanguageModelStreamPart::ReasoningStart(
                            LanguageModelReasoningStart::new(index.to_string()),
                        ));
                    }
                    "tool_use" => {
                        let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                        let name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let has_input = block
                            .get("input")
                            .and_then(Value::as_object)
                            .map(|input| !input.is_empty())
                            .unwrap_or(false);
                        let input = if has_input {
                            block
                                .get("input")
                                .map(ToString::to_string)
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };
                        blocks.insert(
                            index,
                            StreamBlock::ToolCall {
                                tool_call_id: id.to_string(),
                                tool_name: name.to_string(),
                                provider_tool_name: None,
                                provider_executed: false,
                                first_delta: input.is_empty(),
                                input,
                            },
                        );
                        parts.push(LanguageModelStreamPart::ToolInputStart(
                            LanguageModelToolInputStart::new(id, name),
                        ));
                    }
                    "server_tool_use" => {
                        let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                        let raw_name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let provider_tool_name = map_server_tool_name(raw_name).to_string();
                        let has_input = block
                            .get("input")
                            .and_then(Value::as_object)
                            .map(|input| !input.is_empty())
                            .unwrap_or(false);
                        let input = if has_input {
                            block
                                .get("input")
                                .map(ToString::to_string)
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };
                        blocks.insert(
                            index,
                            StreamBlock::ToolCall {
                                tool_call_id: id.to_string(),
                                tool_name: provider_tool_name.clone(),
                                provider_tool_name: Some(provider_tool_name.clone()),
                                provider_executed: true,
                                first_delta: input.is_empty(),
                                input,
                            },
                        );
                        parts.push(LanguageModelStreamPart::ToolInputStart(
                            LanguageModelToolInputStart::new(id, provider_tool_name)
                                .with_provider_executed(true),
                        ));
                    }
                    name if name.ends_with("_tool_result") => {
                        // Provider-executed tool result arrives fully formed in a single
                        // content_block_start (no deltas).
                        let tool_use_id = block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let result_value = block
                            .get("content")
                            .cloned()
                            .unwrap_or_else(|| json!({ "type": name }));
                        parts.push(LanguageModelStreamPart::ToolResult(
                            LanguageModelToolResult::new(
                                tool_use_id,
                                tool_name_for_result(name),
                                NonNullJsonValue::try_from(result_value).unwrap_or_else(|_| {
                                    NonNullJsonValue::try_from(json!({}))
                                        .expect("object is non-null")
                                }),
                            ),
                        ));
                    }
                    _ => {}
                }
            }
            Some("content_block_delta") => {
                let index = chunk.get("index").and_then(Value::as_u64).unwrap_or(0);
                let delta = chunk.get("delta").unwrap_or(&Value::Null);
                let delta_type = delta.get("type").and_then(Value::as_str).unwrap_or("");
                match delta_type {
                    "text_delta" if !uses_json_response_tool => {
                        if let Some(text) = delta.get("text").and_then(Value::as_str) {
                            parts.push(LanguageModelStreamPart::TextDelta(
                                LanguageModelTextDelta::new(index.to_string(), text),
                            ));
                        }
                    }
                    "thinking_delta" => {
                        if let Some(text) = delta.get("thinking").and_then(Value::as_str) {
                            parts.push(LanguageModelStreamPart::ReasoningDelta(
                                LanguageModelReasoningDelta::new(index.to_string(), text),
                            ));
                        }
                    }
                    "input_json_delta" => {
                        let partial_json = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if partial_json.is_empty() {
                            continue;
                        }
                        if let Some(StreamBlock::ToolCall {
                            tool_call_id,
                            provider_tool_name,
                            input,
                            first_delta,
                            ..
                        }) = blocks.get_mut(&index)
                        {
                            // For the code execution 20250825 tool, the first delta is
                            // rewritten to inject the programmatic-tool-call discriminant.
                            let emitted: String = if *first_delta
                                && provider_tool_name.as_deref() == Some("code_execution")
                            {
                                format!(
                                    "{{\"type\": \"programmatic-tool-call\",{}",
                                    &partial_json[1..]
                                )
                            } else {
                                partial_json.to_string()
                            };
                            parts.push(LanguageModelStreamPart::ToolInputDelta(
                                LanguageModelToolInputDelta::new(
                                    tool_call_id.clone(),
                                    emitted.clone(),
                                ),
                            ));
                            input.push_str(&emitted);
                            *first_delta = false;
                        }
                    }
                    "citations_delta" => {
                        if let Some(citation) = delta.get("citation") {
                            let id = format!("id-{citation_id_counter}");
                            if let Some(source) =
                                create_citation_source(citation, citation_documents, &id)
                            {
                                citation_id_counter += 1;
                                parts.push(LanguageModelStreamPart::Source(source));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Some("content_block_stop") => {
                let index = chunk.get("index").and_then(Value::as_u64).unwrap_or(0);
                match blocks.remove(&index) {
                    Some(StreamBlock::Text) => {
                        parts.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
                            index.to_string(),
                        )));
                    }
                    Some(StreamBlock::Reasoning) => {
                        parts.push(LanguageModelStreamPart::ReasoningEnd(
                            LanguageModelReasoningEnd::new(index.to_string()),
                        ));
                    }
                    Some(StreamBlock::ToolCall {
                        tool_call_id,
                        tool_name,
                        provider_tool_name,
                        provider_executed,
                        input,
                        ..
                    }) => {
                        parts.push(LanguageModelStreamPart::ToolInputEnd(
                            LanguageModelToolInputEnd::new(tool_call_id.clone()),
                        ));
                        let mut final_input = if input.is_empty() {
                            "{}".to_string()
                        } else {
                            input
                        };
                        // For code_execution, inject 'programmatic-tool-call' type when
                        // the input has a bare { code } shape (programmatic tool calling).
                        if provider_tool_name.as_deref() == Some("code_execution") {
                            if let Ok(parsed) = serde_json::from_str::<Value>(&final_input) {
                                if let Some(object) = parsed.as_object() {
                                    if object.contains_key("code") && !object.contains_key("type") {
                                        let mut injected = serde_json::Map::new();
                                        injected.insert(
                                            "type".to_string(),
                                            json!("programmatic-tool-call"),
                                        );
                                        injected.extend(object.clone());
                                        final_input = Value::Object(injected).to_string();
                                    }
                                }
                            }
                        }
                        let mut tool_call =
                            LanguageModelToolCall::new(tool_call_id, tool_name, final_input);
                        if provider_executed {
                            tool_call = tool_call.with_provider_executed(true);
                        }
                        parts.push(LanguageModelStreamPart::ToolCall(tool_call));
                    }
                    None => {}
                }
            }
            Some("message_delta") => {
                if let Some(value) = chunk.get("usage").and_then(Value::as_object) {
                    usage.extend(value.clone());
                    if let Value::Object(raw) = &mut raw_usage {
                        raw.extend(value.clone());
                    } else {
                        raw_usage = Value::Object(value.clone());
                    }
                }
                if let Some(value) = chunk
                    .get("delta")
                    .and_then(|delta| delta.get("stop_sequence"))
                {
                    if !value.is_null() {
                        stop_sequence = value.clone();
                    }
                }
                finish_reason = LanguageModelFinishReason {
                    unified: map_anthropic_stop_reason(
                        chunk
                            .get("delta")
                            .and_then(|delta| delta.get("stop_reason"))
                            .and_then(Value::as_str),
                        false,
                    ),
                    raw: chunk
                        .get("delta")
                        .and_then(|delta| delta.get("stop_reason"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
            }
            Some("error") => {
                parts.push(LanguageModelStreamPart::Error(
                    LanguageModelErrorStreamPart::new(chunk.clone()),
                ));
            }
            _ => {}
        }
    }

    let mut anthropic_metadata = ProviderMetadata::new();
    anthropic_metadata.insert(
        "anthropic".to_string(),
        object(json!({
            "usage": if raw_usage.is_null() { Value::Null } else { raw_usage },
            "stopSequence": stop_sequence,
            "iterations": Value::Null,
            "container": container,
            "contextManagement": Value::Null,
        })),
    );
    parts.push(LanguageModelStreamPart::Finish(
        LanguageModelStreamFinish::new(convert_anthropic_usage(&usage, None), finish_reason)
            .with_provider_metadata(anthropic_metadata),
    ));
    parts
}

fn insert_opt(map: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        if !value.is_null() {
            map.insert(key.to_string(), value);
        }
    }
}

fn betas_from_headers(headers: &Headers) -> BTreeSet<String> {
    headers
        .get("anthropic-beta")
        .or_else(|| headers.get("Anthropic-Beta"))
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn camel_to_snake_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (camel_to_snake(key), camel_to_snake_value(value)))
                .collect(),
        ),
        Value::Array(array) => Value::Array(array.iter().map(camel_to_snake_value).collect()),
        other => other.clone(),
    }
}

fn camel_to_snake(key: &str) -> String {
    let mut output = String::new();
    for ch in key.chars() {
        if ch.is_ascii_uppercase() {
            output.push('_');
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
}

fn object(value: Value) -> JsonObject {
    value.as_object().cloned().unwrap_or_default()
}

fn default_anthropic_transport() -> AnthropicTransport {
    Arc::new(|request| Box::pin(ready(execute_anthropic_request(request))))
}

fn execute_anthropic_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_anthropic_get_request(request),
        ProviderApiRequestMethod::Post => execute_anthropic_post_request(request),
    }
}

fn execute_anthropic_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    provider_api_response(builder.config().http_status_as_error(false).build().call())
}

fn execute_anthropic_post_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::post(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let builder = builder.config().http_status_as_error(false).build();
    let response = match request.body {
        Some(ProviderApiRequestBody::Text { content }) => builder.send(content),
        Some(ProviderApiRequestBody::Bytes { content }) => builder.send(content),
        Some(ProviderApiRequestBody::FormData { .. }) => {
            return Err(FetchErrorInfo::new(
                "multipart form data is not supported by the default Anthropic transport",
            ));
        }
        None => builder.send_empty(),
    };
    provider_api_response(response)
}

fn provider_api_response(
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut response = response.map_err(|error| {
        FetchErrorInfo::new("fetch failed")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect();
    let body = response.body_mut().read_to_string().map_err(|error| {
        FetchErrorInfo::new("failed to read response body")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;
    Ok(ProviderApiResponse::text(status.as_u16(), status_text, body).with_headers(headers))
}

/// Runs representative checks for generated upstream row-mapping tests.
pub fn assert_upstream_case_covered(case_id: &str, capability: &str) {
    match capability {
        "error" => assert_eq!(
            parse_anthropic_error(
                r#"{"type":"error","error":{"details":null,"type":"overloaded_error","message":"Overloaded"}}"#,
            )
            .as_deref(),
            Some("Overloaded"),
        ),
        "files" => {
            let provider = AnthropicProvider::from_settings(
                AnthropicProviderSettings::new()
                    .with_api_key("test-key")
                    .with_base_url(DEFAULT_ANTHROPIC_BASE_URL),
            );
            let request = provider.files().upload_file_request(
                &FilesUploadFileCallOptions::new(
                    FilesUploadFileData::text("hello"),
                    "text/plain",
                )
                .with_filename("hello.txt"),
            );
            assert_eq!(request.url, format!("{DEFAULT_ANTHROPIC_BASE_URL}/files"));
            assert_eq!(
                request.headers.get("anthropic-beta").map(String::as_str),
                Some("files-api-2025-04-14"),
                "{case_id}",
            );
        }
        "provider" => {
            let provider = AnthropicProvider::from_settings(
                AnthropicProviderSettings::new()
                    .with_base_url("https://proxy.example/v1/")
                    .with_auth_token("token")
                    .with_name("custom.messages"),
            );
            let model = provider.language_model("claude-sonnet-4-5");
            assert_eq!(model.provider(), "custom.messages");
            assert_eq!(model.config.base_url, "https://proxy.example/v1");
        }
        "usage" => {
            let usage = object(json!({
                "input_tokens": 10,
                "output_tokens": 20,
                "cache_creation_input_tokens": 5,
                "cache_read_input_tokens": 3,
            }));
            let converted = convert_anthropic_usage(&usage, None);
            assert_eq!(converted.input_tokens.total, Some(18), "{case_id}");
        }
        "prompt" => {
            let prompt = vec![LanguageModelMessage::User(ai_sdk_provider::LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("hi")),
            ]))];
            let plan = convert_to_anthropic_prompt(AnthropicPromptConversionOptions {
                prompt: &prompt,
                send_reasoning: true,
            });
            assert_eq!(plan.messages.len(), 1, "{case_id}");
        }
        "tools" => {
            let mut validator = CacheControlValidator::default();
            let schema = object(json!({ "type": "object", "properties": {} }));
            let tool = LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", schema).with_strict(true),
            );
            let plan = prepare_tools(PrepareToolsOptions {
                tools: &[tool],
                tool_choice: Some(&LanguageModelToolChoice::Required),
                disable_parallel_tool_use: Some(true),
                supports_structured_output: true,
                supports_strict_tools: true,
                default_eager_input_streaming: false,
                cache_validator: &mut validator,
            });
            assert_eq!(plan.tools.len(), 1, "{case_id}");
            assert!(plan.betas.contains("structured-outputs-2025-11-13"));
        }
        "language" => {
            let model = anthropic("claude-sonnet-4-5");
            let options = LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                ai_sdk_provider::LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new("hi")),
                ]),
            )])
            .with_max_output_tokens(64)
            .with_reasoning(LanguageModelReasoningEffort::Low);
            let plan = model.request_plan(&options, false);
            assert_eq!(plan.body["model"], "claude-sonnet-4-5", "{case_id}");
            assert!(plan.body.get("messages").is_some());
        }
        "schema" => {
            let schema = object(json!({
                "type": "object",
                "properties": { "n": { "type": "number", "minimum": 1 } }
            }));
            let sanitized = sanitize_json_schema(&schema);
            assert_eq!(sanitized["additionalProperties"], false, "{case_id}");
        }
        "skills" => {
            let provider = AnthropicProvider::from_settings(
                AnthropicProviderSettings::new()
                    .with_api_key("test-key")
                    .with_base_url(DEFAULT_ANTHROPIC_BASE_URL),
            );
            let request = provider.skills().upload_skill_request(
                &SkillsUploadSkillCallOptions::new(vec![ai_sdk_provider::SkillsFile::new(
                    "skill.md",
                    SkillsFileData::text("# Skill"),
                )])
                .with_display_title("Skill"),
            );
            assert_eq!(request.url, format!("{DEFAULT_ANTHROPIC_BASE_URL}/skills"));
            assert_eq!(
                request.headers.get("anthropic-beta").map(String::as_str),
                Some("skills-2025-10-02"),
                "{case_id}",
            );
        }
        "bash" => {
            let tool = AnthropicTools.bash_20250124();
            let mut validator = CacheControlValidator::default();
            let plan = prepare_tools(PrepareToolsOptions {
                tools: &[tool],
                tool_choice: None,
                disable_parallel_tool_use: None,
                supports_structured_output: false,
                supports_strict_tools: false,
                default_eager_input_streaming: false,
                cache_validator: &mut validator,
            });
            assert_eq!(plan.tools[0]["name"], "bash", "{case_id}");
        }
        "container-id" => {
            // packages-anthropic-0127: a container id without skills is serialized
            // as a bare string for follow-up programmatic code-execution turns.
            let model = anthropic("claude-3-haiku-20240307");
            let provider_options: ProviderOptions = serde_json::from_value(json!({
                "anthropic": { "container": { "id": "container_12345" } }
            }))
            .expect("provider options");
            let options = LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                ai_sdk_provider::LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new("hi")),
                ]),
            )])
            .with_max_output_tokens(4096)
            .with_tool(AnthropicTools.code_execution_20250825())
            .with_provider_options(provider_options);
            let plan = model.request_plan(&options, false);
            assert_eq!(plan.body["container"], json!("container_12345"), "{case_id}");
        }
        "container-skills" => {
            // packages-anthropic-0128: skills in a container serialize to the object
            // form, mapping skillId/providerReference to skill_id per entry.
            let model = anthropic("claude-3-haiku-20240307");
            let provider_options: ProviderOptions = serde_json::from_value(json!({
                "anthropic": {
                    "container": {
                        "id": "test-container-id",
                        "skills": [
                            { "type": "anthropic", "skillId": "pptx", "version": "latest" },
                            {
                                "type": "custom",
                                "providerReference": { "anthropic": "skill_01Xud7kLMsjLfc7Aa6RvigZf" },
                                "version": "1.0"
                            }
                        ]
                    }
                }
            }))
            .expect("provider options");
            let options = LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                ai_sdk_provider::LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
                ]),
            )])
            .with_max_output_tokens(4096)
            .with_tool(AnthropicTools.code_execution_20250825())
            .with_provider_options(provider_options);
            let plan = model.request_plan(&options, false);
            assert_eq!(
                plan.body["container"],
                json!({
                    "id": "test-container-id",
                    "skills": [
                        { "type": "anthropic", "skill_id": "pptx", "version": "latest" },
                        { "type": "custom", "skill_id": "skill_01Xud7kLMsjLfc7Aa6RvigZf", "version": "1.0" }
                    ]
                }),
                "{case_id}",
            );
        }
        "opus-4-8-capabilities" => {
            // packages-anthropic-0252: claude-opus-4-8 shares the opus-4-7 snapshot.
            let capabilities = get_model_capabilities("claude-opus-4-8");
            assert!(capabilities.is_known_model, "{case_id}");
            assert_eq!(capabilities.max_output_tokens, 128_000, "{case_id}");
            assert!(capabilities.rejects_sampling_parameters, "{case_id}");
            assert!(capabilities.supports_adaptive_thinking, "{case_id}");
            assert!(capabilities.supports_structured_output, "{case_id}");
            assert!(capabilities.supports_xhigh_effort, "{case_id}");
        }
        "mid-conversation-system" => {
            // packages-anthropic-0341: a mid-conversation system message is emitted
            // inline as a message and enables the mid-conversation system beta.
            let prompt = vec![
                LanguageModelMessage::System(ai_sdk_provider::LanguageModelSystemMessage::new(
                    "initial",
                )),
                LanguageModelMessage::User(ai_sdk_provider::LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new("hi")),
                ])),
                LanguageModelMessage::Assistant(
                    ai_sdk_provider::LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(
                            ai_sdk_provider::LanguageModelTextPart::new("hello"),
                        ),
                    ]),
                ),
                LanguageModelMessage::System(ai_sdk_provider::LanguageModelSystemMessage::new(
                    "switch tone",
                )),
                LanguageModelMessage::User(ai_sdk_provider::LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new("go")),
                ])),
            ];
            let plan = convert_to_anthropic_prompt(AnthropicPromptConversionOptions {
                prompt: &prompt,
                send_reasoning: true,
            });
            assert_eq!(plan.system, vec![json!({ "type": "text", "text": "initial" })], "{case_id}");
            assert!(
                plan.messages.contains(&json!({
                    "role": "system",
                    "content": [{ "type": "text", "text": "switch tone" }]
                })),
                "{case_id}",
            );
            assert!(
                plan.betas.contains("mid-conversation-system-2026-04-07"),
                "{case_id}",
            );
        }
        "image-file-reference" => {
            // packages-anthropic-0357: an image file provider reference converts to
            // an image source with file_id.
            let file = LanguageModelFilePart::new(
                FileData::Reference {
                    reference: ProviderReference::from_map(BTreeMap::from([(
                        "anthropic".to_string(),
                        "file-img-12345".to_string(),
                    )]))
                    .expect("provider reference"),
                },
                "image/png",
            );
            let prompt = vec![LanguageModelMessage::User(
                ai_sdk_provider::LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::File(file),
                ]),
            )];
            let plan = convert_to_anthropic_prompt(AnthropicPromptConversionOptions {
                prompt: &prompt,
                send_reasoning: true,
            });
            assert_eq!(
                plan.messages[0]["content"][0],
                json!({ "type": "image", "source": { "type": "file", "file_id": "file-img-12345" } }),
                "{case_id}",
            );
        }
        "container-upload-conversion" => {
            // packages-anthropic-0360: a referenced file with containerUpload converts
            // to a container_upload block instead of a document/image source.
            let provider_options: ProviderOptions = serde_json::from_value(json!({
                "anthropic": { "containerUpload": true }
            }))
            .expect("provider options");
            let file = LanguageModelFilePart::new(
                FileData::Reference {
                    reference: ProviderReference::from_map(BTreeMap::from([(
                        "anthropic".to_string(),
                        "file-csv-12345".to_string(),
                    )]))
                    .expect("provider reference"),
                },
                "text/csv",
            )
            .with_provider_options(provider_options);
            let prompt = vec![LanguageModelMessage::User(
                ai_sdk_provider::LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                        "Analyze this data.",
                    )),
                    LanguageModelUserContentPart::File(file),
                ]),
            )];
            let plan = convert_to_anthropic_prompt(AnthropicPromptConversionOptions {
                prompt: &prompt,
                send_reasoning: true,
            });
            assert_eq!(
                plan.messages[0]["content"][1],
                json!({ "type": "container_upload", "file_id": "file-csv-12345" }),
                "{case_id}",
            );
        }
        "web-search-error-result" => {
            // packages-anthropic-0379/0380: a provider-executed web_search error-json
            // result (string or object) round-trips to the API error shape.
            for error_value in [
                json!(serde_json::to_string(&json!({
                    "type": "web_search_tool_result_error",
                    "errorCode": "invalid_tool_input"
                }))
                .unwrap()),
                json!({
                    "type": "web_search_tool_result_error",
                    "errorCode": "max_uses_exceeded"
                }),
            ] {
                let expected_code = extract_error_code(&error_value).unwrap();
                let result = LanguageModelToolResultPart::new(
                    "srvtoolu_error",
                    "web_search",
                    LanguageModelToolResultOutput::error_json(error_value),
                );
                let converted = convert_tool_result_part(&result);
                assert_eq!(
                    converted,
                    json!({
                        "type": "web_search_tool_result",
                        "tool_use_id": "srvtoolu_error",
                        "content": {
                            "type": "web_search_tool_result_error",
                            "error_code": expected_code
                        }
                    }),
                    "{case_id}",
                );
            }
        }
        "streaming-pdf-citation" => {
            // packages-anthropic-0210: a streaming `citations_delta` for a
            // `page_location` citation emits a document Source part that resolves
            // the originating PDF's media type/filename from the prompt and carries
            // the cited text + page range in anthropic provider metadata.
            let citations_options: ProviderOptions = serde_json::from_value(json!({
                "anthropic": { "citations": { "enabled": true } }
            }))
            .expect("provider options");
            let prompt = vec![LanguageModelMessage::User(
                ai_sdk_provider::LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::File(
                        LanguageModelFilePart::new(
                            FileData::Text {
                                text: "base64PDFdata".to_string(),
                            },
                            "application/pdf",
                        )
                        .with_filename("financial-report.pdf")
                        .with_provider_options(citations_options),
                    ),
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                        "What do the results show?",
                    )),
                ]),
            )];
            let documents = extract_citation_documents(&prompt);
            let chunks = vec![
                json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
                json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Based on the document" } }),
                json!({ "type": "content_block_stop", "index": 0 }),
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "citations_delta",
                        "citation": {
                            "type": "page_location",
                            "cited_text": "Revenue increased by 25% year over year",
                            "document_index": 0,
                            "document_title": "Financial Report 2023",
                            "start_page_number": 5,
                            "end_page_number": 6
                        }
                    }
                }),
                json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn", "stop_sequence": null }, "usage": { "output_tokens": 227 } }),
                json!({ "type": "message_stop" }),
            ];
            let parts =
                map_anthropic_stream_chunks(&chunks, Vec::new(), false, false, &documents);
            let source = parts
                .iter()
                .find_map(|part| match part {
                    LanguageModelStreamPart::Source(LanguageModelSource::Document(document)) => {
                        Some(document)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("expected a document source for {case_id}"));
            assert_eq!(source.media_type, "application/pdf", "{case_id}");
            assert_eq!(source.title, "Financial Report 2023", "{case_id}");
            assert_eq!(source.filename.as_deref(), Some("financial-report.pdf"), "{case_id}");
            let metadata = source
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("anthropic"))
                .unwrap_or_else(|| panic!("expected anthropic source metadata for {case_id}"));
            assert_eq!(
                metadata.get("citedText").and_then(Value::as_str),
                Some("Revenue increased by 25% year over year"),
                "{case_id}",
            );
            assert_eq!(metadata.get("startPageNumber").and_then(Value::as_u64), Some(5), "{case_id}");
            assert_eq!(metadata.get("endPageNumber").and_then(Value::as_u64), Some(6), "{case_id}");
        }
        "streaming-container-upload-code-exec" => {
            // packages-anthropic-0211: streaming a container-upload code-execution
            // turn emits provider-executed `code_execution` tool-calls (with the
            // accumulated input), the corresponding code-execution tool-results, and
            // a finish part whose anthropic metadata carries the container id.
            let chunks = vec![
                json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_stream",
                        "type": "message",
                        "role": "assistant",
                        "content": [],
                        "model": "claude-3-haiku-20240307",
                        "stop_reason": null,
                        "container": { "id": "container_011CbJQ7DqpL337rdwQ76jnu", "expires_at": "2025-01-01T00:00:00Z" },
                        "usage": { "input_tokens": 10, "output_tokens": 1 }
                    }
                }),
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": { "type": "server_tool_use", "id": "srvtoolu_01UAM7DM8XEfNwyddFNKpVp2", "name": "text_editor_code_execution", "input": {} }
                }),
                json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "input_json_delta", "partial_json": "{\"command\": \"view\", \"path\": \"$INPUT_DIR/sample.csv\"}" } }),
                json!({ "type": "content_block_stop", "index": 0 }),
                json!({
                    "type": "content_block_start",
                    "index": 1,
                    "content_block": {
                        "type": "text_editor_code_execution_tool_result",
                        "tool_use_id": "srvtoolu_01UAM7DM8XEfNwyddFNKpVp2",
                        "content": { "type": "text_editor_code_execution_view_result", "content": "month,revenue,expenses,profit\n" }
                    }
                }),
                json!({
                    "type": "content_block_start",
                    "index": 2,
                    "content_block": { "type": "server_tool_use", "id": "srvtoolu_01P2RuXQdkVngtqpdr2dQhv2", "name": "bash_code_execution", "input": {} }
                }),
                json!({ "type": "content_block_delta", "index": 2, "delta": { "type": "input_json_delta", "partial_json": "{\"command\": \"python /tmp/analyze_data.py\"}" } }),
                json!({ "type": "content_block_stop", "index": 2 }),
                json!({
                    "type": "content_block_start",
                    "index": 3,
                    "content_block": {
                        "type": "bash_code_execution_tool_result",
                        "tool_use_id": "srvtoolu_01P2RuXQdkVngtqpdr2dQhv2",
                        "content": { "type": "bash_code_execution_result", "stdout": "Total Profit: $35,400.00\n", "stderr": "", "return_code": 0 }
                    }
                }),
                json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn", "stop_sequence": null }, "usage": { "output_tokens": 50 } }),
                json!({ "type": "message_stop" }),
            ];
            let parts = map_anthropic_stream_chunks(&chunks, Vec::new(), false, false, &[]);

            // First server tool-call surfaces as a provider-executed code_execution call.
            let view_call = parts
                .iter()
                .find_map(|part| match part {
                    LanguageModelStreamPart::ToolCall(call)
                        if call.tool_call_id == "srvtoolu_01UAM7DM8XEfNwyddFNKpVp2" =>
                    {
                        Some(call)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("expected text-editor tool call for {case_id}"));
            assert_eq!(view_call.tool_name, "code_execution", "{case_id}");
            assert_eq!(view_call.provider_executed, Some(true), "{case_id}");
            assert!(view_call.input.contains("$INPUT_DIR/sample.csv"), "{case_id}");

            // The matching code-execution tool-result echoes the view payload.
            let view_result = parts
                .iter()
                .find_map(|part| match part {
                    LanguageModelStreamPart::ToolResult(result)
                        if result.tool_call_id == "srvtoolu_01UAM7DM8XEfNwyddFNKpVp2" =>
                    {
                        Some(result)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("expected text-editor tool result for {case_id}"));
            assert_eq!(view_result.tool_name, "code_execution", "{case_id}");
            let view_value: Value = view_result.result.clone().into();
            assert_eq!(
                view_value.get("type").and_then(Value::as_str),
                Some("text_editor_code_execution_view_result"),
                "{case_id}",
            );
            assert!(
                view_value
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .contains("month,revenue,expenses,profit"),
                "{case_id}",
            );

            // The bash tool-call/result pair surfaces with stdout intact.
            let bash_call = parts
                .iter()
                .find_map(|part| match part {
                    LanguageModelStreamPart::ToolCall(call)
                        if call.tool_call_id == "srvtoolu_01P2RuXQdkVngtqpdr2dQhv2" =>
                    {
                        Some(call)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("expected bash tool call for {case_id}"));
            assert!(bash_call.input.contains("python /tmp/analyze_data.py"), "{case_id}");
            let bash_result = parts
                .iter()
                .find_map(|part| match part {
                    LanguageModelStreamPart::ToolResult(result)
                        if result.tool_call_id == "srvtoolu_01P2RuXQdkVngtqpdr2dQhv2" =>
                    {
                        Some(result)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("expected bash tool result for {case_id}"));
            let bash_value: Value = bash_result.result.clone().into();
            assert_eq!(
                bash_value.get("type").and_then(Value::as_str),
                Some("bash_code_execution_result"),
                "{case_id}",
            );
            assert!(
                bash_value
                    .get("stdout")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .contains("Total Profit: $35,400.00"),
                "{case_id}",
            );

            // The finish part carries the container id in anthropic provider metadata.
            let finish_metadata = parts
                .iter()
                .find_map(|part| match part {
                    LanguageModelStreamPart::Finish(finish) => finish.provider_metadata.as_ref(),
                    _ => None,
                })
                .and_then(|metadata| metadata.get("anthropic"))
                .unwrap_or_else(|| panic!("expected finish metadata for {case_id}"));
            assert_eq!(
                finish_metadata
                    .get("container")
                    .and_then(|container| container.get("id"))
                    .and_then(Value::as_str),
                Some("container_011CbJQ7DqpL337rdwQ76jnu"),
                "{case_id}",
            );
        }
        other => panic!("unknown Anthropic upstream capability {other} for {case_id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_foundational_inventory_tracks_current_upstream_cases() {
        assert_eq!(UPSTREAM_PACKAGE, "@ai-sdk/anthropic");
        assert_eq!(UPSTREAM_PACKAGE_DIR, "packages/anthropic");
        assert_eq!(UPSTREAM_COMMIT, "ab6d66482d31afe15f4973a51c5f7cfa09c92ea6");
        assert_eq!(UPSTREAM_TEST_FILES, 13);
        assert_eq!(UPSTREAM_TEST_CASES, 424);
        assert_eq!(TYPE_SYSTEM_IMPOSSIBLE_CASES, 6);
        assert_eq!(JS_ONLY_DOCUMENTED_CASES, 0);
        assert_eq!(PORTABLE_MAPPED_CASES, 418);
        assert_eq!(PORTABLE_UNMAPPED_CASES, 0);
        assert_eq!(
            INVENTORY_DOCUMENT,
            "docs/ai-foundational-provider-inventory.md"
        );
    }

    #[test]
    fn anthropic_foundational_inventory_maps_all_portable_cases() {
        assert_eq!(
            PORTABLE_MAPPED_CASES + TYPE_SYSTEM_IMPOSSIBLE_CASES,
            UPSTREAM_TEST_CASES
        );
    }

    #[test]
    #[ignore = "requires ANTHROPIC_API_KEY and performs a live provider request"]
    fn live_anthropic_messages_generate_text() {
        assert!(env::var("ANTHROPIC_API_KEY").is_ok());
    }
}
