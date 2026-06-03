//! Rust port of the upstream `@ai-sdk/quiverai` SVG image generation provider.
//!
//! QuiverAI exposes a single capability: text-to-SVG generation and raster
//! image vectorization through the `arrow-1`, `arrow-1.1`, and `arrow-1.1-max`
//! image models. This crate ports the deterministic, behavior-bearing portions
//! of the upstream package (`packages/quiverai/src`):
//!
//! - the failed-response handler that maps QuiverAI error envelopes into
//!   [`ApiCallError`] values with the provider-specific retry rule
//!   (`quiverai-image-model.ts` `quiveraiFailedResponseHandler`),
//! - the request-body builder for generate and vectorize operations including
//!   reference-image limits and shared sampling options
//!   (`quiverai-image-model.ts` `buildRequestBody`),
//! - operation-path routing, unsupported-feature warnings, provider metadata,
//!   and usage mapping,
//! - the provider surface (`quiverai-provider.ts` `createQuiverAI`): base URL
//!   resolution, auth/user-agent headers, image factory methods, and
//!   `NoSuchModelError` for language/embedding lookups.

use ai_sdk_rust::{
    ApiCallError, FileDataContent, Headers, ImageModelCallOptions, ImageModelFile,
    InvalidArgumentError, JsonObject, JsonValue, LoadApiKeyError, LoadApiKeyOptions,
    LoadOptionalSettingOptions, ModelType, NoSuchModelError, ProviderOptions, Warning,
    convert_bytes_to_base64, load_api_key, load_optional_setting, parse_provider_options,
    with_user_agent_suffix, without_trailing_slash,
};
use serde::{Deserialize, Serialize};

/// Default base URL for upstream `@ai-sdk/quiverai` API calls.
pub const DEFAULT_QUIVERAI_BASE_URL: &str = "https://api.quiver.ai/v1";

/// Version string injected into the user-agent suffix.
///
/// Mirrors the upstream `version.ts` test fallback (`0.0.0-test`).
pub const VERSION: &str = "0.0.0-test";

/// The canonical QuiverAI image model ids.
pub const CANONICAL_MODEL_IDS: &[&str] = &["arrow-1", "arrow-1.1", "arrow-1.1-max"];

/// The QuiverAI image operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum QuiverAIOperation {
    /// Text-to-SVG generation (default).
    #[default]
    Generate,
    /// Convert a raster image into an SVG.
    Vectorize,
}

impl QuiverAIOperation {
    /// Returns the API path suffix appended to the base URL for this operation.
    pub fn path(self) -> &'static str {
        match self {
            QuiverAIOperation::Generate => "/svgs/generations",
            QuiverAIOperation::Vectorize => "/svgs/vectorizations",
        }
    }
}

/// Settings for the upstream QuiverAI provider (`QuiverAIProviderSettings`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuiverAIProviderSettings {
    /// QuiverAI API key. When omitted, `QUIVERAI_API_KEY` is read at request time.
    pub api_key: Option<String>,

    /// Base URL for API calls. Falls back to `QUIVERAI_BASE_URL`, then the default.
    pub base_url: Option<String>,

    /// Custom provider-level headers included with each request.
    pub headers: Headers,
}

impl QuiverAIProviderSettings {
    /// Creates empty QuiverAI provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the QuiverAI API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the base URL used for API calls.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// A resolver for environment-backed settings.
///
/// The default reads the process environment (upstream parity). Tests inject an
/// explicit map so they exercise the same resolution logic without mutating the
/// process environment (which the crate's `forbid(unsafe_code)` lint disallows).
#[derive(Clone, Debug, Default)]
enum EnvSource {
    /// Read from the process environment via the shared provider-utils helpers.
    #[default]
    Process,
    /// Read from an explicit map (test injection).
    Explicit(std::collections::BTreeMap<String, String>),
}

impl EnvSource {
    fn get(&self, name: &str) -> Option<String> {
        match self {
            EnvSource::Process => None,
            EnvSource::Explicit(map) => map.get(name).cloned(),
        }
    }
}

/// Upstream QuiverAI provider (`createQuiverAI`).
#[derive(Clone, Debug)]
pub struct QuiverAIProvider {
    base_url: String,
    settings: QuiverAIProviderSettings,
    env: EnvSource,
}

impl QuiverAIProvider {
    /// Creates a QuiverAI provider with explicit settings (`createQuiverAI`).
    ///
    /// The base URL resolves from the explicit setting, then `QUIVERAI_BASE_URL`,
    /// then the default, with any trailing slash removed.
    pub fn new(settings: QuiverAIProviderSettings) -> Self {
        Self::from_settings_and_env(settings, EnvSource::Process)
    }

    fn from_settings_and_env(settings: QuiverAIProviderSettings, env: EnvSource) -> Self {
        let resolved = match settings.base_url.clone() {
            Some(value) => Some(value),
            None => match env.get("QUIVERAI_BASE_URL") {
                Some(value) => Some(value),
                None => load_optional_setting(LoadOptionalSettingOptions::new("QUIVERAI_BASE_URL")),
            },
        };
        let base_url = without_trailing_slash(resolved.as_deref())
            .unwrap_or(DEFAULT_QUIVERAI_BASE_URL)
            .to_string();

        Self {
            base_url,
            settings,
            env,
        }
    }

    /// The resolved base URL for this provider.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Creates an image model (`provider.image`).
    pub fn image(&self, model_id: impl Into<String>) -> QuiverAIImageModel {
        QuiverAIImageModel::new(model_id, self.base_url.clone())
    }

    /// Creates an image model (`provider.imageModel`).
    pub fn image_model(&self, model_id: impl Into<String>) -> QuiverAIImageModel {
        self.image(model_id)
    }

    /// QuiverAI does not support language models (`provider.languageModel`).
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<core::convert::Infallible, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    /// QuiverAI does not support embedding models (`provider.embeddingModel`).
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<core::convert::Infallible, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias (`provider.textEmbeddingModel`).
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<core::convert::Infallible, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Builds the request headers for an image call, resolving the API key and
    /// appending the QuiverAI user-agent suffix (`createQuiverAI` `getHeaders`).
    ///
    /// Errors with [`LoadApiKeyError`] when no API key is configured and the
    /// `QUIVERAI_API_KEY` environment variable is unset.
    pub fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<Headers, LoadApiKeyError> {
        let api_key_value = match self.settings.api_key.clone() {
            Some(api_key) => Some(api_key),
            None => self.env.get("QUIVERAI_API_KEY"),
        };
        let api_key = load_api_key(
            LoadApiKeyOptions::new("QUIVERAI_API_KEY", "QuiverAI").with_api_key_opt(api_key_value),
        )?;

        let mut entries: Vec<(String, Option<String>)> = Vec::new();
        entries.push((
            "Authorization".to_string(),
            Some(format!("Bearer {api_key}")),
        ));
        for (name, value) in &self.settings.headers {
            entries.push((name.clone(), Some(value.clone())));
        }
        if let Some(call_headers) = call_headers {
            for (name, value) in call_headers {
                entries.push((name.clone(), Some(value.clone())));
            }
        }

        let normalized =
            with_user_agent_suffix(Some(entries), [format!("ai-sdk/quiverai/{VERSION}")]);
        Ok(normalized)
    }
}

/// Upstream QuiverAI image model (`QuiverAIImageModel`).
#[derive(Clone, Debug)]
pub struct QuiverAIImageModel {
    model_id: String,
    base_url: String,
}

impl QuiverAIImageModel {
    fn new(model_id: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            base_url: base_url.into(),
        }
    }

    /// The provider id, always `quiverai.image`.
    pub fn provider(&self) -> &str {
        "quiverai.image"
    }

    /// The model id (e.g. `arrow-1`).
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// The image model specification version, always `v4`.
    pub fn specification_version(&self) -> &str {
        "v4"
    }

    /// The maximum number of images returned per call, always 16.
    pub fn max_images_per_call(&self) -> u32 {
        16
    }

    /// The fully-qualified request URL for the resolved operation.
    pub fn request_url(&self, operation: QuiverAIOperation) -> String {
        format!("{}{}", self.base_url, operation.path())
    }
}

/// Parsed QuiverAI provider options (`quiveraiImageModelOptionsSchema`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuiverAIImageModelOptions {
    /// The operation to perform; defaults to `generate`.
    #[serde(default)]
    pub operation: Option<String>,
    /// Extra style guidance for prompt-based generation.
    #[serde(default)]
    pub instructions: Option<String>,
    /// Sampling temperature (0-2).
    #[serde(default)]
    pub temperature: Option<f64>,
    /// Nucleus sampling top-p (0-1).
    #[serde(default, rename = "topP")]
    pub top_p: Option<f64>,
    /// Presence penalty (-2 to 2).
    #[serde(default, rename = "presencePenalty")]
    pub presence_penalty: Option<f64>,
    /// Maximum number of output tokens (1 - 131072).
    #[serde(default, rename = "maxOutputTokens")]
    pub max_output_tokens: Option<u64>,
    /// Whether to auto-crop the input image before vectorization.
    #[serde(default, rename = "autoCrop")]
    pub auto_crop: Option<bool>,
    /// Target canvas size in pixels for vectorization (128 - 4096).
    #[serde(default, rename = "targetSize")]
    pub target_size: Option<u64>,
}

impl QuiverAIImageModelOptions {
    fn validate(&self) -> Result<(), String> {
        if let Some(operation) = self.operation.as_deref() {
            if operation != "generate" && operation != "vectorize" {
                return Err(format!("invalid operation: {operation}"));
            }
        }
        if let Some(instructions) = self.instructions.as_deref() {
            if instructions.is_empty() {
                return Err("instructions must be non-empty".to_string());
            }
        }
        if let Some(temperature) = self.temperature {
            if !(0.0..=2.0).contains(&temperature) {
                return Err("temperature must be between 0 and 2".to_string());
            }
        }
        if let Some(top_p) = self.top_p {
            if !(0.0..=1.0).contains(&top_p) {
                return Err("topP must be between 0 and 1".to_string());
            }
        }
        if let Some(presence_penalty) = self.presence_penalty {
            if !(-2.0..=2.0).contains(&presence_penalty) {
                return Err("presencePenalty must be between -2 and 2".to_string());
            }
        }
        if let Some(max_output_tokens) = self.max_output_tokens {
            if !(1..=131_072).contains(&max_output_tokens) {
                return Err("maxOutputTokens must be between 1 and 131072".to_string());
            }
        }
        if let Some(target_size) = self.target_size {
            if !(128..=4096).contains(&target_size) {
                return Err("targetSize must be between 128 and 4096".to_string());
            }
        }
        Ok(())
    }

    fn operation(&self) -> QuiverAIOperation {
        match self.operation.as_deref() {
            Some("vectorize") => QuiverAIOperation::Vectorize,
            _ => QuiverAIOperation::Generate,
        }
    }
}

fn quiverai_image_model_options(value: &JsonValue) -> Result<QuiverAIImageModelOptions, String> {
    let options = serde_json::from_value::<QuiverAIImageModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate()?;
    Ok(options)
}

/// Resolves the operation requested through provider options, defaulting to
/// `generate`. Returns the resolved operation and the parsed options.
fn resolve_quiverai_options(
    provider_options: &ProviderOptions,
) -> Result<(QuiverAIOperation, QuiverAIImageModelOptions), InvalidArgumentError> {
    let options = parse_provider_options(
        "quiverai",
        Some(provider_options),
        quiverai_image_model_options,
    )?
    .unwrap_or_default();
    let operation = options.operation();
    Ok((operation, options))
}

/// The reference-image limit for `generate` calls on a given model.
fn generate_reference_limit(model_id: &str) -> usize {
    if model_id == "arrow-1.1-max" { 16 } else { 4 }
}

/// Maps an image file input into a QuiverAI reference object (`url` or `base64`).
fn to_quiverai_image_reference(image: &ImageModelFile) -> JsonValue {
    match image {
        ImageModelFile::Url { url, .. } => {
            let mut object = JsonObject::new();
            object.insert("url".to_string(), JsonValue::String(url.to_string()));
            JsonValue::Object(object)
        }
        ImageModelFile::File { data, .. } => {
            let base64 = match data {
                FileDataContent::Base64(base64) => base64.clone(),
                FileDataContent::Bytes(bytes) => convert_bytes_to_base64(bytes),
            };
            let mut object = JsonObject::new();
            object.insert("base64".to_string(), JsonValue::String(base64));
            JsonValue::Object(object)
        }
    }
}

fn insert_f64(object: &mut JsonObject, key: &str, value: Option<f64>) {
    if let Some(value) = value {
        if let Some(number) = serde_json::Number::from_f64(value) {
            object.insert(key.to_string(), JsonValue::Number(number));
        }
    }
}

fn insert_u64(object: &mut JsonObject, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        object.insert(key.to_string(), JsonValue::from(value));
    }
}

/// Builds the QuiverAI request body for the resolved operation
/// (`quiverai-image-model.ts` `buildRequestBody`).
///
/// For `generate`, requires a non-empty prompt and enforces the per-model
/// reference-image limit. For `vectorize`, requires exactly one input image.
/// Both share the sampling options and `stream: false`.
pub fn build_request_body(
    model_id: &str,
    n: u64,
    prompt: Option<&str>,
    files: Option<&[ImageModelFile]>,
    operation: QuiverAIOperation,
    options: &QuiverAIImageModelOptions,
) -> Result<JsonValue, InvalidArgumentError> {
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));

    if operation == QuiverAIOperation::Generate {
        let prompt = prompt.unwrap_or("");
        if prompt.trim().is_empty() {
            return Err(InvalidArgumentError::new(
                "prompt",
                "QuiverAI image generation requires a non-empty prompt for generateImage.",
            ));
        }

        let references: Option<Vec<JsonValue>> =
            files.map(|files| files.iter().map(to_quiverai_image_reference).collect());
        let max_references = generate_reference_limit(model_id);
        if let Some(references) = references.as_ref() {
            if references.len() > max_references {
                return Err(InvalidArgumentError::new(
                    "files",
                    format!(
                        "QuiverAI generate supports up to {max_references} reference images for model \"{model_id}\"."
                    ),
                ));
            }
        }

        body.insert("n".to_string(), JsonValue::from(n));
        body.insert("prompt".to_string(), JsonValue::String(prompt.to_string()));
        insert_shared_options(&mut body, options);
        if let Some(instructions) = options.instructions.as_ref() {
            body.insert(
                "instructions".to_string(),
                JsonValue::String(instructions.clone()),
            );
        }
        if let Some(references) = references {
            body.insert("references".to_string(), JsonValue::Array(references));
        }
        return Ok(JsonValue::Object(body));
    }

    // Vectorize.
    let files = files.unwrap_or(&[]);
    if files.is_empty() {
        return Err(InvalidArgumentError::new(
            "files",
            "QuiverAI vectorize requires an input image. Pass an image in the generateImage prompt and set providerOptions.quiverai.operation to \"vectorize\".",
        ));
    }
    if files.len() > 1 {
        return Err(InvalidArgumentError::new(
            "files",
            "QuiverAI vectorize accepts a single input image.",
        ));
    }

    body.insert("n".to_string(), JsonValue::from(n));
    body.insert("image".to_string(), to_quiverai_image_reference(&files[0]));
    insert_shared_options(&mut body, options);
    if let Some(auto_crop) = options.auto_crop {
        body.insert("auto_crop".to_string(), JsonValue::Bool(auto_crop));
    }
    insert_u64(&mut body, "target_size", options.target_size);
    Ok(JsonValue::Object(body))
}

fn insert_shared_options(body: &mut JsonObject, options: &QuiverAIImageModelOptions) {
    insert_f64(body, "temperature", options.temperature);
    insert_f64(body, "top_p", options.top_p);
    insert_f64(body, "presence_penalty", options.presence_penalty);
    insert_u64(body, "max_output_tokens", options.max_output_tokens);
    body.insert("stream".to_string(), JsonValue::Bool(false));
}

/// Collects the unsupported-feature warnings for the QuiverAI image model
/// (`quiverai-image-model.ts` `collectWarnings`).
pub fn collect_warnings(options: &ImageModelCallOptions) -> Vec<Warning> {
    let mut warnings = Vec::new();
    if options.size.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "size".to_string(),
            details: Some(
                "QuiverAI SVG generation does not support the `size` option. The setting was ignored.".to_string(),
            ),
        });
    }
    if options.aspect_ratio.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "aspectRatio".to_string(),
            details: Some(
                "QuiverAI SVG generation does not support the `aspectRatio` option. The setting was ignored.".to_string(),
            ),
        });
    }
    if options.seed.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "seed".to_string(),
            details: Some(
                "QuiverAI SVG generation does not support the `seed` option. The setting was ignored.".to_string(),
            ),
        });
    }
    if options.mask.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "mask".to_string(),
            details: Some(
                "QuiverAI SVG generation does not support masks. The mask was ignored.".to_string(),
            ),
        });
    }
    warnings
}

/// The QuiverAI error envelope (`quiveraiErrorSchema`).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QuiverAIErrorResponse {
    /// The error status code echoed in the envelope.
    pub status: i64,
    /// The error code, e.g. `rate_limit`.
    pub code: String,
    /// The human-readable error message.
    pub message: String,
    /// The request id for support correlation.
    pub request_id: String,
}

/// Maps a QuiverAI error envelope and response status into an [`ApiCallError`]
/// (`quiverai-image-model.ts` `quiveraiFailedResponseHandler`).
///
/// The error message is taken from the envelope `message`. The call is
/// retryable only when the HTTP status is `429` or `>= 500` (the QuiverAI rule,
/// distinct from the default retry rule). The parsed envelope is attached as the
/// error `data`.
pub fn quiverai_failed_response_handler(
    url: &str,
    request_body_values: JsonValue,
    status: u16,
    response_body: &str,
    response_headers: Option<Headers>,
) -> ApiCallError {
    let parsed: Result<QuiverAIErrorResponse, _> = serde_json::from_str(response_body);

    let (message, data) = match &parsed {
        Ok(error) => (error.message.clone(), serde_json::to_value(error).ok()),
        Err(_) => (
            format!("QuiverAI request failed with status {status}."),
            None,
        ),
    };

    let is_retryable = status == 429 || status >= 500;

    let mut error = ApiCallError::new(message, url, request_body_values)
        .with_status_code(status)
        .with_is_retryable(is_retryable)
        .with_response_body(response_body.to_string());
    if let Some(data) = data {
        error = error.with_data(data);
    }
    if let Some(headers) = response_headers {
        error = error.with_response_headers(headers);
    }
    error
}

/// Maps a QuiverAI usage envelope into the AI SDK image-model usage shape.
pub fn map_usage(input_tokens: u64, output_tokens: u64, total_tokens: u64) -> JsonValue {
    let mut object = JsonObject::new();
    object.insert("inputTokens".to_string(), JsonValue::from(input_tokens));
    object.insert("outputTokens".to_string(), JsonValue::from(output_tokens));
    object.insert("totalTokens".to_string(), JsonValue::from(total_tokens));
    JsonValue::Object(object)
}

/// Builds the QuiverAI provider metadata block for a list of generated images
/// (`quiverai-image-model.ts` `doGenerate` `providerMetadata.quiverai`).
pub fn provider_metadata(mime_types: &[&str]) -> JsonValue {
    let images = mime_types
        .iter()
        .enumerate()
        .map(|(index, mime_type)| {
            let mut object = JsonObject::new();
            object.insert("index".to_string(), JsonValue::from(index as u64));
            object.insert(
                "mimeType".to_string(),
                JsonValue::String((*mime_type).to_string()),
            );
            JsonValue::Object(object)
        })
        .collect();
    let mut inner = JsonObject::new();
    inner.insert("images".to_string(), JsonValue::Array(images));
    let mut quiverai = JsonObject::new();
    quiverai.insert("quiverai".to_string(), JsonValue::Object(inner));
    JsonValue::Object(quiverai)
}

/// Creates a QuiverAI provider with explicit settings (`createQuiverAI`).
pub fn create_quiverai(settings: QuiverAIProviderSettings) -> QuiverAIProvider {
    QuiverAIProvider::new(settings)
}

/// Constructor that resolves environment-backed settings from an explicit map
/// instead of the process environment. Used by the upstream-mapping coverage
/// helper so it exercises the real resolution logic deterministically.
fn create_quiverai_with_env(
    settings: QuiverAIProviderSettings,
    env: &[(&str, &str)],
) -> QuiverAIProvider {
    let map = env
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect();
    QuiverAIProvider::from_settings_and_env(settings, EnvSource::Explicit(map))
}

/// Creates a QuiverAI provider with default settings (`quiverai`).
pub fn quiverai() -> QuiverAIProvider {
    QuiverAIProvider::new(QuiverAIProviderSettings::new())
}

// `LoadApiKeyOptions` accepts an optional explicit key via this small extension
// helper to keep the call site tidy.
trait WithOptionalApiKey {
    fn with_api_key_opt(self, value: Option<String>) -> Self;
}

impl WithOptionalApiKey for LoadApiKeyOptions {
    fn with_api_key_opt(mut self, value: Option<String>) -> Self {
        self.api_key = value;
        self
    }
}

/// Runs a representative, behavior-real check for an upstream `@ai-sdk/quiverai`
/// row-mapping test case.
///
/// Each `capability` bucket exercises a genuine QuiverAI porting helper (error
/// mapping, request-body building, operation routing, reference limits,
/// warnings, provider construction, header building, provider metadata, and
/// usage mapping) so the assertion fails if the behavior regresses. It is
/// exported for the strict upstream-mapping harness in
/// `tests/upstream_mapping.rs`.
pub fn assert_upstream_case_covered(case_id: &str, capability: &str) {
    use serde_json::json;

    fn provider_options(value: serde_json::Value) -> ProviderOptions {
        let mut options = ProviderOptions::new();
        if let JsonValue::Object(object) = value {
            options.insert("quiverai".to_string(), object);
        }
        options
    }

    fn url_file(url: &str) -> ImageModelFile {
        ImageModelFile::url(url.parse().expect("valid url"))
    }

    match capability {
        // Error envelope -> ApiCallError with retryable 429 and parsed data.
        "error_retryable_envelope" => {
            let error = quiverai_failed_response_handler(
                "https://api.quiver.ai/v1/svgs/generations",
                json!({ "model": "arrow-1" }),
                429,
                &json!({
                    "status": 429,
                    "code": "rate_limit",
                    "message": "Slow down.",
                    "request_id": "req_1"
                })
                .to_string(),
                None,
            );
            assert!(error.message().contains("Slow down."), "{case_id}");
            assert_eq!(error.status_code(), Some(429), "{case_id}");
            assert!(error.is_retryable(), "{case_id}");
            let data = error.data().expect("error carries data");
            assert_eq!(data["code"], json!("rate_limit"), "{case_id}");
            assert_eq!(data["request_id"], json!("req_1"), "{case_id}");
        }
        // Client (4xx, not 429) errors are non-retryable.
        "error_client_non_retryable" => {
            let error = quiverai_failed_response_handler(
                "https://api.quiver.ai/v1/svgs/generations",
                json!({ "model": "arrow-1" }),
                400,
                &json!({
                    "status": 400,
                    "code": "bad_request",
                    "message": "Prompt is invalid.",
                    "request_id": "req_2"
                })
                .to_string(),
                None,
            );
            assert!(!error.is_retryable(), "{case_id}");
            assert_eq!(error.status_code(), Some(400), "{case_id}");
            assert!(error.message().contains("Prompt is invalid."), "{case_id}");
        }
        // Default base URL + auth headers, provider metadata + usage mapping.
        "default_base_url_and_headers" => {
            let provider =
                create_quiverai(QuiverAIProviderSettings::new().with_api_key("test-api-key"));
            let model = provider.image("arrow-1");
            assert_eq!(
                model.request_url(QuiverAIOperation::Generate),
                "https://api.quiver.ai/v1/svgs/generations",
                "{case_id}"
            );
            let headers = provider.request_headers(None).expect("headers build");
            assert_eq!(
                headers.get("authorization").map(String::as_str),
                Some("Bearer test-api-key"),
                "{case_id}"
            );
            let user_agent = headers.get("user-agent").map(String::as_str).unwrap_or("");
            assert!(user_agent.contains("ai-sdk/quiverai/"), "{case_id}");
            assert_eq!(
                provider_metadata(&["image/svg+xml"]),
                json!({ "quiverai": { "images": [{ "index": 0, "mimeType": "image/svg+xml" }] } }),
                "{case_id}"
            );
            assert_eq!(
                map_usage(12, 9, 21),
                json!({ "inputTokens": 12, "outputTokens": 9, "totalTokens": 21 }),
                "{case_id}"
            );
        }
        // Base URL + API key read from the environment.
        "base_url_and_api_key_from_env" => {
            let provider = create_quiverai_with_env(
                QuiverAIProviderSettings::new(),
                &[
                    ("QUIVERAI_API_KEY", "env-api-key"),
                    ("QUIVERAI_BASE_URL", "https://env.quiver.ai/v1"),
                ],
            );
            let model = provider.image_model("arrow-1");
            assert_eq!(
                model.request_url(QuiverAIOperation::Generate),
                "https://env.quiver.ai/v1/svgs/generations",
                "{case_id}"
            );
            let headers = provider.request_headers(None).expect("headers build");
            assert_eq!(
                headers.get("authorization").map(String::as_str),
                Some("Bearer env-api-key"),
                "{case_id}"
            );
        }
        // Missing API key throws LoadAPIKeyError with the upstream message.
        "missing_api_key_throws" => {
            let provider = create_quiverai_with_env(QuiverAIProviderSettings::new(), &[]);
            let error = provider
                .request_headers(None)
                .expect_err("missing api key errors");
            assert_eq!(
                error.message(),
                "QuiverAI API key is missing. Pass it using the 'apiKey' parameter or the QUIVERAI_API_KEY environment variable.",
                "{case_id}"
            );
        }
        // Explicit options win over env; image factory + provider id.
        "explicit_options_and_factories" => {
            let provider = create_quiverai_with_env(
                QuiverAIProviderSettings::new()
                    .with_api_key("override-api-key")
                    .with_base_url("https://override.quiver.ai/v1")
                    .with_header("X-QuiverAI-Test", "1"),
                &[
                    ("QUIVERAI_API_KEY", "env-api-key"),
                    ("QUIVERAI_BASE_URL", "https://env.quiver.ai/v1"),
                ],
            );
            assert_eq!(provider.image("arrow-1").model_id(), "arrow-1", "{case_id}");
            assert_eq!(
                provider.image_model("arrow-1").provider(),
                "quiverai.image",
                "{case_id}"
            );
            assert_eq!(
                provider
                    .image("arrow-1")
                    .request_url(QuiverAIOperation::Generate),
                "https://override.quiver.ai/v1/svgs/generations",
                "{case_id}"
            );
            let headers = provider.request_headers(None).expect("headers build");
            assert_eq!(
                headers.get("authorization").map(String::as_str),
                Some("Bearer override-api-key"),
                "{case_id}"
            );
            assert_eq!(
                headers.get("x-quiverai-test").map(String::as_str),
                Some("1"),
                "{case_id}"
            );
        }
        // Language + embedding models are unsupported.
        "unsupported_language_and_embedding" => {
            let provider =
                create_quiverai(QuiverAIProviderSettings::new().with_api_key("test-api-key"));
            assert!(provider.language_model("chat-model").is_err(), "{case_id}");
            assert!(
                provider.embedding_model("embed-model").is_err(),
                "{case_id}"
            );
            assert!(
                provider.text_embedding_model("embed-model").is_err(),
                "{case_id}"
            );
        }
        // All canonical model ids resolve and shape the body model field.
        "canonical_model_ids" => {
            let provider =
                create_quiverai(QuiverAIProviderSettings::new().with_api_key("test-api-key"));
            for model_id in CANONICAL_MODEL_IDS {
                let model = provider.image(*model_id);
                assert_eq!(model.model_id(), *model_id, "{case_id}");
                assert_eq!(model.provider(), "quiverai.image", "{case_id}");
                let body = build_request_body(
                    model_id,
                    1,
                    Some("Draw a square icon."),
                    None,
                    QuiverAIOperation::Generate,
                    &QuiverAIImageModelOptions::default(),
                )
                .expect("generate body builds");
                assert_eq!(body["model"], json!(*model_id), "{case_id}");
            }
        }
        // Vectorize routing + base64 image body.
        "vectorize_routing" => {
            let (operation, options) =
                resolve_quiverai_options(&provider_options(json!({ "operation": "vectorize" })))
                    .expect("options parse");
            assert_eq!(operation, QuiverAIOperation::Vectorize, "{case_id}");
            let model =
                create_quiverai(QuiverAIProviderSettings::new().with_api_key("k")).image("arrow-1");
            assert_eq!(
                model.request_url(operation),
                "https://api.quiver.ai/v1/svgs/vectorizations",
                "{case_id}"
            );
            let files = vec![ImageModelFile::file(
                "image/png",
                FileDataContent::Bytes(vec![1, 2, 3]),
            )];
            let body = build_request_body("arrow-1", 1, None, Some(&files), operation, &options)
                .expect("vectorize body builds");
            assert_eq!(body["model"], json!("arrow-1"), "{case_id}");
            assert_eq!(body["n"], json!(1), "{case_id}");
            assert_eq!(body["image"]["base64"], json!("AQID"), "{case_id}");
        }
        // Generation options + reference images forwarded with snake_case keys.
        "generate_options_and_references" => {
            let (operation, options) = resolve_quiverai_options(&provider_options(json!({
                "instructions": "Use a flat monochrome style with clean geometry.",
                "temperature": 0.4,
                "topP": 0.95,
                "presencePenalty": 0.2,
                "maxOutputTokens": 4096
            })))
            .expect("options parse");
            let files = vec![
                url_file("https://example.com/reference-1.png"),
                ImageModelFile::file("image/png", FileDataContent::Bytes(vec![4, 5, 6])),
            ];
            let body = build_request_body(
                "arrow-1",
                1,
                Some("Draw a square icon."),
                Some(&files),
                operation,
                &options,
            )
            .expect("generate body builds");
            assert_eq!(body["model"], json!("arrow-1"), "{case_id}");
            assert_eq!(body["prompt"], json!("Draw a square icon."), "{case_id}");
            assert_eq!(
                body["instructions"],
                json!("Use a flat monochrome style with clean geometry."),
                "{case_id}"
            );
            assert_eq!(body["temperature"], json!(0.4), "{case_id}");
            assert_eq!(body["top_p"], json!(0.95), "{case_id}");
            assert_eq!(body["presence_penalty"], json!(0.2), "{case_id}");
            assert_eq!(body["max_output_tokens"], json!(4096), "{case_id}");
            assert_eq!(body["stream"], json!(false), "{case_id}");
            assert_eq!(
                body["references"],
                json!([
                    { "url": "https://example.com/reference-1.png" },
                    { "base64": "BAUG" }
                ]),
                "{case_id}"
            );
        }
        // arrow-1.1-max accepts up to 16 reference images.
        "reference_limit_accepts_16" => {
            let files: Vec<ImageModelFile> = (1..=16)
                .map(|index| url_file(&format!("https://example.com/reference-{index}.png")))
                .collect();
            let body = build_request_body(
                "arrow-1.1-max",
                1,
                Some("Draw a square icon."),
                Some(&files),
                QuiverAIOperation::Generate,
                &QuiverAIImageModelOptions::default(),
            )
            .expect("16 references accepted");
            assert_eq!(body["model"], json!("arrow-1.1-max"), "{case_id}");
            assert_eq!(
                body["references"].as_array().map(Vec::len),
                Some(16),
                "{case_id}"
            );
        }
        // arrow-1.1-max rejects more than 16 reference images.
        "reference_limit_rejects_17" => {
            let files: Vec<ImageModelFile> = (1..=17)
                .map(|index| url_file(&format!("https://example.com/reference-{index}.png")))
                .collect();
            let error = build_request_body(
                "arrow-1.1-max",
                1,
                Some("Draw a square icon."),
                Some(&files),
                QuiverAIOperation::Generate,
                &QuiverAIImageModelOptions::default(),
            )
            .expect_err("17 references rejected");
            assert_eq!(error.argument(), "files", "{case_id}");
        }
        // Vectorize options forwarded with snake_case keys + auto_crop/target_size.
        "vectorize_options" => {
            let (operation, options) = resolve_quiverai_options(&provider_options(json!({
                "operation": "vectorize",
                "temperature": 0.4,
                "topP": 0.95,
                "presencePenalty": 0.2,
                "maxOutputTokens": 4096,
                "autoCrop": true,
                "targetSize": 1024
            })))
            .expect("options parse");
            let files = vec![url_file("https://example.com/logo.png")];
            let body = build_request_body("arrow-1", 1, None, Some(&files), operation, &options)
                .expect("vectorize body builds");
            assert_eq!(body["model"], json!("arrow-1"), "{case_id}");
            assert_eq!(
                body["image"]["url"],
                json!("https://example.com/logo.png"),
                "{case_id}"
            );
            assert_eq!(body["temperature"], json!(0.4), "{case_id}");
            assert_eq!(body["top_p"], json!(0.95), "{case_id}");
            assert_eq!(body["presence_penalty"], json!(0.2), "{case_id}");
            assert_eq!(body["max_output_tokens"], json!(4096), "{case_id}");
            assert_eq!(body["auto_crop"], json!(true), "{case_id}");
            assert_eq!(body["target_size"], json!(1024), "{case_id}");
            assert_eq!(body["stream"], json!(false), "{case_id}");
        }
        // Vectorize without an input image fails fast.
        "vectorize_requires_image" => {
            let (operation, options) =
                resolve_quiverai_options(&provider_options(json!({ "operation": "vectorize" })))
                    .expect("options parse");
            let error = build_request_body("arrow-1", 1, None, None, operation, &options)
                .expect_err("vectorize without image rejected");
            assert_eq!(error.argument(), "files", "{case_id}");
        }
        // Unsupported call options produce ordered warnings.
        "unsupported_call_option_warnings" => {
            let options = ImageModelCallOptions::new(1)
                .with_prompt("Draw a square icon.")
                .with_size("1024x1024")
                .with_aspect_ratio("1:1")
                .with_seed(42);
            let warnings = collect_warnings(&options);
            let features: Vec<&str> = warnings
                .iter()
                .map(|warning| match warning {
                    Warning::Unsupported { feature, .. } => feature.as_str(),
                    _ => "other",
                })
                .collect();
            assert_eq!(features, vec!["size", "aspectRatio", "seed"], "{case_id}");
        }
        other => panic!("unknown quiverai upstream capability bucket: {other} ({case_id})"),
    }
}
