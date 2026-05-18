use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use ai_sdk_provider::headers::Headers;
use ai_sdk_provider::json::{JsonObject, JsonValue};
use ai_sdk_provider::provider::ApiCallError;
use ai_sdk_provider_utils::{FetchErrorInfo, HandledFetchError};

pub const GATEWAY_AUTH_METHOD_HEADER: &str = "ai-gateway-auth-method";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum GatewayAuthMethod {
    #[serde(rename = "api-key")]
    ApiKey,
    #[serde(rename = "oidc")]
    Oidc,
}

impl GatewayAuthMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api-key",
            Self::Oidc => "oidc",
        }
    }
}

impl fmt::Display for GatewayAuthMethod {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub fn parse_gateway_auth_method(
    headers: &BTreeMap<String, Option<String>>,
) -> Option<GatewayAuthMethod> {
    let value = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(GATEWAY_AUTH_METHOD_HEADER))
        .and_then(|(_, value)| value.as_deref())?;

    match value {
        "api-key" => Some(GatewayAuthMethod::ApiKey),
        "oidc" => Some(GatewayAuthMethod::Oidc),
        _ => None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GatewayErrorData {
    base_message: String,
    message: String,
    status_code: u16,
    cause_message: Option<String>,
    generation_id: Option<String>,
    is_retryable: bool,
}

impl GatewayErrorData {
    fn new(message: impl Into<String>, status_code: u16) -> Self {
        let base_message = message.into();
        Self {
            message: base_message.clone(),
            base_message,
            status_code,
            cause_message: None,
            generation_id: None,
            is_retryable: gateway_status_is_retryable(status_code),
        }
    }

    fn set_status_code(&mut self, status_code: u16) {
        self.status_code = status_code;
        self.is_retryable = gateway_status_is_retryable(status_code);
    }

    fn set_generation_id(&mut self, generation_id: Option<String>) {
        self.generation_id = generation_id;
        self.refresh_message();
    }

    fn set_cause_message(&mut self, cause_message: impl Into<String>) {
        self.cause_message = Some(cause_message.into());
    }

    fn refresh_message(&mut self) {
        self.message = self.generation_id.as_ref().map_or_else(
            || self.base_message.clone(),
            |generation_id| format!("{} [{}]", self.base_message, generation_id),
        );
    }

    fn message(&self) -> &str {
        &self.message
    }

    const fn status_code(&self) -> u16 {
        self.status_code
    }

    fn cause_message(&self) -> Option<&str> {
        self.cause_message.as_deref()
    }

    fn generation_id(&self) -> Option<&str> {
        self.generation_id.as_deref()
    }

    const fn is_retryable(&self) -> bool {
        self.is_retryable
    }
}

const fn gateway_status_is_retryable(status_code: u16) -> bool {
    matches!(status_code, 408 | 409 | 429 | 500..=u16::MAX)
}

macro_rules! gateway_error_struct {
    ($name:ident, $display_name:literal, $error_type:literal, $default_message:literal, $default_status:expr) => {
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct $name {
            data: GatewayErrorData,
        }

        impl $name {
            pub fn new() -> Self {
                Self {
                    data: GatewayErrorData::new($default_message, $default_status),
                }
            }

            pub fn with_message(message: impl Into<String>) -> Self {
                Self {
                    data: GatewayErrorData::new(message, $default_status),
                }
            }

            pub fn with_status_code(mut self, status_code: u16) -> Self {
                self.data.set_status_code(status_code);
                self
            }

            pub fn with_generation_id(mut self, generation_id: impl Into<String>) -> Self {
                self.data.set_generation_id(Some(generation_id.into()));
                self
            }

            pub fn with_cause_message(mut self, cause_message: impl Into<String>) -> Self {
                self.data.set_cause_message(cause_message);
                self
            }

            pub const fn name(&self) -> &'static str {
                $display_name
            }

            pub const fn error_type(&self) -> &'static str {
                $error_type
            }

            pub fn message(&self) -> &str {
                self.data.message()
            }

            pub const fn status_code(&self) -> u16 {
                self.data.status_code()
            }

            pub fn cause_message(&self) -> Option<&str> {
                self.data.cause_message()
            }

            pub fn generation_id(&self) -> Option<&str> {
                self.data.generation_id()
            }

            pub const fn is_retryable(&self) -> bool {
                self.data.is_retryable()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.message())
            }
        }

        impl std::error::Error for $name {}
    };
}

gateway_error_struct!(
    GatewayInvalidRequestError,
    "GatewayInvalidRequestError",
    "invalid_request_error",
    "Invalid request",
    400
);

gateway_error_struct!(
    GatewayRateLimitError,
    "GatewayRateLimitError",
    "rate_limit_exceeded",
    "Rate limit exceeded",
    429
);

gateway_error_struct!(
    GatewayInternalServerError,
    "GatewayInternalServerError",
    "internal_server_error",
    "Internal server error",
    500
);

gateway_error_struct!(
    GatewayTimeoutError,
    "GatewayTimeoutError",
    "timeout_error",
    "Request timed out",
    408
);

impl GatewayTimeoutError {
    pub fn create_timeout_error(original_message: impl Into<String>) -> Self {
        Self::with_message(format!(
            "Gateway request timed out: {}\n\n    This is a client-side timeout. To resolve this, increase your timeout configuration: https://vercel.com/docs/ai-gateway/capabilities/video-generation#extending-timeouts-for-node.js",
            original_message.into()
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayAuthenticationError {
    data: GatewayErrorData,
}

impl GatewayAuthenticationError {
    pub fn new() -> Self {
        Self {
            data: GatewayErrorData::new("Authentication failed", 401),
        }
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            data: GatewayErrorData::new(message, 401),
        }
    }

    pub fn create_contextual_error(api_key_provided: bool, oidc_token_provided: bool) -> Self {
        contextual_gateway_authentication_error(api_key_provided, oidc_token_provided, 401, None)
    }

    pub fn with_status_code(mut self, status_code: u16) -> Self {
        self.data.set_status_code(status_code);
        self
    }

    pub fn with_generation_id(mut self, generation_id: impl Into<String>) -> Self {
        self.data.set_generation_id(Some(generation_id.into()));
        self
    }

    pub fn with_cause_message(mut self, cause_message: impl Into<String>) -> Self {
        self.data.set_cause_message(cause_message);
        self
    }

    pub const fn name(&self) -> &'static str {
        "GatewayAuthenticationError"
    }

    pub const fn error_type(&self) -> &'static str {
        "authentication_error"
    }

    pub fn message(&self) -> &str {
        self.data.message()
    }

    pub const fn status_code(&self) -> u16 {
        self.data.status_code()
    }

    pub fn cause_message(&self) -> Option<&str> {
        self.data.cause_message()
    }

    pub fn generation_id(&self) -> Option<&str> {
        self.data.generation_id()
    }

    pub const fn is_retryable(&self) -> bool {
        self.data.is_retryable()
    }
}

impl Default for GatewayAuthenticationError {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for GatewayAuthenticationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for GatewayAuthenticationError {}

fn contextual_gateway_authentication_error(
    api_key_provided: bool,
    oidc_token_provided: bool,
    status_code: u16,
    generation_id: Option<String>,
) -> GatewayAuthenticationError {
    let message = if api_key_provided {
        "AI Gateway authentication failed: Invalid API key.\n\nCreate a new API key: https://vercel.com/d?to=%2F%5Bteam%5D%2F%7E%2Fai%2Fapi-keys\n\nProvide via 'apiKey' option or 'AI_GATEWAY_API_KEY' environment variable."
    } else if oidc_token_provided {
        "AI Gateway authentication failed: Invalid OIDC token.\n\nRun 'npx vercel link' to link your project, then 'vc env pull' to fetch the token.\n\nAlternatively, use an API key: https://vercel.com/d?to=%2F%5Bteam%5D%2F%7E%2Fai%2Fapi-keys"
    } else {
        "AI Gateway authentication failed: No authentication provided.\n\nOption 1 - API key:\nCreate an API key: https://vercel.com/d?to=%2F%5Bteam%5D%2F%7E%2Fai%2Fapi-keys\nProvide via 'apiKey' option or 'AI_GATEWAY_API_KEY' environment variable.\n\nOption 2 - OIDC token:\nRun 'npx vercel link' to link your project, then 'vc env pull' to fetch the token."
    };

    let mut error = GatewayAuthenticationError::with_message(message).with_status_code(status_code);

    if let Some(generation_id) = generation_id {
        error = error.with_generation_id(generation_id);
    }

    error
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayModelNotFoundError {
    data: GatewayErrorData,
    model_id: Option<String>,
}

impl GatewayModelNotFoundError {
    pub fn new() -> Self {
        Self {
            data: GatewayErrorData::new("Model not found", 404),
            model_id: None,
        }
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            data: GatewayErrorData::new(message, 404),
            model_id: None,
        }
    }

    pub fn with_status_code(mut self, status_code: u16) -> Self {
        self.data.set_status_code(status_code);
        self
    }

    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    pub fn with_generation_id(mut self, generation_id: impl Into<String>) -> Self {
        self.data.set_generation_id(Some(generation_id.into()));
        self
    }

    pub fn with_cause_message(mut self, cause_message: impl Into<String>) -> Self {
        self.data.set_cause_message(cause_message);
        self
    }

    pub const fn name(&self) -> &'static str {
        "GatewayModelNotFoundError"
    }

    pub const fn error_type(&self) -> &'static str {
        "model_not_found"
    }

    pub fn message(&self) -> &str {
        self.data.message()
    }

    pub const fn status_code(&self) -> u16 {
        self.data.status_code()
    }

    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    pub fn cause_message(&self) -> Option<&str> {
        self.data.cause_message()
    }

    pub fn generation_id(&self) -> Option<&str> {
        self.data.generation_id()
    }

    pub const fn is_retryable(&self) -> bool {
        self.data.is_retryable()
    }
}

impl Default for GatewayModelNotFoundError {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for GatewayModelNotFoundError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for GatewayModelNotFoundError {}

#[derive(Clone, Debug, PartialEq)]
pub struct GatewayResponseError {
    data: GatewayErrorData,
    response: Option<JsonValue>,
    validation_error: Option<String>,
}

impl GatewayResponseError {
    pub fn new() -> Self {
        Self {
            data: GatewayErrorData::new("Invalid response from Gateway", 502),
            response: None,
            validation_error: None,
        }
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            data: GatewayErrorData::new(message, 502),
            response: None,
            validation_error: None,
        }
    }

    pub fn with_status_code(mut self, status_code: u16) -> Self {
        self.data.set_status_code(status_code);
        self
    }

    pub fn with_response(mut self, response: JsonValue) -> Self {
        self.response = Some(response);
        self
    }

    pub fn with_validation_error(mut self, validation_error: impl Into<String>) -> Self {
        self.validation_error = Some(validation_error.into());
        self
    }

    pub fn with_generation_id(mut self, generation_id: impl Into<String>) -> Self {
        self.data.set_generation_id(Some(generation_id.into()));
        self
    }

    pub fn with_cause_message(mut self, cause_message: impl Into<String>) -> Self {
        self.data.set_cause_message(cause_message);
        self
    }

    pub const fn name(&self) -> &'static str {
        "GatewayResponseError"
    }

    pub const fn error_type(&self) -> &'static str {
        "response_error"
    }

    pub fn message(&self) -> &str {
        self.data.message()
    }

    pub const fn status_code(&self) -> u16 {
        self.data.status_code()
    }

    pub fn response(&self) -> Option<&JsonValue> {
        self.response.as_ref()
    }

    pub fn validation_error(&self) -> Option<&str> {
        self.validation_error.as_deref()
    }

    pub fn cause_message(&self) -> Option<&str> {
        self.data.cause_message()
    }

    pub fn generation_id(&self) -> Option<&str> {
        self.data.generation_id()
    }

    pub const fn is_retryable(&self) -> bool {
        self.data.is_retryable()
    }
}

impl Default for GatewayResponseError {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for GatewayResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for GatewayResponseError {}

#[derive(Clone, Debug, PartialEq)]
pub enum GatewayError {
    Authentication(Box<GatewayAuthenticationError>),
    InvalidRequest(Box<GatewayInvalidRequestError>),
    RateLimit(Box<GatewayRateLimitError>),
    ModelNotFound(Box<GatewayModelNotFoundError>),
    InternalServer(Box<GatewayInternalServerError>),
    Response(Box<GatewayResponseError>),
    Timeout(Box<GatewayTimeoutError>),
}

impl GatewayError {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Authentication(error) => error.name(),
            Self::InvalidRequest(error) => error.name(),
            Self::RateLimit(error) => error.name(),
            Self::ModelNotFound(error) => error.name(),
            Self::InternalServer(error) => error.name(),
            Self::Response(error) => error.name(),
            Self::Timeout(error) => error.name(),
        }
    }

    pub fn error_type(&self) -> &'static str {
        match self {
            Self::Authentication(error) => error.error_type(),
            Self::InvalidRequest(error) => error.error_type(),
            Self::RateLimit(error) => error.error_type(),
            Self::ModelNotFound(error) => error.error_type(),
            Self::InternalServer(error) => error.error_type(),
            Self::Response(error) => error.error_type(),
            Self::Timeout(error) => error.error_type(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Authentication(error) => error.message(),
            Self::InvalidRequest(error) => error.message(),
            Self::RateLimit(error) => error.message(),
            Self::ModelNotFound(error) => error.message(),
            Self::InternalServer(error) => error.message(),
            Self::Response(error) => error.message(),
            Self::Timeout(error) => error.message(),
        }
    }

    pub fn status_code(&self) -> u16 {
        match self {
            Self::Authentication(error) => error.status_code(),
            Self::InvalidRequest(error) => error.status_code(),
            Self::RateLimit(error) => error.status_code(),
            Self::ModelNotFound(error) => error.status_code(),
            Self::InternalServer(error) => error.status_code(),
            Self::Response(error) => error.status_code(),
            Self::Timeout(error) => error.status_code(),
        }
    }

    pub fn generation_id(&self) -> Option<&str> {
        match self {
            Self::Authentication(error) => error.generation_id(),
            Self::InvalidRequest(error) => error.generation_id(),
            Self::RateLimit(error) => error.generation_id(),
            Self::ModelNotFound(error) => error.generation_id(),
            Self::InternalServer(error) => error.generation_id(),
            Self::Response(error) => error.generation_id(),
            Self::Timeout(error) => error.generation_id(),
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Authentication(error) => error.is_retryable(),
            Self::InvalidRequest(error) => error.is_retryable(),
            Self::RateLimit(error) => error.is_retryable(),
            Self::ModelNotFound(error) => error.is_retryable(),
            Self::InternalServer(error) => error.is_retryable(),
            Self::Response(error) => error.is_retryable(),
            Self::Timeout(error) => error.is_retryable(),
        }
    }

    pub fn as_authentication(&self) -> Option<&GatewayAuthenticationError> {
        match self {
            Self::Authentication(error) => Some(error.as_ref()),
            _ => None,
        }
    }

    pub fn as_invalid_request(&self) -> Option<&GatewayInvalidRequestError> {
        match self {
            Self::InvalidRequest(error) => Some(error.as_ref()),
            _ => None,
        }
    }

    pub fn as_rate_limit(&self) -> Option<&GatewayRateLimitError> {
        match self {
            Self::RateLimit(error) => Some(error.as_ref()),
            _ => None,
        }
    }

    pub fn as_model_not_found(&self) -> Option<&GatewayModelNotFoundError> {
        match self {
            Self::ModelNotFound(error) => Some(error.as_ref()),
            _ => None,
        }
    }

    pub fn as_internal_server(&self) -> Option<&GatewayInternalServerError> {
        match self {
            Self::InternalServer(error) => Some(error.as_ref()),
            _ => None,
        }
    }

    pub fn as_response(&self) -> Option<&GatewayResponseError> {
        match self {
            Self::Response(error) => Some(error.as_ref()),
            _ => None,
        }
    }

    pub fn as_timeout(&self) -> Option<&GatewayTimeoutError> {
        match self {
            Self::Timeout(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl fmt::Display for GatewayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for GatewayError {}

impl From<GatewayAuthenticationError> for GatewayError {
    fn from(error: GatewayAuthenticationError) -> Self {
        Self::Authentication(Box::new(error))
    }
}

impl From<GatewayInvalidRequestError> for GatewayError {
    fn from(error: GatewayInvalidRequestError) -> Self {
        Self::InvalidRequest(Box::new(error))
    }
}

impl From<GatewayRateLimitError> for GatewayError {
    fn from(error: GatewayRateLimitError) -> Self {
        Self::RateLimit(Box::new(error))
    }
}

impl From<GatewayModelNotFoundError> for GatewayError {
    fn from(error: GatewayModelNotFoundError) -> Self {
        Self::ModelNotFound(Box::new(error))
    }
}

impl From<GatewayInternalServerError> for GatewayError {
    fn from(error: GatewayInternalServerError) -> Self {
        Self::InternalServer(Box::new(error))
    }
}

impl From<GatewayResponseError> for GatewayError {
    fn from(error: GatewayResponseError) -> Self {
        Self::Response(Box::new(error))
    }
}

impl From<GatewayTimeoutError> for GatewayError {
    fn from(error: GatewayTimeoutError) -> Self {
        Self::Timeout(Box::new(error))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayErrorResponse {
    pub error: GatewayErrorResponseError,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayErrorResponseError {
    pub message: String,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<JsonValue>,
}

pub fn create_gateway_error_from_response(
    response: JsonValue,
    status_code: u16,
    default_message: impl Into<String>,
    auth_method: Option<GatewayAuthMethod>,
) -> GatewayError {
    let default_message = default_message.into();
    let raw_generation_id = response
        .get("generationId")
        .and_then(JsonValue::as_str)
        .map(String::from);

    let parsed = match serde_json::from_value::<GatewayErrorResponse>(response.clone()) {
        Ok(parsed) => parsed,
        Err(error) => {
            let mut response_error = GatewayResponseError::with_message(format!(
                "Invalid error response format: {default_message}"
            ))
            .with_status_code(status_code)
            .with_response(response)
            .with_validation_error(error.to_string());

            if let Some(generation_id) = raw_generation_id {
                response_error = response_error.with_generation_id(generation_id);
            }

            return response_error.into();
        }
    };

    let message = parsed.error.message;
    let generation_id = parsed.generation_id;

    match parsed.error.error_type.as_deref() {
        Some("authentication_error") => {
            let (api_key_provided, oidc_token_provided) = match auth_method {
                Some(GatewayAuthMethod::ApiKey) => (true, false),
                Some(GatewayAuthMethod::Oidc) => (false, true),
                None => (false, false),
            };

            contextual_gateway_authentication_error(
                api_key_provided,
                oidc_token_provided,
                status_code,
                generation_id,
            )
            .into()
        }
        Some("invalid_request_error") => error_with_generation_id(
            GatewayInvalidRequestError::with_message(message).with_status_code(status_code),
            generation_id,
        )
        .into(),
        Some("rate_limit_exceeded") => error_with_generation_id(
            GatewayRateLimitError::with_message(message).with_status_code(status_code),
            generation_id,
        )
        .into(),
        Some("model_not_found") => {
            let mut error =
                GatewayModelNotFoundError::with_message(message).with_status_code(status_code);

            if let Some(model_id) = parsed
                .error
                .param
                .as_ref()
                .and_then(|param| param.get("modelId"))
                .and_then(JsonValue::as_str)
            {
                error = error.with_model_id(model_id);
            }

            error_with_generation_id(error, generation_id).into()
        }
        Some("internal_server_error") | None => error_with_generation_id(
            GatewayInternalServerError::with_message(message).with_status_code(status_code),
            generation_id,
        )
        .into(),
        Some(_) => error_with_generation_id(
            GatewayInternalServerError::with_message(message).with_status_code(status_code),
            generation_id,
        )
        .into(),
    }
}

fn error_with_generation_id<E>(error: E, generation_id: Option<String>) -> E
where
    E: GatewayErrorGenerationId,
{
    if let Some(generation_id) = generation_id {
        error.with_gateway_generation_id(generation_id)
    } else {
        error
    }
}

trait GatewayErrorGenerationId: Sized {
    fn with_gateway_generation_id(self, generation_id: String) -> Self;
}

macro_rules! impl_gateway_generation_id {
    ($name:ident) => {
        impl GatewayErrorGenerationId for $name {
            fn with_gateway_generation_id(self, generation_id: String) -> Self {
                self.with_generation_id(generation_id)
            }
        }
    };
}

impl_gateway_generation_id!(GatewayInvalidRequestError);
impl_gateway_generation_id!(GatewayRateLimitError);
impl_gateway_generation_id!(GatewayModelNotFoundError);
impl_gateway_generation_id!(GatewayInternalServerError);

pub fn extract_gateway_api_call_response(error: &ApiCallError) -> JsonValue {
    if let Some(data) = error.data() {
        return data.clone();
    }

    if let Some(response_body) = error.response_body() {
        return serde_json::from_str::<JsonValue>(response_body)
            .unwrap_or_else(|_| JsonValue::String(response_body.to_string()));
    }

    JsonValue::Object(JsonObject::new())
}

pub fn create_gateway_error_from_api_call(
    error: &ApiCallError,
    auth_method: Option<GatewayAuthMethod>,
) -> GatewayError {
    let default_message = if error.message().is_empty() {
        "Gateway request failed"
    } else {
        error.message()
    };

    create_gateway_error_from_response(
        extract_gateway_api_call_response(error),
        error.status_code().unwrap_or(500),
        default_message,
        auth_method,
    )
}

pub fn as_gateway_error(
    error: HandledFetchError,
    auth_method: Option<GatewayAuthMethod>,
) -> GatewayError {
    match error {
        HandledFetchError::Original { error } => {
            if is_gateway_timeout_fetch_error(&error) {
                return GatewayTimeoutError::create_timeout_error(error.message()).into();
            }

            create_gateway_error_from_response(
                JsonValue::Object(JsonObject::new()),
                500,
                format!("Gateway request failed: {}", error.message()),
                auth_method,
            )
        }
        HandledFetchError::ApiCall { error } => {
            create_gateway_error_from_api_call(&error, auth_method)
        }
    }
}

fn is_gateway_timeout_fetch_error(error: &FetchErrorInfo) -> bool {
    matches!(
        error.code(),
        Some("UND_ERR_HEADERS_TIMEOUT" | "UND_ERR_BODY_TIMEOUT" | "UND_ERR_CONNECT_TIMEOUT")
    )
}

pub fn gateway_headers_from_auth_method(auth_method: GatewayAuthMethod) -> Headers {
    Headers::from([(
        GATEWAY_AUTH_METHOD_HEADER.to_string(),
        auth_method.as_str().to_string(),
    )])
}

#[cfg(test)]
mod tests {
    use super::{
        GatewayAuthMethod, GatewayAuthenticationError, GatewayError, GatewayInternalServerError,
        GatewayInvalidRequestError, GatewayModelNotFoundError, GatewayRateLimitError,
        GatewayResponseError, GatewayTimeoutError, as_gateway_error,
        create_gateway_error_from_api_call, create_gateway_error_from_response,
        extract_gateway_api_call_response, gateway_headers_from_auth_method,
        parse_gateway_auth_method,
    };
    use ai_sdk_provider::headers::Headers;
    use ai_sdk_provider::provider::ApiCallError;
    use ai_sdk_provider_utils::{FetchErrorInfo, HandledFetchError};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn gateway_error_types_expose_upstream_names_status_and_retryability() {
        let auth = GatewayAuthenticationError::new();
        assert_eq!(auth.name(), "GatewayAuthenticationError");
        assert_eq!(auth.error_type(), "authentication_error");
        assert_eq!(auth.message(), "Authentication failed");
        assert_eq!(auth.status_code(), 401);
        assert!(!auth.is_retryable());

        assert!(!GatewayInvalidRequestError::new().is_retryable());
        assert!(GatewayRateLimitError::new().is_retryable());
        assert!(GatewayInternalServerError::new().is_retryable());
        assert!(GatewayTimeoutError::new().is_retryable());
        assert!(GatewayResponseError::new().is_retryable());
        assert!(!GatewayModelNotFoundError::new().is_retryable());
    }

    #[test]
    fn gateway_errors_append_generation_id_to_message() {
        let error = GatewayRateLimitError::with_message("Rate limit exceeded")
            .with_generation_id("gen_123");

        assert_eq!(error.message(), "Rate limit exceeded [gen_123]");
        assert_eq!(error.generation_id(), Some("gen_123"));
        assert_eq!(error.to_string(), "Rate limit exceeded [gen_123]");
    }

    #[test]
    fn gateway_authentication_contextual_messages_match_auth_source() {
        let api_key = GatewayAuthenticationError::create_contextual_error(true, false);
        assert!(api_key.message().contains("Invalid API key"));
        assert!(api_key.message().contains("AI_GATEWAY_API_KEY"));

        let oidc = GatewayAuthenticationError::create_contextual_error(false, true);
        assert!(oidc.message().contains("Invalid OIDC token"));
        assert!(oidc.message().contains("npx vercel link"));

        let missing = GatewayAuthenticationError::create_contextual_error(false, false);
        assert!(missing.message().contains("No authentication provided"));
        assert!(missing.message().contains("Option 1"));
        assert!(missing.message().contains("Option 2"));
    }

    #[test]
    fn create_gateway_error_from_response_maps_gateway_error_types() {
        let auth = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Unauthorized",
                    "type": "authentication_error"
                },
                "generationId": "gen_auth"
            }),
            401,
            "Gateway request failed",
            Some(GatewayAuthMethod::ApiKey),
        );
        assert!(matches!(auth, GatewayError::Authentication(_)));
        assert_eq!(auth.status_code(), 401);
        assert!(auth.message().contains("Invalid API key"));
        assert_eq!(auth.generation_id(), Some("gen_auth"));

        let invalid = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Missing prompt",
                    "type": "invalid_request_error"
                }
            }),
            400,
            "Gateway request failed",
            None,
        );
        assert_eq!(
            invalid.as_invalid_request().map(|error| error.message()),
            Some("Missing prompt")
        );

        let rate_limit = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Rate limit exceeded",
                    "type": "rate_limit_exceeded"
                }
            }),
            429,
            "Gateway request failed",
            None,
        );
        assert!(rate_limit.as_rate_limit().is_some());
        assert!(rate_limit.is_retryable());

        let model_not_found = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Model not found",
                    "type": "model_not_found",
                    "param": {
                        "modelId": "openai/missing"
                    }
                }
            }),
            404,
            "Gateway request failed",
            None,
        );
        assert_eq!(
            model_not_found
                .as_model_not_found()
                .and_then(|error| error.model_id()),
            Some("openai/missing")
        );

        let internal = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Provider failed",
                    "type": "unknown_gateway_error"
                }
            }),
            500,
            "Gateway request failed",
            None,
        );
        assert!(internal.as_internal_server().is_some());
    }

    #[test]
    fn create_gateway_error_from_response_handles_invalid_response_shapes() {
        let error = create_gateway_error_from_response(
            json!({
                "generationId": "gen_bad",
                "unexpected": true
            }),
            502,
            "Gateway request failed",
            None,
        );
        let response_error = error
            .as_response()
            .expect("invalid shape maps to response error");

        assert_eq!(response_error.name(), "GatewayResponseError");
        assert!(
            response_error
                .message()
                .contains("Invalid error response format")
        );
        assert_eq!(response_error.status_code(), 502);
        assert_eq!(response_error.generation_id(), Some("gen_bad"));
        assert!(response_error.response().is_some());
        assert!(response_error.validation_error().is_some());
    }

    #[test]
    fn extract_gateway_api_call_response_prefers_data_then_json_then_raw_body() {
        let data_error = ApiCallError::new("Request failed", "https://api.test", json!({}))
            .with_data(json!({
                "error": {
                    "message": "Parsed",
                    "type": "rate_limit_exceeded"
                }
            }))
            .with_response_body(r#"{"fallback":true}"#);
        assert_eq!(
            extract_gateway_api_call_response(&data_error)
                .pointer("/error/message")
                .and_then(|value| value.as_str()),
            Some("Parsed")
        );

        let json_error = ApiCallError::new("Request failed", "https://api.test", json!({}))
            .with_response_body(r#"{"error":{"message":"Body","type":"internal_server_error"}}"#);
        assert_eq!(
            extract_gateway_api_call_response(&json_error)
                .pointer("/error/message")
                .and_then(|value| value.as_str()),
            Some("Body")
        );

        let raw_error = ApiCallError::new("Request failed", "https://api.test", json!({}))
            .with_response_body("not json");
        assert_eq!(
            extract_gateway_api_call_response(&raw_error),
            json!("not json")
        );
    }

    #[test]
    fn create_gateway_error_from_api_call_maps_data_and_status() {
        let api_error = ApiCallError::new("Request failed", "https://api.test", json!({}))
            .with_status_code(429)
            .with_data(json!({
                "error": {
                    "message": "Slow down",
                    "type": "rate_limit_exceeded"
                }
            }));
        let gateway_error =
            create_gateway_error_from_api_call(&api_error, Some(GatewayAuthMethod::ApiKey));

        assert_eq!(
            gateway_error.as_rate_limit().map(|error| error.message()),
            Some("Slow down")
        );
        assert_eq!(gateway_error.status_code(), 429);
    }

    #[test]
    fn create_gateway_error_from_api_call_preserves_message_without_error_data() {
        let api_error = ApiCallError::new(
            "SSE stream ended without a data event",
            "https://api.test",
            json!({}),
        )
        .with_status_code(200);
        let gateway_error = create_gateway_error_from_api_call(&api_error, None);

        let response_error = gateway_error
            .as_response()
            .expect("empty error data maps to response error");
        assert!(
            response_error
                .message()
                .contains("SSE stream ended without a data event")
        );
        assert_eq!(response_error.status_code(), 200);
    }

    #[test]
    fn parse_gateway_auth_method_accepts_only_gateway_values() {
        let headers = BTreeMap::from([
            (
                "authorization".to_string(),
                Some("Bearer token".to_string()),
            ),
            (
                "AI-Gateway-Auth-Method".to_string(),
                Some("api-key".to_string()),
            ),
        ]);
        assert_eq!(
            parse_gateway_auth_method(&headers),
            Some(GatewayAuthMethod::ApiKey)
        );

        let invalid = BTreeMap::from([(
            "ai-gateway-auth-method".to_string(),
            Some(" API-KEY ".to_string()),
        )]);
        assert_eq!(parse_gateway_auth_method(&invalid), None);
    }

    #[test]
    fn as_gateway_error_maps_handled_fetch_errors() {
        let timeout = as_gateway_error(
            HandledFetchError::Original {
                error: FetchErrorInfo::new("headers timed out")
                    .with_code("UND_ERR_HEADERS_TIMEOUT"),
            },
            None,
        );
        assert!(timeout.as_timeout().is_some());
        assert!(timeout.message().contains("headers timed out"));

        let api_error = ApiCallError::new("Request failed", "https://api.test", json!({}))
            .with_status_code(401)
            .with_data(json!({
                "error": {
                    "message": "Unauthorized",
                    "type": "authentication_error"
                }
            }));
        let auth = as_gateway_error(
            HandledFetchError::ApiCall {
                error: Box::new(api_error),
            },
            Some(GatewayAuthMethod::ApiKey),
        );
        assert!(auth.as_authentication().is_some());
        assert!(auth.message().contains("Invalid API key"));
    }

    #[test]
    fn gateway_headers_from_auth_method_uses_upstream_header_name() {
        assert_eq!(
            gateway_headers_from_auth_method(GatewayAuthMethod::Oidc),
            Headers::from([("ai-gateway-auth-method".to_string(), "oidc".to_string(),)])
        );
    }
}
