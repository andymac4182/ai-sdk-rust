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
    create_gateway_error_from_response_with_cause_message(
        response,
        status_code,
        default_message,
        auth_method,
        None,
    )
}

pub fn create_gateway_error_from_response_with_cause_message(
    response: JsonValue,
    status_code: u16,
    default_message: impl Into<String>,
    auth_method: Option<GatewayAuthMethod>,
    cause_message: Option<String>,
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
            if let Some(cause_message) = cause_message {
                response_error = response_error.with_cause_message(cause_message);
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
            .with_optional_cause_message(cause_message)
            .into()
        }
        Some("invalid_request_error") => error_with_details(
            GatewayInvalidRequestError::with_message(message).with_status_code(status_code),
            generation_id,
            cause_message,
        )
        .into(),
        Some("rate_limit_exceeded") => error_with_details(
            GatewayRateLimitError::with_message(message).with_status_code(status_code),
            generation_id,
            cause_message,
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

            error_with_details(error, generation_id, cause_message).into()
        }
        Some("internal_server_error") | None => error_with_details(
            GatewayInternalServerError::with_message(message).with_status_code(status_code),
            generation_id,
            cause_message,
        )
        .into(),
        Some(_) => error_with_details(
            GatewayInternalServerError::with_message(message).with_status_code(status_code),
            generation_id,
            cause_message,
        )
        .into(),
    }
}

fn error_with_details<E>(
    error: E,
    generation_id: Option<String>,
    cause_message: Option<String>,
) -> E
where
    E: GatewayErrorDetails,
{
    error
        .with_optional_generation_id(generation_id)
        .with_optional_cause_message(cause_message)
}

trait GatewayErrorDetails: Sized {
    fn with_gateway_generation_id(self, generation_id: String) -> Self;
    fn with_gateway_cause_message(self, cause_message: String) -> Self;

    fn with_optional_generation_id(self, generation_id: Option<String>) -> Self {
        if let Some(generation_id) = generation_id {
            self.with_gateway_generation_id(generation_id)
        } else {
            self
        }
    }

    fn with_optional_cause_message(self, cause_message: Option<String>) -> Self {
        if let Some(cause_message) = cause_message {
            self.with_gateway_cause_message(cause_message)
        } else {
            self
        }
    }
}

macro_rules! impl_gateway_error_details {
    ($name:ident) => {
        impl GatewayErrorDetails for $name {
            fn with_gateway_generation_id(self, generation_id: String) -> Self {
                self.with_generation_id(generation_id)
            }

            fn with_gateway_cause_message(self, cause_message: String) -> Self {
                self.with_cause_message(cause_message)
            }
        }
    };
}

impl_gateway_error_details!(GatewayAuthenticationError);
impl_gateway_error_details!(GatewayInvalidRequestError);
impl_gateway_error_details!(GatewayRateLimitError);
impl_gateway_error_details!(GatewayModelNotFoundError);
impl_gateway_error_details!(GatewayInternalServerError);

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

    create_gateway_error_from_response_with_cause_message(
        extract_gateway_api_call_response(error),
        error.status_code().unwrap_or(500),
        default_message,
        auth_method,
        Some(error.message().to_string()),
    )
}

pub fn as_gateway_error(
    error: HandledFetchError,
    auth_method: Option<GatewayAuthMethod>,
) -> GatewayError {
    match error {
        HandledFetchError::Original { error } => {
            if is_gateway_timeout_fetch_error(&error) {
                return GatewayTimeoutError::create_timeout_error(error.message())
                    .with_cause_message(error.message())
                    .into();
            }

            create_gateway_error_from_response_with_cause_message(
                JsonValue::Object(JsonObject::new()),
                500,
                format!("Gateway request failed: {}", error.message()),
                auth_method,
                Some(error.message().to_string()),
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
        GATEWAY_AUTH_METHOD_HEADER, GatewayAuthMethod, GatewayAuthenticationError, GatewayError,
        GatewayInternalServerError, GatewayInvalidRequestError, GatewayModelNotFoundError,
        GatewayRateLimitError, GatewayResponseError, GatewayTimeoutError, as_gateway_error,
        create_gateway_error_from_api_call, create_gateway_error_from_response,
        create_gateway_error_from_response_with_cause_message, extract_gateway_api_call_response,
        gateway_headers_from_auth_method, parse_gateway_auth_method,
    };
    use ai_sdk_provider::headers::Headers;
    use ai_sdk_provider::provider::ApiCallError;
    use ai_sdk_provider_utils::{FetchErrorInfo, HandledFetchError};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn gateway_response_error(error: &GatewayError) -> &GatewayResponseError {
        error.as_response().expect("expected GatewayResponseError")
    }

    fn headers_with_auth_method(value: Option<&str>) -> BTreeMap<String, Option<String>> {
        BTreeMap::from([(
            GATEWAY_AUTH_METHOD_HEADER.to_string(),
            value.map(String::from),
        )])
    }

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
    fn gateway_authentication_error_matches_default_and_custom_upstream_values() {
        let default_error = GatewayAuthenticationError::new();
        assert_eq!(default_error.name(), "GatewayAuthenticationError");
        assert_eq!(default_error.error_type(), "authentication_error");
        assert_eq!(default_error.message(), "Authentication failed");
        assert_eq!(default_error.status_code(), 401);
        assert_eq!(default_error.cause_message(), None);

        let custom_error = GatewayAuthenticationError::with_message("Custom auth failed")
            .with_status_code(403)
            .with_cause_message("Original error");
        assert_eq!(custom_error.message(), "Custom auth failed");
        assert_eq!(custom_error.status_code(), 403);
        assert_eq!(custom_error.cause_message(), Some("Original error"));

        let gateway_error = GatewayError::from(default_error);
        assert!(gateway_error.as_authentication().is_some());
        assert!(gateway_error.as_invalid_request().is_none());
    }

    #[test]
    fn gateway_authentication_contextual_error_matches_upstream_matrix() {
        let api_key = GatewayAuthenticationError::create_contextual_error(true, false);
        assert!(api_key.message().contains("Invalid API key"));
        assert!(api_key.message().contains("vercel.com/d?to="));
        assert_eq!(api_key.status_code(), 401);

        let oidc = GatewayAuthenticationError::create_contextual_error(false, true);
        assert!(oidc.message().contains("Invalid OIDC token"));
        assert!(oidc.message().contains("npx vercel link"));
        assert_eq!(oidc.status_code(), 401);

        let missing = GatewayAuthenticationError::create_contextual_error(false, false);
        assert!(missing.message().contains("No authentication provided"));
        assert!(missing.message().contains("Option 1"));
        assert!(missing.message().contains("Option 2"));
        assert_eq!(missing.status_code(), 401);

        let both = GatewayAuthenticationError::create_contextual_error(true, true);
        assert!(both.message().contains("Invalid API key"));
        assert!(both.message().contains("vercel.com/d?to="));
        assert_eq!(both.status_code(), 401);
    }

    #[test]
    fn gateway_invalid_request_error_matches_default_custom_and_variant_checks() {
        let default_error = GatewayInvalidRequestError::new();
        assert_eq!(default_error.name(), "GatewayInvalidRequestError");
        assert_eq!(default_error.error_type(), "invalid_request_error");
        assert_eq!(default_error.message(), "Invalid request");
        assert_eq!(default_error.status_code(), 400);

        let custom_error = GatewayInvalidRequestError::with_message("Missing required field")
            .with_status_code(422);
        assert_eq!(custom_error.message(), "Missing required field");
        assert_eq!(custom_error.status_code(), 422);

        let gateway_error = GatewayError::from(default_error);
        assert!(gateway_error.as_invalid_request().is_some());
        assert!(gateway_error.as_authentication().is_none());
    }

    #[test]
    fn gateway_rate_limit_error_matches_default_and_variant_checks() {
        let error = GatewayRateLimitError::new();
        assert_eq!(error.name(), "GatewayRateLimitError");
        assert_eq!(error.error_type(), "rate_limit_exceeded");
        assert_eq!(error.message(), "Rate limit exceeded");
        assert_eq!(error.status_code(), 429);

        let gateway_error = GatewayError::from(error);
        assert!(gateway_error.as_rate_limit().is_some());
        assert!(gateway_error.as_internal_server().is_none());
    }

    #[test]
    fn gateway_model_not_found_error_matches_default_custom_and_variant_checks() {
        let default_error = GatewayModelNotFoundError::new();
        assert_eq!(default_error.name(), "GatewayModelNotFoundError");
        assert_eq!(default_error.error_type(), "model_not_found");
        assert_eq!(default_error.message(), "Model not found");
        assert_eq!(default_error.status_code(), 404);
        assert_eq!(default_error.model_id(), None);

        let custom_error =
            GatewayModelNotFoundError::with_message("Model gpt-4 not found").with_model_id("gpt-4");
        assert_eq!(custom_error.message(), "Model gpt-4 not found");
        assert_eq!(custom_error.model_id(), Some("gpt-4"));

        let gateway_error = GatewayError::from(default_error);
        assert!(gateway_error.as_model_not_found().is_some());
        assert!(gateway_error.as_rate_limit().is_none());
    }

    #[test]
    fn gateway_internal_server_error_matches_default_and_variant_checks() {
        let error = GatewayInternalServerError::new();
        assert_eq!(error.name(), "GatewayInternalServerError");
        assert_eq!(error.error_type(), "internal_server_error");
        assert_eq!(error.message(), "Internal server error");
        assert_eq!(error.status_code(), 500);

        let gateway_error = GatewayError::from(error);
        assert!(gateway_error.as_internal_server().is_some());
        assert!(gateway_error.as_model_not_found().is_none());
    }

    #[test]
    fn gateway_retryability_matches_upstream_status_matrix() {
        assert!(GatewayInternalServerError::new().is_retryable());
        assert!(GatewayRateLimitError::new().is_retryable());
        assert!(GatewayTimeoutError::new().is_retryable());
        assert!(
            GatewayInternalServerError::with_message("Service unavailable")
                .with_status_code(503)
                .is_retryable()
        );
        assert!(!GatewayAuthenticationError::new().is_retryable());
        assert!(!GatewayInvalidRequestError::new().is_retryable());
        assert!(!GatewayModelNotFoundError::new().is_retryable());
        assert!(GatewayResponseError::new().is_retryable());
    }

    #[test]
    fn gateway_response_error_matches_default_custom_and_variant_checks() {
        let default_error = GatewayResponseError::new();
        assert_eq!(default_error.name(), "GatewayResponseError");
        assert_eq!(default_error.error_type(), "response_error");
        assert_eq!(default_error.message(), "Invalid response from Gateway");
        assert_eq!(default_error.status_code(), 502);
        assert_eq!(default_error.response(), None);
        assert_eq!(default_error.validation_error(), None);

        let response = json!({ "invalidField": "value" });
        let custom_error = GatewayResponseError::with_message("Custom parsing error")
            .with_status_code(422)
            .with_response(response.clone())
            .with_validation_error(r#"{"issues":[{"path":["error"],"message":"Required"}]}"#);
        assert_eq!(custom_error.message(), "Custom parsing error");
        assert_eq!(custom_error.status_code(), 422);
        assert_eq!(custom_error.response(), Some(&response));
        assert!(custom_error.validation_error().is_some());

        let gateway_error = GatewayError::from(default_error);
        assert!(gateway_error.as_response().is_some());
        assert!(gateway_error.as_timeout().is_none());
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
    fn create_gateway_error_from_response_preserves_empty_auth_messages_with_context() {
        let error = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "",
                    "type": "authentication_error"
                }
            }),
            401,
            "Custom default message",
            None,
        );

        let auth = error
            .as_authentication()
            .expect("authentication_error maps to contextual auth error");
        assert!(auth.message().contains("No authentication provided"));
        assert_eq!(auth.status_code(), 401);
        assert_eq!(auth.error_type(), "authentication_error");
    }

    #[test]
    fn create_gateway_error_from_response_uses_default_message_for_null_message() {
        let response = json!({
            "error": {
                "message": null,
                "type": "authentication_error"
            }
        });
        let error = create_gateway_error_from_response(
            response.clone(),
            401,
            "Custom default message",
            None,
        );

        let response_error = gateway_response_error(&error);
        assert_eq!(
            response_error.message(),
            "Invalid error response format: Custom default message"
        );
        assert_eq!(response_error.status_code(), 401);
        assert_eq!(response_error.response(), Some(&response));
        assert!(response_error.validation_error().is_some());
    }

    #[test]
    fn create_gateway_error_from_response_handles_null_error_type_as_internal() {
        let error = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Some error",
                    "type": null
                }
            }),
            500,
            "Gateway request failed",
            None,
        );

        let internal = error
            .as_internal_server()
            .expect("null error type maps to internal server error");
        assert_eq!(internal.message(), "Some error");
        assert_eq!(internal.status_code(), 500);
    }

    #[test]
    fn create_gateway_error_from_response_includes_cause_message() {
        let error = create_gateway_error_from_response_with_cause_message(
            json!({
                "error": {
                    "message": "Gateway timeout",
                    "type": "internal_server_error"
                }
            }),
            504,
            "Gateway request failed",
            None,
            Some("Original network error".to_string()),
        );

        let internal = error
            .as_internal_server()
            .expect("internal_server_error maps to internal server error");
        assert_eq!(internal.message(), "Gateway timeout");
        assert_eq!(internal.status_code(), 504);
        assert_eq!(internal.cause_message(), Some("Original network error"));
    }

    #[test]
    fn create_gateway_error_from_response_maps_malformed_responses() {
        let cases = [
            (json!({ "invalidField": "value" }), "Gateway request failed"),
            (json!({ "data": "some data" }), "Custom error message"),
            (json!(null), "Gateway request failed"),
            (json!("Error string"), "Gateway request failed"),
            (json!(["error", "array"]), "Gateway request failed"),
        ];

        for (response, default_message) in cases {
            let error =
                create_gateway_error_from_response(response.clone(), 500, default_message, None);
            let response_error = gateway_response_error(&error);

            assert_eq!(
                response_error.message(),
                format!("Invalid error response format: {default_message}")
            );
            assert_eq!(response_error.status_code(), 500);
            assert_eq!(response_error.response(), Some(&response));
            assert!(response_error.validation_error().is_some());
        }
    }

    #[test]
    fn create_gateway_error_from_response_handles_model_not_found_param_edges() {
        let invalid_param = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Model not available",
                    "type": "model_not_found",
                    "param": {
                        "invalidField": "value"
                    }
                }
            }),
            404,
            "Gateway request failed",
            None,
        );
        let invalid_param_error = invalid_param
            .as_model_not_found()
            .expect("model_not_found maps with invalid param");
        assert_eq!(invalid_param_error.model_id(), None);

        let missing_param = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Model not found",
                    "type": "model_not_found"
                }
            }),
            404,
            "Gateway request failed",
            None,
        );
        let missing_param_error = missing_param
            .as_model_not_found()
            .expect("model_not_found maps without param");
        assert_eq!(missing_param_error.model_id(), None);
        assert_eq!(missing_param_error.message(), "Model not found");
    }

    #[test]
    fn create_gateway_error_from_response_ignores_extra_fields() {
        let error = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Test error",
                    "type": "authentication_error",
                    "code": "AUTH_FAILED",
                    "param": null,
                    "extraField": "should be ignored"
                },
                "metadata": "should be ignored"
            }),
            401,
            "Gateway request failed",
            None,
        );

        let auth = error
            .as_authentication()
            .expect("extra fields are ignored for authentication errors");
        assert!(auth.message().contains("No authentication provided"));
        assert_eq!(auth.status_code(), 401);
    }

    #[test]
    fn create_gateway_error_from_response_preserves_error_properties() {
        let error = create_gateway_error_from_response_with_cause_message(
            json!({
                "error": {
                    "message": "Rate limit hit",
                    "type": "rate_limit_exceeded"
                }
            }),
            429,
            "Fallback message",
            None,
            Some("Type error".to_string()),
        );

        let rate_limit = error
            .as_rate_limit()
            .expect("rate_limit_exceeded maps to rate-limit error");
        assert_eq!(rate_limit.name(), "GatewayRateLimitError");
        assert_eq!(rate_limit.error_type(), "rate_limit_exceeded");
        assert_eq!(rate_limit.message(), "Rate limit hit");
        assert_eq!(rate_limit.status_code(), 429);
        assert_eq!(rate_limit.cause_message(), Some("Type error"));
    }

    #[test]
    fn create_gateway_error_from_response_maps_generation_id_to_error_variants() {
        let internal = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Internal server error",
                    "type": "internal_server_error"
                },
                "generationId": "gen_01ABC123XYZ"
            }),
            500,
            "Gateway request failed",
            None,
        );
        assert!(internal.as_internal_server().is_some());
        assert_eq!(internal.generation_id(), Some("gen_01ABC123XYZ"));

        let auth = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Invalid API key",
                    "type": "authentication_error"
                },
                "generationId": "gen_01AUTH456"
            }),
            401,
            "Gateway request failed",
            Some(GatewayAuthMethod::ApiKey),
        );
        assert!(auth.as_authentication().is_some());
        assert_eq!(auth.generation_id(), Some("gen_01AUTH456"));

        let rate_limit = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Rate limit exceeded",
                    "type": "rate_limit_exceeded"
                },
                "generationId": "gen_01RATE789"
            }),
            429,
            "Gateway request failed",
            None,
        );
        assert!(rate_limit.as_rate_limit().is_some());
        assert_eq!(rate_limit.generation_id(), Some("gen_01RATE789"));

        let model_not_found = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Model not found",
                    "type": "model_not_found",
                    "param": {
                        "modelId": "gpt-5"
                    }
                },
                "generationId": "gen_01MODEL000"
            }),
            404,
            "Gateway request failed",
            None,
        );
        let model_error = model_not_found
            .as_model_not_found()
            .expect("model_not_found maps with generation id");
        assert_eq!(model_error.model_id(), Some("gpt-5"));
        assert_eq!(model_not_found.generation_id(), Some("gen_01MODEL000"));

        let without_generation_id = create_gateway_error_from_response(
            json!({
                "error": {
                    "message": "Some error",
                    "type": "internal_server_error"
                }
            }),
            500,
            "Gateway request failed",
            None,
        );
        assert_eq!(without_generation_id.generation_id(), None);

        let malformed = create_gateway_error_from_response(
            json!({
                "invalidField": "value",
                "generationId": "gen_01MALFORMED"
            }),
            500,
            "Gateway request failed",
            None,
        );
        assert!(malformed.as_response().is_some());
        assert_eq!(malformed.generation_id(), Some("gen_01MALFORMED"));
    }

    #[test]
    fn create_gateway_error_from_response_creates_contextual_auth_errors() {
        let api_key = create_gateway_error_from_response(
            json!({
                "error": {
                    "type": "authentication_error",
                    "message": "Invalid API key"
                }
            }),
            401,
            "Gateway request failed",
            Some(GatewayAuthMethod::ApiKey),
        );
        let api_key_error = api_key
            .as_authentication()
            .expect("api-key auth failure maps to authentication error");
        assert!(api_key_error.message().contains("Invalid API key"));
        assert!(api_key_error.message().contains("vercel.com/d?to="));
        assert_eq!(api_key_error.status_code(), 401);

        let oidc = create_gateway_error_from_response(
            json!({
                "error": {
                    "type": "authentication_error",
                    "message": "Invalid OIDC token"
                }
            }),
            401,
            "Gateway request failed",
            Some(GatewayAuthMethod::Oidc),
        );
        let oidc_error = oidc
            .as_authentication()
            .expect("oidc auth failure maps to authentication error");
        assert!(oidc_error.message().contains("Invalid OIDC token"));
        assert!(oidc_error.message().contains("npx vercel link"));
        assert_eq!(oidc_error.status_code(), 401);

        let missing = create_gateway_error_from_response(
            json!({
                "error": {
                    "type": "authentication_error",
                    "message": "Authentication failed"
                }
            }),
            401,
            "Gateway request failed",
            None,
        );
        let missing_error = missing
            .as_authentication()
            .expect("missing auth context maps to authentication error");
        assert!(
            missing_error
                .message()
                .contains("No authentication provided")
        );
        assert_eq!(missing_error.status_code(), 401);
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
    fn extract_gateway_api_call_response_prefers_explicit_data_even_null_or_empty() {
        let null_data = ApiCallError::new("Request failed", "http://test.url", json!({}))
            .with_status_code(500)
            .with_data(json!(null))
            .with_response_body(r#"{"fallback":"data"}"#);
        assert_eq!(extract_gateway_api_call_response(&null_data), json!(null));

        let empty_data = ApiCallError::new("Request failed", "http://test.url", json!({}))
            .with_status_code(400)
            .with_data(json!({}))
            .with_response_body(r#"{"fallback":"data"}"#);
        assert_eq!(extract_gateway_api_call_response(&empty_data), json!({}));
    }

    #[test]
    fn extract_gateway_api_call_response_parses_json_or_returns_raw_text() {
        let complex_data = json!({
            "error": {
                "message": "Complex error",
                "type": "validation_error",
                "details": {
                    "field": "prompt",
                    "issues": [
                        {
                            "code": "too_long",
                            "message": "Prompt exceeds maximum length"
                        },
                        {
                            "code": "invalid_format",
                            "message": "Contains invalid characters"
                        }
                    ]
                }
            },
            "metadata": {
                "requestId": "12345",
                "timestamp": "2024-01-01T00:00:00Z"
            }
        });
        let complex_error = ApiCallError::new("Request failed", "http://test.url", json!({}))
            .with_status_code(400)
            .with_response_body(complex_data.to_string());
        assert_eq!(
            extract_gateway_api_call_response(&complex_error),
            complex_data
        );

        for raw_body in [
            "This is not valid JSON",
            "<html><body><h1>500 Internal Server Error</h1></body></html>",
            "",
            r#"{"incomplete": json"#,
        ] {
            let error = ApiCallError::new("Request failed", "http://test.url", json!({}))
                .with_status_code(500)
                .with_response_body(raw_body);
            assert_eq!(extract_gateway_api_call_response(&error), json!(raw_body));
        }
    }

    #[test]
    fn extract_gateway_api_call_response_returns_empty_object_without_body() {
        let error =
            ApiCallError::new("Request failed", "http://test.url", json!({})).with_status_code(500);

        assert_eq!(extract_gateway_api_call_response(&error), json!({}));
    }

    #[test]
    fn extract_gateway_api_call_response_parses_scalar_and_array_bodies() {
        for (body, expected) in [
            ("404", json!(404)),
            ("true", json!(true)),
            (
                r#"["error1","error2","error3"]"#,
                json!(["error1", "error2", "error3"]),
            ),
        ] {
            let error = ApiCallError::new("Request failed", "http://test.url", json!({}))
                .with_status_code(500)
                .with_response_body(body);

            assert_eq!(extract_gateway_api_call_response(&error), expected);
        }
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
    fn gateway_auth_method_header_matches_upstream_name() {
        assert_eq!(GATEWAY_AUTH_METHOD_HEADER, "ai-gateway-auth-method");
    }

    #[test]
    fn parse_gateway_auth_method_accepts_valid_values_and_extra_headers() {
        assert_eq!(
            parse_gateway_auth_method(&headers_with_auth_method(Some("api-key"))),
            Some(GatewayAuthMethod::ApiKey)
        );
        assert_eq!(
            parse_gateway_auth_method(&headers_with_auth_method(Some("oidc"))),
            Some(GatewayAuthMethod::Oidc)
        );

        let headers = BTreeMap::from([
            (
                "authorization".to_string(),
                Some("Bearer token".to_string()),
            ),
            (
                "content-type".to_string(),
                Some("application/json".to_string()),
            ),
            (
                GATEWAY_AUTH_METHOD_HEADER.to_string(),
                Some("api-key".to_string()),
            ),
            ("user-agent".to_string(), Some("test-agent".to_string())),
        ]);
        assert_eq!(
            parse_gateway_auth_method(&headers),
            Some(GatewayAuthMethod::ApiKey)
        );
    }

    #[test]
    fn parse_gateway_auth_method_rejects_invalid_values() {
        for value in ["invalid-method", "", "123", "true", "API-KEY", "OIDC"] {
            assert_eq!(
                parse_gateway_auth_method(&headers_with_auth_method(Some(value))),
                None
            );
        }
    }

    #[test]
    fn parse_gateway_auth_method_returns_none_for_missing_or_nullish_headers() {
        let missing = BTreeMap::from([(
            "authorization".to_string(),
            Some("Bearer token".to_string()),
        )]);
        assert_eq!(parse_gateway_auth_method(&missing), None);
        assert_eq!(
            parse_gateway_auth_method(&headers_with_auth_method(None)),
            None
        );
        assert_eq!(parse_gateway_auth_method(&BTreeMap::new()), None);
    }

    #[test]
    fn parse_gateway_auth_method_rejects_whitespace() {
        for value in ["   ", " api-key "] {
            assert_eq!(
                parse_gateway_auth_method(&headers_with_auth_method(Some(value))),
                None
            );
        }
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
    fn as_gateway_error_detects_all_undici_timeout_codes() {
        for (code, message) in [
            ("UND_ERR_HEADERS_TIMEOUT", "Request timeout"),
            ("UND_ERR_BODY_TIMEOUT", "Body timeout"),
            ("UND_ERR_CONNECT_TIMEOUT", "Connect timeout"),
        ] {
            let result = as_gateway_error(
                HandledFetchError::Original {
                    error: FetchErrorInfo::new(message).with_code(code),
                },
                None,
            );

            let timeout = result
                .as_timeout()
                .expect("Undici timeout codes map to GatewayTimeoutError");
            assert!(timeout.message().contains(message));
            assert_eq!(timeout.status_code(), 408);
            assert_eq!(timeout.error_type(), "timeout_error");
            assert_eq!(timeout.cause_message(), Some(message));
        }
    }

    #[test]
    fn as_gateway_error_maps_non_timeout_original_errors_to_response_errors() {
        let network = as_gateway_error(
            HandledFetchError::Original {
                error: FetchErrorInfo::new("Network error"),
            },
            None,
        );
        let network_error = gateway_response_error(&network);
        assert!(
            network_error
                .message()
                .contains("Gateway request failed: Network error")
        );
        assert_eq!(network_error.cause_message(), Some("Network error"));

        let connection = as_gateway_error(
            HandledFetchError::Original {
                error: FetchErrorInfo::new("Connection refused").with_code("ECONNREFUSED"),
            },
            None,
        );
        assert!(connection.as_timeout().is_none());
        assert!(connection.as_response().is_some());
    }

    #[test]
    fn gateway_headers_from_auth_method_uses_upstream_header_name() {
        assert_eq!(
            gateway_headers_from_auth_method(GatewayAuthMethod::Oidc),
            Headers::from([("ai-gateway-auth-method".to_string(), "oidc".to_string(),)])
        );
    }
}
