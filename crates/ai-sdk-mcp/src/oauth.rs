//! OAuth helpers for upstream `@ai-sdk/mcp` parity.

use std::collections::BTreeMap;
use std::fmt;

use ai_sdk_provider::{JsonObject, JsonValue};
use ai_sdk_provider_utils::convert_bytes_to_base64;
use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};
use serde::de::Error as SerdeError;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;
use url::form_urlencoded::Serializer as FormUrlEncodedSerializer;

use crate::LATEST_PROTOCOL_VERSION;

/// Result alias for MCP OAuth helpers.
pub type McpOAuthResult<T> = Result<T, McpOAuthError>;

/// Error returned by MCP OAuth discovery and authorization helpers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpOAuthError {
    pub message: String,
    pub error_code: Option<String>,
    pub error_uri: Option<String>,
}

impl McpOAuthError {
    /// Creates an MCP OAuth error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_code: None,
            error_uri: None,
        }
    }

    /// Creates an MCP OAuth error from an OAuth error response body.
    pub fn from_oauth_response(
        error_code: impl Into<String>,
        message: impl Into<String>,
        error_uri: Option<String>,
    ) -> Self {
        Self {
            message: message.into(),
            error_code: Some(error_code.into()),
            error_uri,
        }
    }
}

impl fmt::Display for McpOAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for McpOAuthError {}

/// OAuth 2.1 token response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OAuthTokens {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    pub token_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

/// OAuth 2.0 Protected Resource Metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OAuthProtectedResourceMetadata {
    #[serde(deserialize_with = "deserialize_url_string")]
    pub resource: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_url_vec",
        skip_serializing_if = "Option::is_none"
    )]
    pub authorization_servers: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_url_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub jwks_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_methods_supported: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_signing_alg_values_supported: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_documentation: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_url_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub resource_policy_uri: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_url_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub resource_tos_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_client_certificate_bound_access_tokens: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_details_types_supported: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpop_signing_alg_values_supported: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpop_bound_access_tokens_required: Option<bool>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// OAuth Authorization Server Metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OAuthMetadata {
    pub issuer: String,
    #[serde(deserialize_with = "deserialize_safe_url_string")]
    pub authorization_endpoint: String,
    #[serde(deserialize_with = "deserialize_safe_url_string")]
    pub token_endpoint: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_url_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub registration_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    pub response_types_supported: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_types_supported: Option<Vec<String>>,
    pub code_challenge_methods_supported: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_signing_alg_values_supported: Option<Vec<String>>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// OpenID Connect Discovery 1.0 Provider Metadata plus MCP-required PKCE data.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OpenIdProviderDiscoveryMetadata {
    pub issuer: String,
    #[serde(deserialize_with = "deserialize_safe_url_string")]
    pub authorization_endpoint: String,
    #[serde(deserialize_with = "deserialize_safe_url_string")]
    pub token_endpoint: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_url_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub userinfo_endpoint: Option<String>,
    #[serde(deserialize_with = "deserialize_safe_url_string")]
    pub jwks_uri: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_url_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub registration_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    pub response_types_supported: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_types_supported: Option<Vec<String>>,
    pub subject_types_supported: Vec<String>,
    pub id_token_signing_alg_values_supported: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claims_supported: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,
    pub code_challenge_methods_supported: Vec<String>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// Authorization server metadata discovered from OAuth or OIDC well-known endpoints.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum AuthorizationServerMetadata {
    OAuth(OAuthMetadata),
    OpenId(OpenIdProviderDiscoveryMetadata),
}

impl AuthorizationServerMetadata {
    /// Returns the authorization endpoint URL.
    pub fn authorization_endpoint(&self) -> &str {
        match self {
            Self::OAuth(metadata) => &metadata.authorization_endpoint,
            Self::OpenId(metadata) => &metadata.authorization_endpoint,
        }
    }

    /// Returns the token endpoint URL.
    pub fn token_endpoint(&self) -> &str {
        match self {
            Self::OAuth(metadata) => &metadata.token_endpoint,
            Self::OpenId(metadata) => &metadata.token_endpoint,
        }
    }

    /// Returns the dynamic client registration endpoint URL, when present.
    pub fn registration_endpoint(&self) -> Option<&str> {
        match self {
            Self::OAuth(metadata) => metadata.registration_endpoint.as_deref(),
            Self::OpenId(metadata) => metadata.registration_endpoint.as_deref(),
        }
    }

    /// Returns response types supported by the server.
    pub fn response_types_supported(&self) -> &[String] {
        match self {
            Self::OAuth(metadata) => &metadata.response_types_supported,
            Self::OpenId(metadata) => &metadata.response_types_supported,
        }
    }

    /// Returns PKCE challenge methods supported by the server.
    pub fn code_challenge_methods_supported(&self) -> &[String] {
        match self {
            Self::OAuth(metadata) => &metadata.code_challenge_methods_supported,
            Self::OpenId(metadata) => &metadata.code_challenge_methods_supported,
        }
    }

    /// Returns grant types supported by the server, when advertised.
    pub fn grant_types_supported(&self) -> Option<&[String]> {
        match self {
            Self::OAuth(metadata) => metadata.grant_types_supported.as_deref(),
            Self::OpenId(metadata) => metadata.grant_types_supported.as_deref(),
        }
    }

    /// Returns token endpoint authentication methods supported by the server.
    pub fn token_endpoint_auth_methods_supported(&self) -> Option<&[String]> {
        match self {
            Self::OAuth(metadata) => metadata.token_endpoint_auth_methods_supported.as_deref(),
            Self::OpenId(metadata) => metadata.token_endpoint_auth_methods_supported.as_deref(),
        }
    }
}

impl From<OAuthMetadata> for AuthorizationServerMetadata {
    fn from(metadata: OAuthMetadata) -> Self {
        Self::OAuth(metadata)
    }
}

impl From<OpenIdProviderDiscoveryMetadata> for AuthorizationServerMetadata {
    fn from(metadata: OpenIdProviderDiscoveryMetadata) -> Self {
        Self::OpenId(metadata)
    }
}

/// OAuth dynamic client information.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OAuthClientInformation {
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id_issued_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret_expires_at: Option<u64>,
}

impl OAuthClientInformation {
    /// Creates OAuth client information for a public client id.
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            client_id_issued_at: None,
            client_secret_expires_at: None,
        }
    }

    /// Sets a client secret.
    pub fn with_client_secret(mut self, client_secret: impl Into<String>) -> Self {
        self.client_secret = Some(client_secret.into());
        self
    }
}

/// OAuth dynamic client metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OAuthClientMetadata {
    #[serde(deserialize_with = "deserialize_safe_url_vec")]
    pub redirect_uris: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_types: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_types: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_url_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_uri: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_url_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub logo_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_url_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub tos_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_uri: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_url_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub jwks_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub software_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub software_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub software_statement: Option<String>,
}

impl OAuthClientMetadata {
    /// Creates OAuth client metadata for the supplied redirect URIs.
    pub fn new(redirect_uris: Vec<String>) -> Self {
        Self {
            redirect_uris,
            token_endpoint_auth_method: None,
            grant_types: None,
            response_types: None,
            client_name: None,
            client_uri: None,
            logo_uri: None,
            scope: None,
            contacts: None,
            tos_uri: None,
            policy_uri: None,
            jwks_uri: None,
            jwks: None,
            software_id: None,
            software_version: None,
            software_statement: None,
        }
    }

    /// Sets the OAuth client display name.
    pub fn with_client_name(mut self, client_name: impl Into<String>) -> Self {
        self.client_name = Some(client_name.into());
        self
    }

    /// Sets the default requested OAuth scope.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }
}

/// Dynamic client information combined with client metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OAuthClientInformationFull {
    #[serde(flatten)]
    pub metadata: OAuthClientMetadata,
    #[serde(flatten)]
    pub information: OAuthClientInformation,
}

/// OAuth error response body.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OAuthErrorResponse {
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_uri: Option<String>,
}

/// Discovery metadata endpoint family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryMetadataType {
    OAuth,
    OpenId,
}

/// Authorization server discovery URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryUrl {
    pub url: Url,
    pub metadata_type: DiscoveryMetadataType,
}

/// Options for OAuth protected-resource metadata discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthProtectedResourceMetadataDiscoveryOptions {
    pub protocol_version: String,
    pub resource_metadata_url: Option<String>,
}

impl Default for OAuthProtectedResourceMetadataDiscoveryOptions {
    fn default() -> Self {
        Self {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            resource_metadata_url: None,
        }
    }
}

impl OAuthProtectedResourceMetadataDiscoveryOptions {
    /// Uses an explicit protected-resource metadata URL and disables root fallback.
    pub fn with_resource_metadata_url(mut self, resource_metadata_url: impl Into<String>) -> Self {
        self.resource_metadata_url = Some(resource_metadata_url.into());
        self
    }

    /// Uses a custom MCP protocol version header.
    pub fn with_protocol_version(mut self, protocol_version: impl Into<String>) -> Self {
        self.protocol_version = protocol_version.into();
        self
    }
}

/// Options for authorization-server metadata discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizationServerMetadataDiscoveryOptions {
    pub protocol_version: String,
}

impl Default for AuthorizationServerMetadataDiscoveryOptions {
    fn default() -> Self {
        Self {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
        }
    }
}

impl AuthorizationServerMetadataDiscoveryOptions {
    /// Uses a custom MCP protocol version header.
    pub fn with_protocol_version(mut self, protocol_version: impl Into<String>) -> Self {
        self.protocol_version = protocol_version.into();
        self
    }
}

/// Caller-provided PKCE challenge material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthPkceChallenge {
    pub code_verifier: String,
    pub code_challenge: String,
}

impl OAuthPkceChallenge {
    /// Creates PKCE challenge material.
    pub fn new(code_verifier: impl Into<String>, code_challenge: impl Into<String>) -> Self {
        Self {
            code_verifier: code_verifier.into(),
            code_challenge: code_challenge.into(),
        }
    }

    /// Derives an S256 PKCE code challenge from a verifier.
    pub fn from_code_verifier(code_verifier: impl Into<String>) -> Self {
        let code_verifier = code_verifier.into();
        let hashed_verifier = digest::digest(&digest::SHA256, code_verifier.as_bytes());
        Self {
            code_verifier,
            code_challenge: base64_url_no_padding(hashed_verifier.as_ref()),
        }
    }

    /// Generates random PKCE material using an S256 code challenge.
    pub fn generate() -> McpOAuthResult<Self> {
        let mut verifier_bytes = [0u8; 32];
        SystemRandom::new()
            .fill(&mut verifier_bytes)
            .map_err(|_| McpOAuthError::new("failed to generate PKCE code verifier"))?;
        Ok(Self::from_code_verifier(base64_url_no_padding(
            &verifier_bytes,
        )))
    }
}

/// Options for constructing an authorization redirect URL.
#[derive(Clone, Debug, PartialEq)]
pub struct StartAuthorizationOptions {
    pub metadata: Option<AuthorizationServerMetadata>,
    pub client_information: OAuthClientInformation,
    pub redirect_url: String,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub resource: Option<Url>,
    pub pkce_challenge: OAuthPkceChallenge,
}

impl StartAuthorizationOptions {
    /// Creates authorization options with required client, redirect, and PKCE data.
    pub fn new(
        client_information: OAuthClientInformation,
        redirect_url: impl Into<String>,
        pkce_challenge: OAuthPkceChallenge,
    ) -> Self {
        Self {
            metadata: None,
            client_information,
            redirect_url: redirect_url.into(),
            scope: None,
            state: None,
            resource: None,
            pkce_challenge,
        }
    }

    /// Creates authorization options with generated S256 PKCE material.
    pub fn with_generated_pkce(
        client_information: OAuthClientInformation,
        redirect_url: impl Into<String>,
    ) -> McpOAuthResult<Self> {
        Ok(Self::new(
            client_information,
            redirect_url,
            OAuthPkceChallenge::generate()?,
        ))
    }

    /// Sets authorization server metadata.
    pub fn with_metadata(mut self, metadata: impl Into<AuthorizationServerMetadata>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Sets the requested OAuth scope.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Sets the OAuth state parameter.
    pub fn with_state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }

    /// Sets the RFC 8707 resource parameter.
    pub fn with_resource(mut self, resource: Url) -> Self {
        self.resource = Some(resource);
        self
    }
}

/// Result of constructing an authorization redirect URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartAuthorizationResult {
    pub authorization_url: Url,
    pub code_verifier: String,
}

/// Options for exchanging an OAuth authorization code.
#[derive(Clone, Debug, PartialEq)]
pub struct ExchangeAuthorizationOptions {
    pub metadata: Option<AuthorizationServerMetadata>,
    pub client_information: OAuthClientInformation,
    pub authorization_code: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub resource: Option<Url>,
}

impl ExchangeAuthorizationOptions {
    /// Creates authorization-code exchange options.
    pub fn new(
        client_information: OAuthClientInformation,
        authorization_code: impl Into<String>,
        code_verifier: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Self {
        Self {
            metadata: None,
            client_information,
            authorization_code: authorization_code.into(),
            code_verifier: code_verifier.into(),
            redirect_uri: redirect_uri.into(),
            resource: None,
        }
    }

    /// Sets authorization server metadata.
    pub fn with_metadata(mut self, metadata: impl Into<AuthorizationServerMetadata>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Sets the RFC 8707 resource parameter.
    pub fn with_resource(mut self, resource: Url) -> Self {
        self.resource = Some(resource);
        self
    }
}

/// Options for refreshing OAuth tokens.
#[derive(Clone, Debug, PartialEq)]
pub struct RefreshAuthorizationOptions {
    pub metadata: Option<AuthorizationServerMetadata>,
    pub client_information: OAuthClientInformation,
    pub refresh_token: String,
    pub resource: Option<Url>,
}

impl RefreshAuthorizationOptions {
    /// Creates refresh-token exchange options.
    pub fn new(
        client_information: OAuthClientInformation,
        refresh_token: impl Into<String>,
    ) -> Self {
        Self {
            metadata: None,
            client_information,
            refresh_token: refresh_token.into(),
            resource: None,
        }
    }

    /// Sets authorization server metadata.
    pub fn with_metadata(mut self, metadata: impl Into<AuthorizationServerMetadata>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Sets the RFC 8707 resource parameter.
    pub fn with_resource(mut self, resource: Url) -> Self {
        self.resource = Some(resource);
        self
    }
}

/// Options for OAuth dynamic client registration.
#[derive(Clone, Debug, PartialEq)]
pub struct RegisterClientOptions {
    pub metadata: Option<AuthorizationServerMetadata>,
    pub client_metadata: OAuthClientMetadata,
}

impl RegisterClientOptions {
    /// Creates dynamic client registration options.
    pub fn new(client_metadata: OAuthClientMetadata) -> Self {
        Self {
            metadata: None,
            client_metadata,
        }
    }

    /// Sets authorization server metadata.
    pub fn with_metadata(mut self, metadata: impl Into<AuthorizationServerMetadata>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }
}

/// Validates that a URL is parseable and does not use a dangerous scheme.
pub fn validate_safe_url(value: &str) -> McpOAuthResult<()> {
    let url = Url::parse(value)
        .map_err(|error| McpOAuthError::new(format!("URL must be parseable: {error}")))?;
    match url.scheme().to_ascii_lowercase().as_str() {
        "javascript" | "data" | "vbscript" => Err(McpOAuthError::new(
            "URL cannot use javascript:, data:, or vbscript: scheme",
        )),
        _ => Ok(()),
    }
}

/// Converts a server URL to a resource URL by removing the fragment.
pub fn resource_url_from_server_url(url: impl AsRef<str>) -> McpOAuthResult<Url> {
    let mut resource_url = Url::parse(url.as_ref())
        .map_err(|error| McpOAuthError::new(format!("invalid resource URL: {error}")))?;
    resource_url.set_fragment(None);
    Ok(resource_url)
}

/// Serializes a resource URL, removing the trailing slash added to pathless URLs.
pub fn resource_url_strip_slash(resource: &Url) -> String {
    let href = resource.as_str();
    if resource.path() == "/" && href.ends_with('/') {
        href[..href.len() - 1].to_string()
    } else {
        href.to_string()
    }
}

fn base64_url_no_padding(bytes: &[u8]) -> String {
    convert_bytes_to_base64(bytes)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

/// Checks whether a requested resource URL is allowed by a configured resource URL.
pub fn check_resource_allowed(
    requested_resource: impl AsRef<str>,
    configured_resource: impl AsRef<str>,
) -> McpOAuthResult<bool> {
    let requested = Url::parse(requested_resource.as_ref())
        .map_err(|error| McpOAuthError::new(format!("invalid requested resource URL: {error}")))?;
    let configured = Url::parse(configured_resource.as_ref())
        .map_err(|error| McpOAuthError::new(format!("invalid configured resource URL: {error}")))?;

    if !same_origin(&requested, &configured) {
        return Ok(false);
    }

    if requested.path().len() < configured.path().len() {
        return Ok(false);
    }

    let requested_path = path_with_trailing_slash(requested.path());
    let configured_path = path_with_trailing_slash(configured.path());
    Ok(requested_path.starts_with(&configured_path))
}

/// Selects the RFC 8707 resource URL to include in OAuth requests.
///
/// When protected-resource metadata is unavailable, MCP's upstream auth flow
/// omits the `resource` parameter. When it is available, the configured
/// protected resource must match the requested server URL by origin and path
/// prefix.
pub fn select_resource_url(
    server_url: impl AsRef<str>,
    resource_metadata: Option<&OAuthProtectedResourceMetadata>,
) -> McpOAuthResult<Option<Url>> {
    let Some(resource_metadata) = resource_metadata else {
        return Ok(None);
    };
    let default_resource = resource_url_from_server_url(server_url)?;
    if !check_resource_allowed(default_resource.as_str(), &resource_metadata.resource)? {
        return Err(McpOAuthError::new(format!(
            "Protected resource {} does not match expected {} (or origin)",
            resource_metadata.resource, default_resource
        )));
    }
    Url::parse(&resource_metadata.resource)
        .map(Some)
        .map_err(|error| McpOAuthError::new(format!("invalid protected resource URL: {error}")))
}

/// Extracts the protected-resource metadata URL from a `WWW-Authenticate` header value.
pub fn extract_resource_metadata_url(header: Option<&str>) -> Option<Url> {
    let header = header?;
    let (auth_type, _) = header.split_once(' ')?;
    if !auth_type.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let marker = "resource_metadata=\"";
    let start = header.find(marker)? + marker.len();
    let end = header[start..].find('"')? + start;
    Url::parse(&header[start..end]).ok()
}

/// Builds authorization-server metadata discovery URLs in upstream priority order.
pub fn build_discovery_urls(
    authorization_server_url: impl AsRef<str>,
) -> McpOAuthResult<Vec<DiscoveryUrl>> {
    let url = Url::parse(authorization_server_url.as_ref()).map_err(|error| {
        McpOAuthError::new(format!(
            "invalid authorization server discovery URL: {error}"
        ))
    })?;
    let origin = origin_string(&url).ok_or_else(|| {
        McpOAuthError::new(format!("invalid authorization server discovery URL: {url}"))
    })?;
    let has_path = url.path() != "/";

    if !has_path {
        return Ok(vec![
            DiscoveryUrl {
                url: Url::parse(&format!("{origin}/.well-known/oauth-authorization-server"))
                    .expect("origin and OAuth well-known path form a URL"),
                metadata_type: DiscoveryMetadataType::OAuth,
            },
            DiscoveryUrl {
                url: Url::parse(&format!("{origin}/.well-known/openid-configuration"))
                    .expect("origin and OIDC well-known path form a URL"),
                metadata_type: DiscoveryMetadataType::OpenId,
            },
        ]);
    }

    let pathname = url.path().trim_end_matches('/');
    Ok(vec![
        DiscoveryUrl {
            url: Url::parse(&format!(
                "{origin}/.well-known/oauth-authorization-server{pathname}"
            ))
            .expect("origin and OAuth path-aware well-known path form a URL"),
            metadata_type: DiscoveryMetadataType::OAuth,
        },
        DiscoveryUrl {
            url: Url::parse(&format!("{origin}/.well-known/oauth-authorization-server"))
                .expect("origin and OAuth root well-known path form a URL"),
            metadata_type: DiscoveryMetadataType::OAuth,
        },
        DiscoveryUrl {
            url: Url::parse(&format!(
                "{origin}/.well-known/openid-configuration{pathname}"
            ))
            .expect("origin and OIDC path-aware well-known path form a URL"),
            metadata_type: DiscoveryMetadataType::OpenId,
        },
        DiscoveryUrl {
            url: Url::parse(&format!(
                "{origin}{pathname}/.well-known/openid-configuration"
            ))
            .expect("origin and OIDC nested well-known path form a URL"),
            metadata_type: DiscoveryMetadataType::OpenId,
        },
    ])
}

/// Discovers OAuth 2.0 Protected Resource Metadata over HTTP.
pub fn discover_oauth_protected_resource_metadata(
    server_url: impl AsRef<str>,
    options: OAuthProtectedResourceMetadataDiscoveryOptions,
) -> McpOAuthResult<OAuthProtectedResourceMetadata> {
    let response = discover_metadata_with_fallback(
        server_url.as_ref(),
        WellKnownType::OAuthProtectedResource,
        &options.protocol_version,
        options.resource_metadata_url.as_deref(),
    )?;

    if response.status == 404 {
        return Err(McpOAuthError::new(
            "Resource server does not implement OAuth 2.0 Protected Resource Metadata.",
        ));
    }

    if !(200..300).contains(&response.status) {
        return Err(McpOAuthError::new(format!(
            "HTTP {} trying to load well-known OAuth protected resource metadata.",
            response.status
        )));
    }

    serde_json::from_str(&response.body).map_err(|error| {
        McpOAuthError::new(format!(
            "Failed to parse OAuth protected resource metadata: {error}"
        ))
    })
}

/// Discovers OAuth or OpenID authorization-server metadata over HTTP.
pub fn discover_authorization_server_metadata(
    authorization_server_url: impl AsRef<str>,
    options: AuthorizationServerMetadataDiscoveryOptions,
) -> McpOAuthResult<Option<AuthorizationServerMetadata>> {
    for discovery_url in build_discovery_urls(authorization_server_url.as_ref())? {
        let response = fetch_metadata(&discovery_url.url, &options.protocol_version)?;
        if response.status >= 400 && response.status < 500 {
            continue;
        }
        if !(200..300).contains(&response.status) {
            let metadata_name = match discovery_url.metadata_type {
                DiscoveryMetadataType::OAuth => "OAuth",
                DiscoveryMetadataType::OpenId => "OpenID provider",
            };
            return Err(McpOAuthError::new(format!(
                "HTTP {} trying to load {metadata_name} metadata from {}",
                response.status, discovery_url.url
            )));
        }

        return match discovery_url.metadata_type {
            DiscoveryMetadataType::OAuth => {
                let metadata =
                    serde_json::from_str::<OAuthMetadata>(&response.body).map_err(|error| {
                        McpOAuthError::new(format!(
                            "Failed to parse OAuth authorization server metadata: {error}"
                        ))
                    })?;
                Ok(Some(AuthorizationServerMetadata::OAuth(metadata)))
            }
            DiscoveryMetadataType::OpenId => {
                let metadata =
                    serde_json::from_str::<OpenIdProviderDiscoveryMetadata>(&response.body)
                        .map_err(|error| {
                            McpOAuthError::new(format!(
                                "Failed to parse OpenID provider metadata: {error}"
                            ))
                        })?;
                if !metadata
                    .code_challenge_methods_supported
                    .iter()
                    .any(|method| method == "S256")
                {
                    return Err(McpOAuthError::new(format!(
                        "Incompatible OIDC provider at {}: does not support S256 code challenge method required by MCP specification",
                        discovery_url.url
                    )));
                }
                Ok(Some(AuthorizationServerMetadata::OpenId(metadata)))
            }
        };
    }

    Ok(None)
}

/// Constructs an OAuth authorization URL with upstream MCP query parameters.
pub fn start_authorization(
    authorization_server_url: impl AsRef<str>,
    options: StartAuthorizationOptions,
) -> McpOAuthResult<StartAuthorizationResult> {
    let response_type = "code";
    let code_challenge_method = "S256";

    let mut authorization_url = if let Some(metadata) = &options.metadata {
        if !metadata
            .response_types_supported()
            .iter()
            .any(|supported| supported == response_type)
        {
            return Err(McpOAuthError::new(format!(
                "Incompatible auth server: does not support response type {response_type}"
            )));
        }
        if !metadata
            .code_challenge_methods_supported()
            .iter()
            .any(|supported| supported == code_challenge_method)
        {
            return Err(McpOAuthError::new(format!(
                "Incompatible auth server: does not support code challenge method {code_challenge_method}"
            )));
        }
        Url::parse(metadata.authorization_endpoint()).map_err(|error| {
            McpOAuthError::new(format!("invalid authorization endpoint URL: {error}"))
        })?
    } else {
        Url::parse(authorization_server_url.as_ref())
            .and_then(|url| url.join("/authorize"))
            .map_err(|error| McpOAuthError::new(format!("invalid authorization URL: {error}")))?
    };

    set_url_query_param(&mut authorization_url, "response_type", response_type);
    set_url_query_param(
        &mut authorization_url,
        "client_id",
        &options.client_information.client_id,
    );
    set_url_query_param(
        &mut authorization_url,
        "code_challenge",
        &options.pkce_challenge.code_challenge,
    );
    set_url_query_param(
        &mut authorization_url,
        "code_challenge_method",
        code_challenge_method,
    );
    set_url_query_param(
        &mut authorization_url,
        "redirect_uri",
        &options.redirect_url,
    );

    if let Some(state) = &options.state {
        set_url_query_param(&mut authorization_url, "state", state);
    }
    if let Some(scope) = &options.scope {
        set_url_query_param(&mut authorization_url, "scope", scope);
        if scope.contains("offline_access") {
            append_url_query_param(&mut authorization_url, "prompt", "consent");
        }
    }
    if let Some(resource) = &options.resource {
        set_url_query_param(
            &mut authorization_url,
            "resource",
            &resource_url_strip_slash(resource),
        );
    }

    Ok(StartAuthorizationResult {
        authorization_url,
        code_verifier: options.pkce_challenge.code_verifier,
    })
}

/// Parses an OAuth error response body.
pub fn parse_oauth_error_response(status: Option<u16>, body: &str) -> McpOAuthError {
    match serde_json::from_str::<OAuthErrorResponse>(body) {
        Ok(error) => McpOAuthError::from_oauth_response(
            error.error,
            error.error_description.unwrap_or_default(),
            error.error_uri,
        ),
        Err(error) => {
            let status_prefix =
                status.map_or_else(String::new, |status| format!("HTTP {status}: "));
            McpOAuthError::new(format!(
                "{status_prefix}Invalid OAuth error response: {error}. Raw body: {body}"
            ))
        }
    }
}

/// Exchanges an authorization code for OAuth tokens.
pub fn exchange_authorization(
    authorization_server_url: impl AsRef<str>,
    options: ExchangeAuthorizationOptions,
) -> McpOAuthResult<OAuthTokens> {
    let grant_type = "authorization_code";
    let token_url =
        token_endpoint_url(authorization_server_url.as_ref(), options.metadata.as_ref())?;
    validate_grant_type(options.metadata.as_ref(), grant_type)?;

    let mut params = BTreeMap::from([
        ("grant_type".to_string(), grant_type.to_string()),
        ("code".to_string(), options.authorization_code),
        ("code_verifier".to_string(), options.code_verifier),
        ("redirect_uri".to_string(), options.redirect_uri),
    ]);
    let mut headers = token_request_headers();
    apply_client_authentication(
        select_client_auth_method(
            &options.client_information,
            supported_auth_methods(options.metadata.as_ref()),
        ),
        &options.client_information,
        &mut headers,
        &mut params,
    )?;
    if let Some(resource) = &options.resource {
        params.insert("resource".to_string(), resource_url_strip_slash(resource));
    }

    let response = post_form_urlencoded(&token_url, &headers, &params)?;
    if !(200..300).contains(&response.status) {
        return Err(parse_oauth_error_response(
            Some(response.status),
            &response.body,
        ));
    }
    serde_json::from_str::<OAuthTokens>(&response.body).map_err(|error| {
        McpOAuthError::new(format!("Failed to parse OAuth token response: {error}"))
    })
}

/// Exchanges a refresh token for updated OAuth tokens.
pub fn refresh_authorization(
    authorization_server_url: impl AsRef<str>,
    options: RefreshAuthorizationOptions,
) -> McpOAuthResult<OAuthTokens> {
    let grant_type = "refresh_token";
    let token_url =
        token_endpoint_url(authorization_server_url.as_ref(), options.metadata.as_ref())?;
    validate_grant_type(options.metadata.as_ref(), grant_type)?;

    let original_refresh_token = options.refresh_token.clone();
    let mut params = BTreeMap::from([
        ("grant_type".to_string(), grant_type.to_string()),
        ("refresh_token".to_string(), options.refresh_token),
    ]);
    let mut headers = token_request_headers();
    apply_client_authentication(
        select_client_auth_method(
            &options.client_information,
            supported_auth_methods(options.metadata.as_ref()),
        ),
        &options.client_information,
        &mut headers,
        &mut params,
    )?;
    if let Some(resource) = &options.resource {
        params.insert("resource".to_string(), resource_url_strip_slash(resource));
    }

    let response = post_form_urlencoded(&token_url, &headers, &params)?;
    if !(200..300).contains(&response.status) {
        return Err(parse_oauth_error_response(
            Some(response.status),
            &response.body,
        ));
    }
    let mut tokens = serde_json::from_str::<OAuthTokens>(&response.body).map_err(|error| {
        McpOAuthError::new(format!(
            "Failed to parse OAuth refresh token response: {error}"
        ))
    })?;
    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(original_refresh_token);
    }
    Ok(tokens)
}

/// Performs OAuth 2.0 Dynamic Client Registration.
pub fn register_client(
    authorization_server_url: impl AsRef<str>,
    options: RegisterClientOptions,
) -> McpOAuthResult<OAuthClientInformationFull> {
    let registration_url = if let Some(metadata) = &options.metadata {
        let registration_endpoint = metadata.registration_endpoint().ok_or_else(|| {
            McpOAuthError::new(
                "Incompatible auth server: does not support dynamic client registration",
            )
        })?;
        Url::parse(registration_endpoint).map_err(|error| {
            McpOAuthError::new(format!("invalid registration endpoint URL: {error}"))
        })?
    } else {
        Url::parse(authorization_server_url.as_ref())
            .and_then(|url| url.join("/register"))
            .map_err(|error| McpOAuthError::new(format!("invalid registration URL: {error}")))?
    };

    let body = serde_json::to_string(&options.client_metadata).map_err(|error| {
        McpOAuthError::new(format!("Failed to serialize client metadata: {error}"))
    })?;
    let response = post_json(&registration_url, &body)?;
    if !(200..300).contains(&response.status) {
        return Err(parse_oauth_error_response(
            Some(response.status),
            &response.body,
        ));
    }
    serde_json::from_str::<OAuthClientInformationFull>(&response.body).map_err(|error| {
        McpOAuthError::new(format!(
            "Failed to parse OAuth client information response: {error}"
        ))
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WellKnownType {
    OAuthProtectedResource,
}

impl WellKnownType {
    fn path_prefix(self) -> &'static str {
        match self {
            Self::OAuthProtectedResource => "oauth-protected-resource",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OAuthHttpResponse {
    status: u16,
    body: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClientAuthMethod {
    ClientSecretBasic,
    ClientSecretPost,
    None,
}

fn discover_metadata_with_fallback(
    server_url: &str,
    well_known_type: WellKnownType,
    protocol_version: &str,
    metadata_url: Option<&str>,
) -> McpOAuthResult<OAuthHttpResponse> {
    let issuer = Url::parse(server_url)
        .map_err(|error| McpOAuthError::new(format!("invalid OAuth discovery URL: {error}")))?;
    let discovery_url = if let Some(metadata_url) = metadata_url {
        Url::parse(metadata_url)
            .map_err(|error| McpOAuthError::new(format!("invalid OAuth metadata URL: {error}")))?
    } else {
        let well_known_path = build_well_known_path(well_known_type, issuer.path());
        let mut url = issuer
            .join(&well_known_path)
            .map_err(|error| McpOAuthError::new(format!("invalid OAuth metadata URL: {error}")))?;
        url.set_query(issuer.query());
        url
    };

    let mut response = fetch_metadata(&discovery_url, protocol_version)?;

    if metadata_url.is_none()
        && response.status >= 400
        && response.status < 500
        && issuer.path() != "/"
    {
        let root_url = issuer
            .join(&format!("/.well-known/{}", well_known_type.path_prefix()))
            .expect("root OAuth well-known URL is valid");
        response = fetch_metadata(&root_url, protocol_version)?;
    }

    Ok(response)
}

fn build_well_known_path(well_known_type: WellKnownType, pathname: &str) -> String {
    let pathname = pathname.trim_end_matches('/');
    format!("/.well-known/{}{pathname}", well_known_type.path_prefix())
}

fn fetch_metadata(url: &Url, protocol_version: &str) -> McpOAuthResult<OAuthHttpResponse> {
    let mut response = ureq::get(url.as_str())
        .header("MCP-Protocol-Version", protocol_version)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|error| McpOAuthError::new(format!("OAuth metadata fetch failed: {error}")))?;
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().map_err(|error| {
        McpOAuthError::new(format!("OAuth metadata response read failed: {error}"))
    })?;
    Ok(OAuthHttpResponse { status, body })
}

fn token_endpoint_url(
    authorization_server_url: &str,
    metadata: Option<&AuthorizationServerMetadata>,
) -> McpOAuthResult<Url> {
    if let Some(metadata) = metadata {
        Url::parse(metadata.token_endpoint())
            .map_err(|error| McpOAuthError::new(format!("invalid token endpoint URL: {error}")))
    } else {
        Url::parse(authorization_server_url)
            .and_then(|url| url.join("/token"))
            .map_err(|error| McpOAuthError::new(format!("invalid token URL: {error}")))
    }
}

fn validate_grant_type(
    metadata: Option<&AuthorizationServerMetadata>,
    grant_type: &str,
) -> McpOAuthResult<()> {
    if let Some(grant_types) = metadata.and_then(AuthorizationServerMetadata::grant_types_supported)
        && !grant_types.iter().any(|supported| supported == grant_type)
    {
        return Err(McpOAuthError::new(format!(
            "Incompatible auth server: does not support grant type {grant_type}"
        )));
    }
    Ok(())
}

fn supported_auth_methods(metadata: Option<&AuthorizationServerMetadata>) -> &[String] {
    metadata
        .and_then(AuthorizationServerMetadata::token_endpoint_auth_methods_supported)
        .unwrap_or(&[])
}

fn select_client_auth_method(
    client_information: &OAuthClientInformation,
    supported_methods: &[String],
) -> ClientAuthMethod {
    let has_client_secret = client_information.client_secret.is_some();
    if supported_methods.is_empty() {
        return if has_client_secret {
            ClientAuthMethod::ClientSecretPost
        } else {
            ClientAuthMethod::None
        };
    }
    if has_client_secret
        && supported_methods
            .iter()
            .any(|method| method == "client_secret_basic")
    {
        return ClientAuthMethod::ClientSecretBasic;
    }
    if has_client_secret
        && supported_methods
            .iter()
            .any(|method| method == "client_secret_post")
    {
        return ClientAuthMethod::ClientSecretPost;
    }
    if supported_methods.iter().any(|method| method == "none") {
        return ClientAuthMethod::None;
    }
    if has_client_secret {
        ClientAuthMethod::ClientSecretPost
    } else {
        ClientAuthMethod::None
    }
}

fn token_request_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
        ("Accept".to_string(), "application/json".to_string()),
    ])
}

fn apply_client_authentication(
    method: ClientAuthMethod,
    client_information: &OAuthClientInformation,
    headers: &mut BTreeMap<String, String>,
    params: &mut BTreeMap<String, String>,
) -> McpOAuthResult<()> {
    match method {
        ClientAuthMethod::ClientSecretBasic => {
            let client_secret = client_information.client_secret.as_ref().ok_or_else(|| {
                McpOAuthError::new("client_secret_basic authentication requires a client_secret")
            })?;
            let credentials = convert_bytes_to_base64(
                format!("{}:{client_secret}", client_information.client_id).as_bytes(),
            );
            headers.insert("Authorization".to_string(), format!("Basic {credentials}"));
        }
        ClientAuthMethod::ClientSecretPost => {
            params.insert(
                "client_id".to_string(),
                client_information.client_id.clone(),
            );
            if let Some(client_secret) = &client_information.client_secret {
                params.insert("client_secret".to_string(), client_secret.clone());
            }
        }
        ClientAuthMethod::None => {
            params.insert(
                "client_id".to_string(),
                client_information.client_id.clone(),
            );
        }
    }
    Ok(())
}

fn post_form_urlencoded(
    url: &Url,
    headers: &BTreeMap<String, String>,
    params: &BTreeMap<String, String>,
) -> McpOAuthResult<OAuthHttpResponse> {
    let mut body = FormUrlEncodedSerializer::new(String::new());
    for (key, value) in params {
        body.append_pair(key, value);
    }
    let body = body.finish();
    let mut builder = ureq::post(url.as_str());
    for (key, value) in headers {
        builder = builder.header(key.as_str(), value.as_str());
    }
    let mut response = builder
        .config()
        .http_status_as_error(false)
        .build()
        .send(body)
        .map_err(|error| McpOAuthError::new(format!("OAuth token request failed: {error}")))?;
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().map_err(|error| {
        McpOAuthError::new(format!("OAuth token response read failed: {error}"))
    })?;
    Ok(OAuthHttpResponse { status, body })
}

fn post_json(url: &Url, body: &str) -> McpOAuthResult<OAuthHttpResponse> {
    let mut response = ureq::post(url.as_str())
        .header("Content-Type", "application/json")
        .config()
        .http_status_as_error(false)
        .build()
        .send(body)
        .map_err(|error| {
            McpOAuthError::new(format!("OAuth client registration request failed: {error}"))
        })?;
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().map_err(|error| {
        McpOAuthError::new(format!(
            "OAuth client registration response read failed: {error}"
        ))
    })?;
    Ok(OAuthHttpResponse { status, body })
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn origin_string(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let mut origin = format!("{}://{host}", url.scheme());
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Some(origin)
}

fn path_with_trailing_slash(path: &str) -> String {
    if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    }
}

fn set_url_query_param(url: &mut Url, key: &str, value: &str) {
    let pairs = url
        .query_pairs()
        .filter(|(existing_key, _)| existing_key != key)
        .map(|(existing_key, existing_value)| {
            (existing_key.into_owned(), existing_value.into_owned())
        })
        .collect::<Vec<_>>();
    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        for (existing_key, existing_value) in pairs {
            query.append_pair(&existing_key, &existing_value);
        }
        query.append_pair(key, value);
    }
}

fn append_url_query_param(url: &mut Url, key: &str, value: &str) {
    url.query_pairs_mut().append_pair(key, value);
}

fn validate_url_string(value: &str) -> Result<(), String> {
    Url::parse(value)
        .map(|_| ())
        .map_err(|error| format!("URL must be parseable: {error}"))
}

fn deserialize_url_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    validate_url_string(&value).map_err(D::Error::custom)?;
    Ok(value)
}

fn deserialize_optional_url_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    if let Some(value) = &value {
        validate_url_string(value).map_err(D::Error::custom)?;
    }
    Ok(value)
}

fn deserialize_safe_url_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    validate_safe_url(&value).map_err(D::Error::custom)?;
    Ok(value)
}

fn deserialize_optional_safe_url_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    if let Some(value) = &value {
        validate_safe_url(value).map_err(D::Error::custom)?;
    }
    Ok(value)
}

fn deserialize_safe_url_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<String>::deserialize(deserializer)?;
    for value in &values {
        validate_safe_url(value).map_err(D::Error::custom)?;
    }
    Ok(values)
}

fn deserialize_optional_safe_url_vec<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Option::<Vec<String>>::deserialize(deserializer)?;
    if let Some(values) = &values {
        for value in values {
            validate_safe_url(value).map_err(D::Error::custom)?;
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    #[test]
    fn resource_url_from_server_url_removes_fragment_and_preserves_url_parts() {
        assert_eq!(
            resource_url_from_server_url("https://example.com/path#fragment")
                .expect("resource URL")
                .as_str(),
            "https://example.com/path"
        );
        assert_eq!(
            resource_url_from_server_url("https://example.com/path?query=1#fragment")
                .expect("resource URL")
                .as_str(),
            "https://example.com/path?query=1"
        );
        assert_eq!(
            resource_url_from_server_url("https://EXAMPLE.COM/PATH")
                .expect("resource URL")
                .as_str(),
            "https://example.com/PATH"
        );
        assert_eq!(
            resource_url_from_server_url("https://example.com:443/path")
                .expect("resource URL")
                .as_str(),
            "https://example.com/path"
        );
        assert_eq!(
            resource_url_from_server_url("https://example.com:8080/path")
                .expect("resource URL")
                .as_str(),
            "https://example.com:8080/path"
        );
        assert_eq!(
            resource_url_from_server_url("https://example.com?foo=bar&baz=qux")
                .expect("resource URL")
                .as_str(),
            "https://example.com/?foo=bar&baz=qux"
        );
    }

    #[test]
    fn oauth_pkce_challenge_derives_s256_challenge_from_verifier() {
        let challenge = OAuthPkceChallenge::from_code_verifier("test_verifier");

        assert_eq!(challenge.code_verifier, "test_verifier");
        assert_eq!(
            challenge.code_challenge,
            "0Ku4rR8EgR1w3HyHLBCxVLtPsAAks5HOlpmTEt0XhVA"
        );
        assert!(!challenge.code_challenge.contains('='));
        assert!(!challenge.code_challenge.contains('+'));
        assert!(!challenge.code_challenge.contains('/'));
    }

    #[test]
    fn oauth_pkce_challenge_generates_random_url_safe_verifier() {
        let challenge = OAuthPkceChallenge::generate().expect("PKCE generation succeeds");

        assert_eq!(challenge.code_verifier.len(), 43);
        assert_eq!(challenge.code_challenge.len(), 43);
        assert!(
            challenge
                .code_verifier
                .chars()
                .all(|character| character.is_ascii_alphanumeric()
                    || character == '-'
                    || character == '_')
        );
        assert_ne!(challenge.code_verifier, challenge.code_challenge);
    }

    #[test]
    fn resource_url_strip_slash_removes_only_pathless_trailing_slash() {
        assert_eq!(
            resource_url_strip_slash(&Url::parse("https://mcp.example.com").expect("URL")),
            "https://mcp.example.com"
        );
        assert_eq!(
            resource_url_strip_slash(&Url::parse("https://mcp.example.com/").expect("URL")),
            "https://mcp.example.com"
        );
        assert_eq!(
            resource_url_strip_slash(&Url::parse("https://mcp.example.com/path/").expect("URL")),
            "https://mcp.example.com/path/"
        );
        assert_eq!(
            resource_url_strip_slash(&Url::parse("https://mcp.example.com/?q=1").expect("URL")),
            "https://mcp.example.com/?q=1"
        );
    }

    #[test]
    fn check_resource_allowed_matches_origin_and_path_boundaries() {
        assert!(
            check_resource_allowed("https://example.com/path", "https://example.com/path")
                .expect("resource check")
        );
        assert!(
            check_resource_allowed("https://example.com/api/v1", "https://example.com/api")
                .expect("resource check")
        );
        assert!(
            check_resource_allowed("https://example.com/mcp/", "https://example.com/mcp")
                .expect("resource check")
        );
        assert!(
            !check_resource_allowed("https://example.com/path1", "https://example.com/path2")
                .expect("resource check")
        );
        assert!(
            !check_resource_allowed("https://example.com/", "https://example.com/path")
                .expect("resource check")
        );
        assert!(
            !check_resource_allowed("https://example.com/path", "https://example.org/path")
                .expect("resource check")
        );
        assert!(
            !check_resource_allowed("https://example.com:8080/path", "https://example.com/path")
                .expect("resource check")
        );
        assert!(
            !check_resource_allowed("https://example.com/mcpxxxx", "https://example.com/mcp")
                .expect("resource check")
        );
        assert!(
            !check_resource_allowed("https://example.com/folder", "https://example.com/folder/")
                .expect("resource check")
        );
    }

    #[test]
    fn select_resource_url_uses_protected_metadata_when_allowed() {
        let metadata = OAuthProtectedResourceMetadata {
            resource: "https://api.example.com/mcp".to_string(),
            authorization_servers: Some(vec!["https://auth.example.com".to_string()]),
            jwks_uri: None,
            scopes_supported: None,
            bearer_methods_supported: None,
            resource_signing_alg_values_supported: None,
            resource_name: None,
            resource_documentation: None,
            resource_policy_uri: None,
            resource_tos_uri: None,
            tls_client_certificate_bound_access_tokens: None,
            authorization_details_types_supported: None,
            dpop_signing_alg_values_supported: None,
            dpop_bound_access_tokens_required: None,
            extra: JsonObject::new(),
        };

        let resource = select_resource_url("https://api.example.com/mcp/server", Some(&metadata))
            .expect("resource selected")
            .expect("resource metadata is present");

        assert_eq!(resource.as_str(), "https://api.example.com/mcp");
        assert!(
            select_resource_url("https://api.example.com/mcp/server", None)
                .expect("missing metadata is allowed")
                .is_none()
        );
    }

    #[test]
    fn select_resource_url_rejects_mismatched_protected_metadata() {
        let metadata = OAuthProtectedResourceMetadata {
            resource: "https://different.example.com/mcp".to_string(),
            authorization_servers: None,
            jwks_uri: None,
            scopes_supported: None,
            bearer_methods_supported: None,
            resource_signing_alg_values_supported: None,
            resource_name: None,
            resource_documentation: None,
            resource_policy_uri: None,
            resource_tos_uri: None,
            tls_client_certificate_bound_access_tokens: None,
            authorization_details_types_supported: None,
            dpop_signing_alg_values_supported: None,
            dpop_bound_access_tokens_required: None,
            extra: JsonObject::new(),
        };

        let error = select_resource_url("https://api.example.com/mcp/server", Some(&metadata))
            .expect_err("mismatched resource fails");

        assert!(error.message.contains("does not match expected"));
    }

    #[test]
    fn extract_resource_metadata_url_reads_bearer_www_authenticate_parameter() {
        let resource_url = "https://resource.example.com/.well-known/oauth-protected-resource";
        assert_eq!(
            extract_resource_metadata_url(Some(&format!(
                "Bearer realm=\"mcp\", resource_metadata=\"{resource_url}\""
            )))
            .expect("resource metadata URL")
            .as_str(),
            resource_url
        );
        assert!(
            extract_resource_metadata_url(Some(&format!(
                "Basic realm=\"mcp\", resource_metadata=\"{resource_url}\""
            )))
            .is_none()
        );
        assert!(extract_resource_metadata_url(Some("Bearer realm=\"mcp\"")).is_none());
        assert!(
            extract_resource_metadata_url(Some(
                "Bearer realm=\"mcp\", resource_metadata=\"invalid-url\""
            ))
            .is_none()
        );
        assert!(extract_resource_metadata_url(None).is_none());
    }

    #[test]
    fn build_discovery_urls_matches_upstream_priority_order() {
        let root_urls =
            build_discovery_urls("https://auth.example.com").expect("discovery URLs build");
        assert_eq!(
            discovery_url_tuples(&root_urls),
            vec![
                (
                    "https://auth.example.com/.well-known/oauth-authorization-server".to_string(),
                    DiscoveryMetadataType::OAuth,
                ),
                (
                    "https://auth.example.com/.well-known/openid-configuration".to_string(),
                    DiscoveryMetadataType::OpenId,
                ),
            ]
        );

        let path_urls =
            build_discovery_urls("https://auth.example.com/tenant1").expect("discovery URLs build");
        assert_eq!(
            discovery_url_tuples(&path_urls),
            vec![
                (
                    "https://auth.example.com/.well-known/oauth-authorization-server/tenant1"
                        .to_string(),
                    DiscoveryMetadataType::OAuth,
                ),
                (
                    "https://auth.example.com/.well-known/oauth-authorization-server".to_string(),
                    DiscoveryMetadataType::OAuth,
                ),
                (
                    "https://auth.example.com/.well-known/openid-configuration/tenant1".to_string(),
                    DiscoveryMetadataType::OpenId,
                ),
                (
                    "https://auth.example.com/tenant1/.well-known/openid-configuration".to_string(),
                    DiscoveryMetadataType::OpenId,
                ),
            ]
        );
    }

    #[test]
    fn oauth_metadata_safe_url_deserialization_rejects_dangerous_schemes() {
        let error = serde_json::from_value::<OAuthMetadata>(json!({
            "issuer": "https://auth.example.com",
            "authorization_endpoint": "javascript:alert(1)",
            "token_endpoint": "https://auth.example.com/token",
            "response_types_supported": ["code"],
            "code_challenge_methods_supported": ["S256"]
        }))
        .expect_err("dangerous URL rejected");

        assert!(
            error
                .to_string()
                .contains("URL cannot use javascript:, data:, or vbscript: scheme")
        );
    }

    #[test]
    fn discover_oauth_protected_resource_metadata_uses_path_query_and_protocol_header() {
        let server = LocalOAuthServer::new(vec![LocalOAuthResponse::json(json!({
            "resource": "https://resource.example.com/path/name",
            "authorization_servers": ["https://auth.example.com"],
            "resource_name": "Example resource",
            "vendor_extension": true
        }))]);

        let metadata = discover_oauth_protected_resource_metadata(
            format!("{}/path/name?param=value", server.url()),
            OAuthProtectedResourceMetadataDiscoveryOptions::default(),
        )
        .expect("resource metadata discovered");

        assert_eq!(metadata.resource, "https://resource.example.com/path/name");
        assert_eq!(metadata.extra.get("vendor_extension"), Some(&json!(true)));
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].path,
            "/.well-known/oauth-protected-resource/path/name?param=value"
        );
        assert_eq!(
            requests[0].headers.get("mcp-protocol-version"),
            Some(&LATEST_PROTOCOL_VERSION.to_string())
        );
    }

    #[test]
    fn discover_oauth_protected_resource_metadata_falls_back_to_root_on_path_4xx() {
        let server = LocalOAuthServer::new(vec![
            LocalOAuthResponse::empty(404),
            LocalOAuthResponse::json(json!({
                "resource": "https://resource.example.com",
                "authorization_servers": ["https://auth.example.com"]
            })),
        ]);

        let metadata = discover_oauth_protected_resource_metadata(
            format!("{}/deep/path", server.url()),
            OAuthProtectedResourceMetadataDiscoveryOptions::default(),
        )
        .expect("resource metadata discovered");

        assert_eq!(metadata.resource, "https://resource.example.com");
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].path,
            "/.well-known/oauth-protected-resource/deep/path"
        );
        assert_eq!(requests[1].path, "/.well-known/oauth-protected-resource");
    }

    #[test]
    fn discover_oauth_protected_resource_metadata_does_not_fallback_for_explicit_metadata_url() {
        let server = LocalOAuthServer::new(vec![LocalOAuthResponse::empty(404)]);
        let explicit_url = format!("{}/custom/metadata", server.url());

        let error = discover_oauth_protected_resource_metadata(
            format!("{}/deep/path", server.url()),
            OAuthProtectedResourceMetadataDiscoveryOptions::default()
                .with_resource_metadata_url(explicit_url),
        )
        .expect_err("404 metadata is reported");

        assert_eq!(
            error.message,
            "Resource server does not implement OAuth 2.0 Protected Resource Metadata."
        );
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/custom/metadata");
    }

    #[test]
    fn discover_authorization_server_metadata_tries_urls_in_order() {
        let server = LocalOAuthServer::new(vec![
            LocalOAuthResponse::empty(404),
            LocalOAuthResponse::json(json!({
                "issuer": "https://auth.example.com",
                "authorization_endpoint": "https://auth.example.com/authorize",
                "token_endpoint": "https://auth.example.com/token",
                "registration_endpoint": "https://auth.example.com/register",
                "response_types_supported": ["code"],
                "code_challenge_methods_supported": ["S256"]
            })),
        ]);

        let metadata = discover_authorization_server_metadata(
            format!("{}/tenant1", server.url()),
            AuthorizationServerMetadataDiscoveryOptions::default(),
        )
        .expect("authorization metadata discovery succeeds")
        .expect("metadata exists");

        assert!(matches!(metadata, AuthorizationServerMetadata::OAuth(_)));
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].path,
            "/.well-known/oauth-authorization-server/tenant1"
        );
        assert_eq!(requests[1].path, "/.well-known/oauth-authorization-server");
    }

    #[test]
    fn discover_authorization_server_metadata_validates_oidc_s256_support() {
        let server = LocalOAuthServer::new(vec![
            LocalOAuthResponse::empty(404),
            LocalOAuthResponse::json(json!({
                "issuer": "https://auth.example.com",
                "authorization_endpoint": "https://auth.example.com/authorize",
                "token_endpoint": "https://auth.example.com/token",
                "jwks_uri": "https://auth.example.com/jwks",
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["RS256"],
                "response_types_supported": ["code"],
                "code_challenge_methods_supported": ["plain"]
            })),
        ]);

        let error = discover_authorization_server_metadata(
            server.url(),
            AuthorizationServerMetadataDiscoveryOptions::default(),
        )
        .expect_err("OIDC without S256 fails");

        assert!(
            error.message.contains(
                "does not support S256 code challenge method required by MCP specification"
            )
        );
    }

    #[test]
    fn discover_authorization_server_metadata_returns_none_when_all_endpoints_are_4xx() {
        let server = LocalOAuthServer::new(vec![
            LocalOAuthResponse::empty(404),
            LocalOAuthResponse::empty(404),
        ]);

        let metadata = discover_authorization_server_metadata(
            server.url(),
            AuthorizationServerMetadataDiscoveryOptions::default(),
        )
        .expect("all 4xx is not fatal");

        assert!(metadata.is_none());
        assert_eq!(server.requests().len(), 2);
    }

    #[test]
    fn start_authorization_builds_pkce_resource_scope_state_and_prompt_params() {
        let result = start_authorization(
            "https://auth.example.com",
            StartAuthorizationOptions::new(
                OAuthClientInformation::new("client123").with_client_secret("secret123"),
                "http://localhost:3000/callback",
                OAuthPkceChallenge::new("test_verifier", "test_challenge"),
            )
            .with_scope("read write profile offline_access")
            .with_state("foobar")
            .with_resource(Url::parse("https://api.example.com/mcp-server").expect("URL")),
        )
        .expect("authorization starts");

        assert_eq!(result.authorization_url.scheme(), "https");
        assert_eq!(
            result.authorization_url.host_str(),
            Some("auth.example.com")
        );
        assert_eq!(result.authorization_url.path(), "/authorize");
        assert_eq!(
            query_param(&result.authorization_url, "response_type").as_deref(),
            Some("code")
        );
        assert_eq!(
            query_param(&result.authorization_url, "code_challenge").as_deref(),
            Some("test_challenge")
        );
        assert_eq!(
            query_param(&result.authorization_url, "code_challenge_method").as_deref(),
            Some("S256")
        );
        assert_eq!(
            query_param(&result.authorization_url, "redirect_uri").as_deref(),
            Some("http://localhost:3000/callback")
        );
        assert_eq!(
            query_param(&result.authorization_url, "resource").as_deref(),
            Some("https://api.example.com/mcp-server")
        );
        assert_eq!(
            query_param(&result.authorization_url, "scope").as_deref(),
            Some("read write profile offline_access")
        );
        assert_eq!(
            query_param(&result.authorization_url, "state").as_deref(),
            Some("foobar")
        );
        assert_eq!(
            query_param(&result.authorization_url, "prompt").as_deref(),
            Some("consent")
        );
        assert_eq!(result.code_verifier, "test_verifier");
    }

    #[test]
    fn start_authorization_can_generate_pkce_material() {
        let result = start_authorization(
            "https://auth.example.com",
            StartAuthorizationOptions::with_generated_pkce(
                OAuthClientInformation::new("client123"),
                "http://localhost:3000/callback",
            )
            .expect("generated PKCE options"),
        )
        .expect("authorization URL builds");

        assert_eq!(result.code_verifier.len(), 43);
        assert_eq!(
            query_param(&result.authorization_url, "code_challenge_method").as_deref(),
            Some("S256")
        );
        assert_eq!(
            query_param(&result.authorization_url, "redirect_uri").as_deref(),
            Some("http://localhost:3000/callback")
        );
        let code_challenge =
            query_param(&result.authorization_url, "code_challenge").expect("challenge query");
        assert_eq!(code_challenge.len(), 43);
        assert_ne!(code_challenge, result.code_verifier);
    }

    #[test]
    fn start_authorization_uses_metadata_endpoint_and_validates_capabilities() {
        let metadata = OAuthMetadata {
            issuer: "https://auth.example.com".to_string(),
            authorization_endpoint: "https://auth.example.com/auth".to_string(),
            token_endpoint: "https://auth.example.com/token".to_string(),
            registration_endpoint: None,
            scopes_supported: None,
            response_types_supported: vec!["code".to_string()],
            grant_types_supported: None,
            code_challenge_methods_supported: vec!["S256".to_string()],
            token_endpoint_auth_methods_supported: None,
            token_endpoint_auth_signing_alg_values_supported: None,
            extra: JsonObject::new(),
        };

        let result = start_authorization(
            "https://ignored.example.com",
            StartAuthorizationOptions::new(
                OAuthClientInformation::new("client123"),
                "http://localhost:3000/callback",
                OAuthPkceChallenge::new("verifier", "challenge"),
            )
            .with_metadata(metadata.clone()),
        )
        .expect("authorization starts");
        assert_eq!(result.authorization_url.path(), "/auth");

        let response_type_error = start_authorization(
            "https://auth.example.com",
            StartAuthorizationOptions::new(
                OAuthClientInformation::new("client123"),
                "http://localhost:3000/callback",
                OAuthPkceChallenge::new("verifier", "challenge"),
            )
            .with_metadata(OAuthMetadata {
                response_types_supported: vec!["token".to_string()],
                ..metadata.clone()
            }),
        )
        .expect_err("unsupported response type fails");
        assert!(
            response_type_error
                .message
                .contains("does not support response type code")
        );

        let pkce_error = start_authorization(
            "https://auth.example.com",
            StartAuthorizationOptions::new(
                OAuthClientInformation::new("client123"),
                "http://localhost:3000/callback",
                OAuthPkceChallenge::new("verifier", "challenge"),
            )
            .with_metadata(OAuthMetadata {
                code_challenge_methods_supported: vec!["plain".to_string()],
                ..metadata
            }),
        )
        .expect_err("unsupported PKCE method fails");
        assert!(
            pkce_error
                .message
                .contains("does not support code challenge method S256")
        );
    }

    #[test]
    fn start_authorization_strips_pathless_resource_trailing_slash() {
        let result = start_authorization(
            "https://auth.example.com",
            StartAuthorizationOptions::new(
                OAuthClientInformation::new("client123"),
                "http://localhost:3000/callback",
                OAuthPkceChallenge::new("verifier", "challenge"),
            )
            .with_resource(Url::parse("https://mcp.example.com").expect("URL")),
        )
        .expect("authorization starts");

        assert_eq!(
            query_param(&result.authorization_url, "resource").as_deref(),
            Some("https://mcp.example.com")
        );
    }

    #[test]
    fn parse_oauth_error_response_reads_standard_error_body() {
        let error = parse_oauth_error_response(
            Some(400),
            &json!({
                "error": "invalid_grant",
                "error_description": "Token exchange failed",
                "error_uri": "https://auth.example.com/errors/invalid-grant"
            })
            .to_string(),
        );

        assert_eq!(error.message, "Token exchange failed");
        assert_eq!(error.error_code.as_deref(), Some("invalid_grant"));
        assert_eq!(
            error.error_uri.as_deref(),
            Some("https://auth.example.com/errors/invalid-grant")
        );

        let fallback = parse_oauth_error_response(Some(500), "not-json");
        assert!(fallback.message.contains("HTTP 500"));
        assert!(fallback.message.contains("Invalid OAuth error response"));
        assert_eq!(fallback.error_code, None);
    }

    #[test]
    fn exchange_authorization_posts_code_verifier_client_secret_and_resource() {
        let server = LocalOAuthServer::new(vec![LocalOAuthResponse::json(json!({
            "access_token": "access123",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "refresh123"
        }))]);

        let tokens = exchange_authorization(
            server.url(),
            ExchangeAuthorizationOptions::new(
                OAuthClientInformation::new("client123").with_client_secret("secret123"),
                "code123",
                "verifier123",
                "http://localhost:3000/callback",
            )
            .with_resource(Url::parse("https://api.example.com/mcp-server").expect("URL")),
        )
        .expect("authorization code exchanges");

        assert_eq!(tokens.access_token, "access123");
        assert_eq!(tokens.refresh_token.as_deref(), Some("refresh123"));
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/token");
        assert_eq!(
            requests[0].headers.get("content-type"),
            Some(&"application/x-www-form-urlencoded".to_string())
        );
        assert_eq!(
            requests[0].headers.get("accept"),
            Some(&"application/json".to_string())
        );
        let body = form_body(&requests[0].body);
        assert_eq!(
            body.get("grant_type").map(String::as_str),
            Some("authorization_code")
        );
        assert_eq!(body.get("code").map(String::as_str), Some("code123"));
        assert_eq!(
            body.get("code_verifier").map(String::as_str),
            Some("verifier123")
        );
        assert_eq!(body.get("client_id").map(String::as_str), Some("client123"));
        assert_eq!(
            body.get("client_secret").map(String::as_str),
            Some("secret123")
        );
        assert_eq!(
            body.get("redirect_uri").map(String::as_str),
            Some("http://localhost:3000/callback")
        );
        assert_eq!(
            body.get("resource").map(String::as_str),
            Some("https://api.example.com/mcp-server")
        );
    }

    #[test]
    fn exchange_authorization_uses_basic_auth_when_metadata_prefers_it() {
        let server = LocalOAuthServer::new(vec![LocalOAuthResponse::json(json!({
            "access_token": "access123",
            "token_type": "Bearer"
        }))]);
        let metadata = oauth_metadata()
            .with_token_endpoint(format!("{}/custom-token", server.url()))
            .with_grant_types(["authorization_code"])
            .with_auth_methods(["client_secret_basic"]);

        let tokens = exchange_authorization(
            "https://auth.example.com",
            ExchangeAuthorizationOptions::new(
                OAuthClientInformation::new("client123").with_client_secret("secret123"),
                "code123",
                "verifier123",
                "http://localhost:3000/callback",
            )
            .with_metadata(metadata),
        )
        .expect("authorization code exchanges");

        assert_eq!(tokens.access_token, "access123");
        let requests = server.requests();
        assert_eq!(requests[0].path, "/custom-token");
        assert_eq!(
            requests[0].headers.get("authorization"),
            Some(&"Basic Y2xpZW50MTIzOnNlY3JldDEyMw==".to_string())
        );
        let body = form_body(&requests[0].body);
        assert_eq!(body.get("client_id"), None);
        assert_eq!(body.get("client_secret"), None);
    }

    #[test]
    fn exchange_authorization_validates_grant_type_and_token_response() {
        let grant_error = exchange_authorization(
            "https://auth.example.com",
            ExchangeAuthorizationOptions::new(
                OAuthClientInformation::new("client123"),
                "code123",
                "verifier123",
                "http://localhost:3000/callback",
            )
            .with_metadata(oauth_metadata().with_grant_types(["refresh_token"])),
        )
        .expect_err("unsupported grant type fails");
        assert!(
            grant_error
                .message
                .contains("does not support grant type authorization_code")
        );

        let server = LocalOAuthServer::new(vec![LocalOAuthResponse::json(json!({
            "access_token": "access123"
        }))]);
        let parse_error = exchange_authorization(
            server.url(),
            ExchangeAuthorizationOptions::new(
                OAuthClientInformation::new("client123"),
                "code123",
                "verifier123",
                "http://localhost:3000/callback",
            ),
        )
        .expect_err("invalid token response fails");
        assert!(
            parse_error
                .message
                .contains("Failed to parse OAuth token response")
        );
    }

    #[test]
    fn refresh_authorization_posts_refresh_token_and_preserves_missing_replacement() {
        let server = LocalOAuthServer::new(vec![LocalOAuthResponse::json(json!({
            "access_token": "newaccess123",
            "token_type": "Bearer",
            "expires_in": 3600
        }))]);

        let tokens = refresh_authorization(
            server.url(),
            RefreshAuthorizationOptions::new(
                OAuthClientInformation::new("client123").with_client_secret("secret123"),
                "refresh123",
            )
            .with_resource(Url::parse("https://mcp.example.com").expect("URL")),
        )
        .expect("refresh succeeds");

        assert_eq!(tokens.access_token, "newaccess123");
        assert_eq!(tokens.refresh_token.as_deref(), Some("refresh123"));
        let requests = server.requests();
        assert_eq!(requests[0].path, "/token");
        let body = form_body(&requests[0].body);
        assert_eq!(
            body.get("grant_type").map(String::as_str),
            Some("refresh_token")
        );
        assert_eq!(
            body.get("refresh_token").map(String::as_str),
            Some("refresh123")
        );
        assert_eq!(
            body.get("resource").map(String::as_str),
            Some("https://mcp.example.com")
        );
    }

    #[test]
    fn refresh_authorization_reports_oauth_error_response() {
        let server = LocalOAuthServer::new(vec![LocalOAuthResponse::new(
            400,
            [("content-type", "application/json")],
            json!({
                "error": "server_error",
                "error_description": "Token refresh failed"
            })
            .to_string(),
        )]);

        let error = refresh_authorization(
            server.url(),
            RefreshAuthorizationOptions::new(
                OAuthClientInformation::new("client123"),
                "refresh123",
            ),
        )
        .expect_err("refresh error is reported");

        assert_eq!(error.message, "Token refresh failed");
        assert_eq!(error.error_code.as_deref(), Some("server_error"));
    }

    #[test]
    fn register_client_posts_metadata_and_parses_full_information() {
        let server = LocalOAuthServer::new(vec![LocalOAuthResponse::json(json!({
            "client_id": "client123",
            "client_secret": "secret123",
            "client_id_issued_at": 1612137600_u64,
            "client_secret_expires_at": 1612224000_u64,
            "redirect_uris": ["http://localhost:3000/callback"],
            "client_name": "Test Client"
        }))]);
        let client_metadata =
            OAuthClientMetadata::new(vec!["http://localhost:3000/callback".to_string()])
                .with_client_name("Test Client");

        let client = register_client(server.url(), RegisterClientOptions::new(client_metadata))
            .expect("client registers");

        assert_eq!(client.information.client_id, "client123");
        assert_eq!(
            client.information.client_secret.as_deref(),
            Some("secret123")
        );
        assert_eq!(
            client.metadata.redirect_uris,
            vec!["http://localhost:3000/callback".to_string()]
        );
        assert_eq!(client.metadata.client_name.as_deref(), Some("Test Client"));
        let requests = server.requests();
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/register");
        assert_eq!(
            requests[0].headers.get("content-type"),
            Some(&"application/json".to_string())
        );
        let body = json_body(&requests[0].body);
        assert_eq!(body["redirect_uris"][0], "http://localhost:3000/callback");
        assert_eq!(body["client_name"], "Test Client");
    }

    #[test]
    fn register_client_uses_metadata_endpoint_and_requires_registration_support() {
        let server = LocalOAuthServer::new(vec![LocalOAuthResponse::json(json!({
            "client_id": "client123",
            "redirect_uris": ["http://localhost:3000/callback"]
        }))]);
        let metadata = oauth_metadata()
            .with_registration_endpoint(format!("{}/register-client", server.url()));

        let client = register_client(
            "https://auth.example.com",
            RegisterClientOptions::new(OAuthClientMetadata::new(vec![
                "http://localhost:3000/callback".to_string(),
            ]))
            .with_metadata(metadata),
        )
        .expect("client registers through metadata endpoint");

        assert_eq!(client.information.client_id, "client123");
        assert_eq!(server.requests()[0].path, "/register-client");

        let error = register_client(
            "https://auth.example.com",
            RegisterClientOptions::new(OAuthClientMetadata::new(vec![
                "http://localhost:3000/callback".to_string(),
            ]))
            .with_metadata(oauth_metadata()),
        )
        .expect_err("missing registration endpoint fails");
        assert!(
            error
                .message
                .contains("does not support dynamic client registration")
        );
    }

    trait OAuthMetadataTestExt {
        fn with_token_endpoint(self, token_endpoint: String) -> Self;
        fn with_registration_endpoint(self, registration_endpoint: String) -> Self;
        fn with_grant_types<const N: usize>(self, grant_types: [&str; N]) -> Self;
        fn with_auth_methods<const N: usize>(self, auth_methods: [&str; N]) -> Self;
    }

    impl OAuthMetadataTestExt for OAuthMetadata {
        fn with_token_endpoint(mut self, token_endpoint: String) -> Self {
            self.token_endpoint = token_endpoint;
            self
        }

        fn with_registration_endpoint(mut self, registration_endpoint: String) -> Self {
            self.registration_endpoint = Some(registration_endpoint);
            self
        }

        fn with_grant_types<const N: usize>(mut self, grant_types: [&str; N]) -> Self {
            self.grant_types_supported =
                Some(grant_types.into_iter().map(str::to_string).collect());
            self
        }

        fn with_auth_methods<const N: usize>(mut self, auth_methods: [&str; N]) -> Self {
            self.token_endpoint_auth_methods_supported =
                Some(auth_methods.into_iter().map(str::to_string).collect());
            self
        }
    }

    fn oauth_metadata() -> OAuthMetadata {
        OAuthMetadata {
            issuer: "https://auth.example.com".to_string(),
            authorization_endpoint: "https://auth.example.com/authorize".to_string(),
            token_endpoint: "https://auth.example.com/token".to_string(),
            registration_endpoint: None,
            scopes_supported: None,
            response_types_supported: vec!["code".to_string()],
            grant_types_supported: None,
            code_challenge_methods_supported: vec!["S256".to_string()],
            token_endpoint_auth_methods_supported: None,
            token_endpoint_auth_signing_alg_values_supported: None,
            extra: JsonObject::new(),
        }
    }

    fn discovery_url_tuples(urls: &[DiscoveryUrl]) -> Vec<(String, DiscoveryMetadataType)> {
        urls.iter()
            .map(|url| (url.url.as_str().to_string(), url.metadata_type))
            .collect()
    }

    fn query_param(url: &Url, key: &str) -> Option<String> {
        url.query_pairs()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.into_owned())
    }

    #[derive(Clone, Debug)]
    struct LocalOAuthRequest {
        method: String,
        path: String,
        headers: BTreeMap<String, String>,
        body: String,
    }

    struct LocalOAuthResponse {
        status: u16,
        headers: BTreeMap<String, String>,
        body: String,
    }

    impl LocalOAuthResponse {
        fn new<K, V, I>(status: u16, headers: I, body: impl Into<String>) -> Self
        where
            I: IntoIterator<Item = (K, V)>,
            K: Into<String>,
            V: Into<String>,
        {
            Self {
                status,
                headers: headers
                    .into_iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect(),
                body: body.into(),
            }
        }

        fn json(body: JsonValue) -> Self {
            Self::new(
                200,
                [("content-type", "application/json")],
                body.to_string(),
            )
        }

        fn empty(status: u16) -> Self {
            Self::new(status, [("content-type", "text/plain")], "")
        }
    }

    struct LocalOAuthServer {
        url: String,
        requests: Arc<Mutex<Vec<LocalOAuthRequest>>>,
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl LocalOAuthServer {
        fn new(responses: Vec<LocalOAuthResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind local OAuth server");
            listener
                .set_nonblocking(true)
                .expect("set local OAuth server nonblocking");
            let url = format!("http://{}", listener.local_addr().expect("local address"));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
            let stop = Arc::new(AtomicBool::new(false));
            let handle_requests = Arc::clone(&requests);
            let handle_responses = Arc::clone(&responses);
            let handle_stop = Arc::clone(&stop);

            let handle = thread::spawn(move || {
                while !handle_stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            handle_local_oauth_connection(
                                stream,
                                &handle_requests,
                                &handle_responses,
                            );
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });

            Self {
                url,
                requests,
                stop,
                handle: Some(handle),
            }
        }

        fn url(&self) -> String {
            self.url.clone()
        }

        fn requests(&self) -> Vec<LocalOAuthRequest> {
            self.requests.lock().expect("local requests lock").clone()
        }
    }

    impl Drop for LocalOAuthServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(
                self.url
                    .strip_prefix("http://")
                    .expect("local server URL has prefix"),
            );
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn handle_local_oauth_connection(
        mut stream: TcpStream,
        requests: &Arc<Mutex<Vec<LocalOAuthRequest>>>,
        responses: &Arc<Mutex<VecDeque<LocalOAuthResponse>>>,
    ) {
        stream
            .set_nonblocking(false)
            .expect("local test stream is blocking");
        let mut buffer = Vec::new();
        let mut chunk = [0; 1024];
        let request = loop {
            let read = stream.read(&mut chunk).expect("read request");
            if read == 0 {
                return;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(request) = parse_local_oauth_request(&buffer) {
                break request;
            }
        };
        requests.lock().expect("local requests lock").push(request);

        let response = responses
            .lock()
            .expect("local responses lock")
            .pop_front()
            .unwrap_or_else(|| LocalOAuthResponse::empty(404));
        let body = response.body;
        let mut response_text = format!(
            "HTTP/1.1 {} OK\r\ncontent-length: {}\r\nconnection: close\r\n",
            response.status,
            body.len()
        );
        for (key, value) in response.headers {
            response_text.push_str(&format!("{key}: {value}\r\n"));
        }
        response_text.push_str("\r\n");
        response_text.push_str(&body);
        stream
            .write_all(response_text.as_bytes())
            .expect("write response");
    }

    fn parse_local_oauth_request(buffer: &[u8]) -> Option<LocalOAuthRequest> {
        let header_end = buffer.windows(4).position(|window| window == b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(&buffer[..header_end]);
        let mut lines = head.lines();
        let request_line = lines.next()?;
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next()?.to_string();
        let path = request_parts.next()?.to_string();
        let mut headers = BTreeMap::new();
        for line in lines {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
        let content_length = headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let body_start = header_end + 4;
        if buffer.len() < body_start + content_length {
            return None;
        }
        let body =
            String::from_utf8_lossy(&buffer[body_start..body_start + content_length]).to_string();
        Some(LocalOAuthRequest {
            method,
            path,
            headers,
            body,
        })
    }

    fn form_body(body: &str) -> BTreeMap<String, String> {
        url::form_urlencoded::parse(body.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect()
    }

    fn json_body(body: &str) -> JsonValue {
        serde_json::from_str(body).expect("body is JSON")
    }
}
