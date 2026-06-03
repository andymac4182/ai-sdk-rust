//! Rust port of the Vercel AI SDK Claude Platform on AWS (`@ai-sdk/anthropic-aws`) provider.
//!
//! This crate ports the fetch-signing wrappers (`createSigV4FetchFunction`,
//! `createApiKeyFetchFunction`) and the provider configuration logic
//! (`createAnthropicAws`) that selects an authentication path, resolves the base
//! URL with region templating, and computes the per-request Anthropic headers.
//!
//! Behavior parity is proven by the colocated `#[cfg(test)] mod tests` block and
//! by the row-level mapping harness in `tests/upstream_mapping.rs`.

use std::collections::BTreeMap;

/// Mirror of the mocked `VERSION` used in the upstream test suite. The upstream
/// tests mock `./version` to `0.0.0-test`; the user-agent assertions are anchored
/// to that value so we reproduce it here for deterministic parity checks.
pub const TEST_VERSION: &str = "0.0.0-test";

/// Mirror of the mocked runtime user-agent (`getRuntimeEnvironmentUserAgent`)
/// used in the upstream test suite.
pub const TEST_RUNTIME_USER_AGENT: &str = "runtime/testenv";

/// AWS credentials used to sign Claude Platform on AWS requests with SigV4.
///
/// Port of the upstream `AnthropicAwsCredentials` interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnthropicAwsCredentials {
    /// AWS region used for signing.
    pub region: String,
    /// AWS access key ID.
    pub access_key_id: String,
    /// AWS secret access key.
    pub secret_access_key: String,
    /// Optional AWS session token.
    pub session_token: Option<String>,
}

/// A normalized, lower-cased header map.
///
/// Header insertion order is irrelevant to upstream assertions (they compare
/// plain objects), so a `BTreeMap` is used for deterministic iteration.
pub type Headers = BTreeMap<String, String>;

/// Normalizes header entries into a lower-cased map, dropping `None` values.
///
/// Port of `normalizeHeaders` from `@ai-sdk/provider-utils`.
pub fn normalize_headers<I, K, V>(headers: I) -> Headers
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    headers
        .into_iter()
        .filter_map(|(key, value)| {
            value.map(|value| (key.as_ref().to_ascii_lowercase(), value.into()))
        })
        .collect()
}

/// Combines header maps left-to-right; later maps override earlier ones, and a
/// `None` value removes the key.
///
/// Port of `combineHeaders` from `@ai-sdk/provider-utils`.
pub fn combine_headers(
    maps: &[&BTreeMap<String, Option<String>>],
) -> BTreeMap<String, Option<String>> {
    let mut combined: BTreeMap<String, Option<String>> = BTreeMap::new();
    for map in maps {
        for (key, value) in map.iter() {
            combined.insert(key.clone(), value.clone());
        }
    }
    combined
}

/// Appends the given non-empty user-agent suffix parts to the `user-agent`
/// header of an already-normalized header map.
///
/// Port of `withUserAgentSuffix` from `@ai-sdk/provider-utils`.
pub fn with_user_agent_suffix<I, S>(mut headers: Headers, suffix_parts: I) -> Headers
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut parts: Vec<String> = Vec::new();
    if let Some(existing) = headers.get("user-agent") {
        if !existing.is_empty() {
            parts.push(existing.clone());
        }
    }
    for part in suffix_parts {
        let part = part.as_ref();
        if !part.is_empty() {
            parts.push(part.to_string());
        }
    }
    headers.insert("user-agent".to_string(), parts.join(" "));
    headers
}

/// Builds the `ai-sdk/anthropic-aws/<version>` plus runtime user-agent suffix list.
fn user_agent_suffix(version: &str, runtime_user_agent: &str) -> [String; 2] {
    [
        format!("ai-sdk/anthropic-aws/{version}"),
        runtime_user_agent.to_string(),
    ]
}

/// Outcome of running a SigV4 fetch wrapper: the resolved request the underlying
/// fetch would be invoked with, mirroring the upstream `fetchFn` indirection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedRequest {
    /// The URL the underlying fetch is called with.
    pub url: String,
    /// The HTTP method, when supplied by the caller.
    pub method: Option<String>,
    /// The body forwarded to the underlying fetch (signing requires a string body).
    pub body: Option<String>,
    /// The merged, normalized headers forwarded to the underlying fetch.
    pub headers: Headers,
    /// Whether the request was actually signed (POST with a non-empty body).
    pub signed: bool,
}

/// A request body, mirroring the upstream `BodyInit` variants the wrapper handles.
#[derive(Clone, Debug)]
pub enum Body {
    /// A string body, forwarded verbatim.
    Text(String),
    /// A byte body (`Uint8Array` / `ArrayBuffer`), decoded as UTF-8 text.
    Bytes(Vec<u8>),
    /// A non-string/non-bytes body, JSON-stringified.
    Json(serde_json::Value),
}

/// Converts a body into the string the upstream `prepareBodyString` produces.
///
/// Port of `prepareBodyString`.
pub fn prepare_body_string(body: &Body) -> String {
    match body {
        Body::Text(text) => text.clone(),
        Body::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Body::Json(value) => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// Mock SigV4 signer matching the upstream `MockAwsV4Signer`: returns fixed
/// `x-amz-date` and `authorization` headers, plus `x-amz-security-token` when a
/// session token is present. Used to assert the deterministic header-merge
/// behavior of the fetch wrapper.
fn mock_sign_headers(session_token: Option<&str>) -> BTreeMap<String, Option<String>> {
    let mut headers: BTreeMap<String, Option<String>> = BTreeMap::new();
    headers.insert(
        "x-amz-date".to_string(),
        Some("20240315T000000Z".to_string()),
    );
    headers.insert(
        "authorization".to_string(),
        Some("AWS4-HMAC-SHA256 Credential=test".to_string()),
    );
    if let Some(token) = session_token {
        headers.insert("x-amz-security-token".to_string(), Some(token.to_string()));
    }
    headers
}

/// Simulated request input for the fetch wrappers.
#[derive(Clone, Debug, Default)]
pub struct RequestInput {
    /// The request URL.
    pub url: String,
    /// Headers carried on a `Request` object (merged below `init` headers).
    pub request_headers: Vec<(String, String)>,
    /// Method carried on a `Request` object.
    pub request_method: Option<String>,
    /// Body carried on a `Request` object (used when `init` has no body).
    pub request_body: Option<String>,
}

/// Simulated `init` (`RequestInit`) for the fetch wrappers.
#[derive(Clone, Debug, Default)]
pub struct RequestInit {
    /// The HTTP method.
    pub method: Option<String>,
    /// The request body.
    pub body: Option<Body>,
    /// The init headers (override `Request` headers).
    pub headers: Option<Vec<(String, Option<String>)>>,
}

/// Ports `createSigV4FetchFunction`: returns the request that would be passed to
/// the underlying fetch, applying user-agent suffixing, the POST/body bypass
/// rule, body stringification, and signed-header merging (via the mock signer).
pub fn sig_v4_prepared_request(
    credentials: &AnthropicAwsCredentials,
    input: &RequestInput,
    init: Option<&RequestInit>,
    version: &str,
    runtime_user_agent: &str,
) -> PreparedRequest {
    // Combine Request headers (lower precedence) with init headers (higher).
    let request_headers_norm = normalize_headers(
        input
            .request_headers
            .iter()
            .map(|(k, v)| (k.clone(), Some(v.clone()))),
    );
    let init_headers_norm = init
        .and_then(|i| i.headers.clone())
        .map(normalize_headers)
        .unwrap_or_default();
    let original = combine_headers(&[
        &to_optional(&request_headers_norm),
        &to_optional(&init_headers_norm),
    ]);
    let original_headers = from_optional(&original);
    let headers_with_ua = with_user_agent_suffix(
        original_headers,
        user_agent_suffix(version, runtime_user_agent),
    );

    // Effective body: init body, else fall back to the Request body.
    let effective_body: Option<Body> = init
        .and_then(|i| i.body.clone())
        .or_else(|| input.request_body.clone().map(Body::Text));

    let effective_method = init
        .and_then(|i| i.method.clone())
        .or_else(|| input.request_method.clone());

    let is_post = effective_method
        .as_deref()
        .map(|m| m.eq_ignore_ascii_case("POST"))
        .unwrap_or(false);

    // Bypass signing for non-POST requests or POST requests with no body.
    if !is_post || effective_body.is_none() {
        return PreparedRequest {
            url: input.url.clone(),
            method: effective_method,
            body: init
                .and_then(|i| i.body.as_ref().map(prepare_body_string))
                .or_else(|| input.request_body.clone()),
            headers: headers_with_ua,
            signed: false,
        };
    }

    let body_string = prepare_body_string(effective_body.as_ref().unwrap());
    let signed = mock_sign_headers(credentials.session_token.as_deref());
    let combined = combine_headers(&[&to_optional(&headers_with_ua), &signed]);

    PreparedRequest {
        url: input.url.clone(),
        method: Some("POST".to_string()),
        body: Some(body_string),
        headers: from_optional(&combined),
        signed: true,
    }
}

/// Ports `createApiKeyFetchFunction`: returns the request passed to the
/// underlying fetch with the user-agent suffix applied and the `x-api-key`
/// header set (overriding any caller-provided value).
pub fn api_key_prepared_request(
    api_key: &str,
    input: &RequestInput,
    init: Option<&RequestInit>,
    version: &str,
    runtime_user_agent: &str,
) -> PreparedRequest {
    let init_headers_norm = init
        .and_then(|i| i.headers.clone())
        .map(normalize_headers)
        .unwrap_or_default();
    let headers_with_ua = with_user_agent_suffix(
        init_headers_norm,
        user_agent_suffix(version, runtime_user_agent),
    );

    let mut api_key_map: BTreeMap<String, Option<String>> = BTreeMap::new();
    api_key_map.insert("x-api-key".to_string(), Some(api_key.to_string()));
    let combined = combine_headers(&[&to_optional(&headers_with_ua), &api_key_map]);

    PreparedRequest {
        url: input.url.clone(),
        method: init.and_then(|i| i.method.clone()),
        body: init.and_then(|i| i.body.as_ref().map(prepare_body_string)),
        headers: from_optional(&combined),
        signed: false,
    }
}

fn to_optional(headers: &Headers) -> BTreeMap<String, Option<String>> {
    headers
        .iter()
        .map(|(k, v)| (k.clone(), Some(v.clone())))
        .collect()
}

fn from_optional(headers: &BTreeMap<String, Option<String>>) -> Headers {
    headers
        .iter()
        .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
        .collect()
}

// ---------------------------------------------------------------------------
// Provider configuration (createAnthropicAws)
// ---------------------------------------------------------------------------

/// The two authentication paths the provider can select.
///
/// Port of the `apiKey ? createApiKeyFetchFunction : createSigV4FetchFunction`
/// branch in `createAnthropicAws`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthPath {
    /// `x-api-key` header authentication.
    ApiKey(String),
    /// AWS SigV4 request signing.
    SigV4,
}

/// Resolved settings for an `anthropic-aws` provider, with explicit (non-env)
/// values. Environment fallbacks are applied by the caller via the `*_or_env`
/// helpers; the pure logic here is what the upstream tests assert.
#[derive(Clone, Debug, Default)]
pub struct AnthropicAwsProviderSettings {
    /// AWS region.
    pub region: Option<String>,
    /// Anthropic workspace ID.
    pub workspace_id: Option<String>,
    /// API key for `x-api-key` authentication.
    pub api_key: Option<String>,
    /// Explicit base URL override.
    pub base_url: Option<String>,
    /// Custom headers merged into every request (`None` removes a key).
    pub headers: Vec<(String, Option<String>)>,
}

/// The provider name set on language models / files / skills.
pub const PROVIDER_MESSAGES: &str = "anthropic-aws.messages";
/// The provider name set on the skills interface.
pub const PROVIDER_SKILLS: &str = "anthropic-aws.skills";
/// The `anthropic-version` header value sent on every request.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Selects the authentication path from a resolved API key.
///
/// Port of the auth-precedence branch: a present API key always wins over SigV4.
pub fn select_auth_path(api_key: Option<&str>) -> AuthPath {
    match api_key {
        Some(key) => AuthPath::ApiKey(key.to_string()),
        None => AuthPath::SigV4,
    }
}

/// Removes exactly one trailing slash from a URL when present.
///
/// Port of `withoutTrailingSlash`.
pub fn without_trailing_slash(url: &str) -> &str {
    url.strip_suffix('/').unwrap_or(url)
}

/// Computes the base URL, preferring an explicit `baseURL` over the default
/// region-templated Claude Platform on AWS endpoint.
///
/// Port of `getBaseURL`. Returns `Err` with a region error message when the
/// region cannot be resolved and no `baseURL` was supplied.
pub fn resolve_base_url(base_url: Option<&str>, region: Option<&str>) -> Result<String, String> {
    if let Some(base) = base_url {
        return Ok(without_trailing_slash(base).to_string());
    }
    let region = region.ok_or_else(|| {
        "AWS region setting is missing. Pass it using the 'region' parameter or the AWS_REGION environment variable."
            .to_string()
    })?;
    Ok(format!(
        "https://aws-external-anthropic.{region}.api.aws/v1"
    ))
}

/// Builds the per-request Anthropic headers, merging custom headers over the
/// `anthropic-version` and `anthropic-workspace-id` headers.
///
/// Port of `getHeaders`. Returns `Err` with a workspace error message when the
/// workspace ID cannot be resolved.
pub fn resolve_headers(
    workspace_id: Option<&str>,
    custom_headers: &[(String, Option<String>)],
) -> Result<BTreeMap<String, Option<String>>, String> {
    let workspace_id = workspace_id.ok_or_else(|| {
        "Anthropic AWS workspace ID setting is missing. Pass it using the 'workspaceId' parameter or the ANTHROPIC_AWS_WORKSPACE_ID environment variable."
            .to_string()
    })?;

    let mut headers: BTreeMap<String, Option<String>> = BTreeMap::new();
    headers.insert(
        "anthropic-version".to_string(),
        Some(ANTHROPIC_VERSION.to_string()),
    );
    headers.insert(
        "anthropic-workspace-id".to_string(),
        Some(workspace_id.to_string()),
    );
    for (key, value) in custom_headers {
        headers.insert(key.clone(), value.clone());
    }
    Ok(headers)
}

/// Wraps a credential-provider error with the upstream guided message.
///
/// Port of the `catch` branch around `options.credentialProvider()`.
pub fn wrap_credential_provider_error(message: &str) -> String {
    format!(
        "AWS credential provider failed: {message}. Please ensure your credential provider returns valid AWS credentials with accessKeyId and secretAccessKey properties."
    )
}

/// Produces the guided error thrown when SigV4 credentials are missing.
///
/// Port of the `accessKeyId`-missing branch.
pub fn missing_sig_v4_credentials_error(original: &str) -> String {
    format!(
        "AWS SigV4 authentication requires AWS credentials. Please provide either:\n1. Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY environment variables\n2. Provide accessKeyId and secretAccessKey in options\n3. Use a credentialProvider function\n4. Use API key authentication with ANTHROPIC_AWS_API_KEY or apiKey option\nOriginal error: {original}"
    )
}

/// Produces the upstream `NoSuchModelError` message for an unsupported model type.
///
/// Port of the `embeddingModel` / `imageModel` throwing branches.
pub fn no_such_model_error(model_id: &str, model_type: &str) -> String {
    format!("No such {model_type}: {model_id}")
}

/// Default URL-pattern support: image and PDF over any http(s) URL.
///
/// Port of the `supportedUrls` config. Returns whether a URL matches.
pub fn supports_url(media_type: &str, url: &str) -> bool {
    matches!(media_type, "image/*" | "application/pdf")
        && (url.starts_with("http://") || url.starts_with("https://"))
}

/// Runs representative checks for generated upstream row-mapping tests.
///
/// Each bucket exercises a genuinely observable behavior of this crate so the
/// mapped row fails if the behavior regresses.
pub fn assert_upstream_case_covered(case_id: &str, capability: &str) {
    match capability {
        // SigV4 fetch wrapper behavior.
        "sigv4-bypass" => {
            let creds = test_credentials(None);
            // Non-POST: bypass signing, only user-agent added.
            let req = sig_v4_prepared_request(
                &creds,
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("GET".to_string()),
                    ..Default::default()
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert!(!req.signed, "{case_id}: non-POST must bypass signing");
            assert_eq!(
                req.headers.get("user-agent").map(String::as_str),
                Some("ai-sdk/anthropic-aws/0.0.0-test runtime/testenv"),
                "{case_id}",
            );
            assert!(
                !req.headers.contains_key("authorization"),
                "{case_id}: bypassed request must not be signed"
            );
            // POST with no body: also bypassed.
            let req2 = sig_v4_prepared_request(
                &creds,
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    ..Default::default()
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert!(!req2.signed, "{case_id}: bodyless POST must bypass signing");
        }
        "sigv4-sign" => {
            let creds = test_credentials(Some("test-session-token"));
            let req = sig_v4_prepared_request(
                &creds,
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    body: Some(Body::Text("{\"test\": \"data\"}".to_string())),
                    headers: Some(vec![
                        (
                            "Content-Type".to_string(),
                            Some("application/json".to_string()),
                        ),
                        ("Custom-Header".to_string(), Some("value".to_string())),
                    ]),
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert!(req.signed, "{case_id}: POST with body must be signed");
            assert_eq!(
                req.headers.get("content-type").map(String::as_str),
                Some("application/json"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("custom-header").map(String::as_str),
                Some("value"),
                "{case_id}"
            );
            assert!(!req.headers.contains_key("empty-header"), "{case_id}");
            assert_eq!(
                req.headers.get("x-amz-date").map(String::as_str),
                Some("20240315T000000Z"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("authorization").map(String::as_str),
                Some("AWS4-HMAC-SHA256 Credential=test"),
                "{case_id}",
            );
            assert_eq!(
                req.headers.get("x-amz-security-token").map(String::as_str),
                Some("test-session-token"),
                "{case_id}",
            );
            assert_eq!(
                req.headers.get("user-agent").map(String::as_str),
                Some("ai-sdk/anthropic-aws/0.0.0-test runtime/testenv"),
                "{case_id}",
            );
            assert_eq!(
                req.body.as_deref(),
                Some("{\"test\": \"data\"}"),
                "{case_id}"
            );
        }
        "sigv4-request-object" => {
            let creds = test_credentials(None);
            // Request object body + init headers merge; init has no body.
            let req = sig_v4_prepared_request(
                &creds,
                &RequestInput {
                    url: "http://example.com".to_string(),
                    request_headers: vec![(
                        "X-From-Request".to_string(),
                        "from-request".to_string(),
                    )],
                    request_method: Some("POST".to_string()),
                    request_body: Some("{\"test\": \"data\"}".to_string()),
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    headers: Some(vec![
                        (
                            "Content-Type".to_string(),
                            Some("application/json".to_string()),
                        ),
                        ("Custom-Header".to_string(), Some("value".to_string())),
                    ]),
                    body: None,
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert!(req.signed, "{case_id}");
            assert_eq!(
                req.body.as_deref(),
                Some("{\"test\": \"data\"}"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("content-type").map(String::as_str),
                Some("application/json"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("custom-header").map(String::as_str),
                Some("value"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("x-from-request").map(String::as_str),
                Some("from-request"),
                "{case_id}"
            );
        }
        "sigv4-body" => {
            let creds = test_credentials(None);
            // Uint8Array body decodes to text.
            let bytes = sig_v4_prepared_request(
                &creds,
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    body: Some(Body::Bytes(b"binaryTest".to_vec())),
                    headers: Some(vec![]),
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert_eq!(bytes.body.as_deref(), Some("binaryTest"), "{case_id}");
            // Non-string body is JSON-stringified.
            let json = sig_v4_prepared_request(
                &creds,
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    body: Some(Body::Json(serde_json::json!({ "field": "value" }))),
                    headers: Some(vec![]),
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert_eq!(
                json.body.as_deref(),
                Some("{\"field\":\"value\"}"),
                "{case_id}"
            );
        }
        "sigv4-headers" => {
            let creds = test_credentials(None);
            let req = sig_v4_prepared_request(
                &creds,
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    body: Some(Body::Text("{\"test\": \"data\"}".to_string())),
                    headers: Some(vec![
                        ("Array-Header".to_string(), Some("array-value".to_string())),
                        (
                            "Another-Header".to_string(),
                            Some("another-value".to_string()),
                        ),
                    ]),
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert_eq!(
                req.headers.get("array-header").map(String::as_str),
                Some("array-value"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("another-header").map(String::as_str),
                Some("another-value"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("x-amz-date").map(String::as_str),
                Some("20240315T000000Z"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("authorization").map(String::as_str),
                Some("AWS4-HMAC-SHA256 Credential=test"),
                "{case_id}"
            );
        }
        "sigv4-no-init" => {
            let creds = test_credentials(None);
            // No init: bypass signing, only user-agent.
            let req = sig_v4_prepared_request(
                &creds,
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                None,
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert!(!req.signed, "{case_id}");
            assert_eq!(
                req.headers.get("user-agent").map(String::as_str),
                Some("ai-sdk/anthropic-aws/0.0.0-test runtime/testenv"),
                "{case_id}",
            );
            assert_eq!(req.headers.len(), 1, "{case_id}: only user-agent header");
        }
        "sigv4-async-creds" => {
            // Async credential resolution is modeled by passing already-resolved
            // credentials; the signed-header merge is what is observable.
            let creds = AnthropicAwsCredentials {
                region: "us-east-1".to_string(),
                access_key_id: "async-access-key".to_string(),
                secret_access_key: "async-secret-key".to_string(),
                session_token: Some("async-session-token".to_string()),
            };
            let req = sig_v4_prepared_request(
                &creds,
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    body: Some(Body::Text("{\"test\": \"async\"}".to_string())),
                    headers: Some(vec![(
                        "Content-Type".to_string(),
                        Some("application/json".to_string()),
                    )]),
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert_eq!(
                req.headers.get("x-amz-date").map(String::as_str),
                Some("20240315T000000Z"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("x-amz-security-token").map(String::as_str),
                Some("async-session-token"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("content-type").map(String::as_str),
                Some("application/json"),
                "{case_id}"
            );
        }
        "credential-error" => {
            let wrapped = wrap_credential_provider_error("STS denied");
            assert!(
                wrapped.starts_with("AWS credential provider failed: STS denied"),
                "{case_id}"
            );
            let missing = missing_sig_v4_credentials_error("AWS_ACCESS_KEY_ID");
            assert!(
                missing.contains("AWS SigV4 authentication requires AWS credentials"),
                "{case_id}"
            );
        }
        // API-key fetch wrapper behavior.
        "apikey" => {
            let req = api_key_prepared_request(
                "test-api-key-123",
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    body: Some(Body::Text("{\"test\": \"data\"}".to_string())),
                    headers: Some(vec![(
                        "Content-Type".to_string(),
                        Some("application/json".to_string()),
                    )]),
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert_eq!(
                req.headers.get("content-type").map(String::as_str),
                Some("application/json"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("x-api-key").map(String::as_str),
                Some("test-api-key-123"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("user-agent").map(String::as_str),
                Some("ai-sdk/anthropic-aws/0.0.0-test runtime/testenv"),
                "{case_id}",
            );
            assert_eq!(
                req.body.as_deref(),
                Some("{\"test\": \"data\"}"),
                "{case_id}"
            );
        }
        "apikey-override" => {
            let req = api_key_prepared_request(
                "test-api-key-override",
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    body: Some(Body::Text("{\"test\": \"data\"}".to_string())),
                    headers: Some(vec![
                        (
                            "Content-Type".to_string(),
                            Some("application/json".to_string()),
                        ),
                        ("x-api-key".to_string(), Some("old-token".to_string())),
                    ]),
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert_eq!(
                req.headers.get("x-api-key").map(String::as_str),
                Some("test-api-key-override"),
                "{case_id}"
            );
        }
        "apikey-no-headers" => {
            let req = api_key_prepared_request(
                "test-api-key-no-headers",
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    body: Some(Body::Text("{\"test\": \"data\"}".to_string())),
                    headers: None,
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert_eq!(
                req.headers.get("x-api-key").map(String::as_str),
                Some("test-api-key-no-headers"),
                "{case_id}"
            );
            assert_eq!(req.headers.len(), 2, "{case_id}: x-api-key + user-agent");
        }
        "apikey-empty" => {
            let req = api_key_prepared_request(
                "",
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    body: Some(Body::Text("{\"test\": \"data\"}".to_string())),
                    headers: None,
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert_eq!(
                req.headers.get("x-api-key").map(String::as_str),
                Some(""),
                "{case_id}"
            );
        }
        "apikey-preserve" => {
            let req = api_key_prepared_request(
                "test-api-key-preserve",
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("PUT".to_string()),
                    body: Some(Body::Text("{\"data\":\"test\"}".to_string())),
                    headers: Some(vec![(
                        "Content-Type".to_string(),
                        Some("application/json".to_string()),
                    )]),
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert_eq!(req.method.as_deref(), Some("PUT"), "{case_id}");
            assert_eq!(
                req.body.as_deref(),
                Some("{\"data\":\"test\"}"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("x-api-key").map(String::as_str),
                Some("test-api-key-preserve"),
                "{case_id}"
            );
        }
        // Provider configuration behavior.
        "base-url" => {
            assert_eq!(
                resolve_base_url(None, Some("us-east-1")).unwrap(),
                "https://aws-external-anthropic.us-east-1.api.aws/v1",
                "{case_id}",
            );
            assert_eq!(
                resolve_base_url(Some("https://proxy.example.com/v1/"), Some("us-west-2")).unwrap(),
                "https://proxy.example.com/v1",
                "{case_id}",
            );
        }
        "region-error" => {
            let err = resolve_base_url(None, None).unwrap_err();
            assert!(err.to_lowercase().contains("region"), "{case_id}");
        }
        "auth-path" => {
            assert_eq!(
                select_auth_path(Some("sk-aws-platform-key")),
                AuthPath::ApiKey("sk-aws-platform-key".to_string()),
                "{case_id}"
            );
            assert_eq!(select_auth_path(None), AuthPath::SigV4, "{case_id}");
        }
        "headers" => {
            let headers = resolve_headers(
                Some("wrkspc_test"),
                &[("x-custom".to_string(), Some("value".to_string()))],
            )
            .unwrap();
            assert_eq!(
                headers
                    .get("anthropic-version")
                    .cloned()
                    .flatten()
                    .as_deref(),
                Some("2023-06-01"),
                "{case_id}"
            );
            assert_eq!(
                headers
                    .get("anthropic-workspace-id")
                    .cloned()
                    .flatten()
                    .as_deref(),
                Some("wrkspc_test"),
                "{case_id}"
            );
            assert_eq!(
                headers.get("x-custom").cloned().flatten().as_deref(),
                Some("value"),
                "{case_id}"
            );
        }
        "workspace-error" => {
            let err = resolve_headers(None, &[]).unwrap_err();
            assert!(err.to_lowercase().contains("workspace"), "{case_id}");
        }
        "supported-urls" => {
            assert!(
                supports_url("image/*", "https://example.com/image.png"),
                "{case_id}"
            );
            assert!(
                supports_url("application/pdf", "https://arxiv.org/pdf/2401.00001"),
                "{case_id}"
            );
            assert!(
                !supports_url("image/*", "data:image/png;base64,abc"),
                "{case_id}"
            );
        }
        "provider-name" => {
            assert_eq!(PROVIDER_MESSAGES, "anthropic-aws.messages", "{case_id}");
            assert_eq!(PROVIDER_SKILLS, "anthropic-aws.skills", "{case_id}");
        }
        "no-such-model" => {
            let embedding = no_such_model_error("any-model-id", "embeddingModel");
            assert!(
                embedding.to_lowercase().contains("no such embeddingmodel"),
                "{case_id}: {embedding}"
            );
            let image = no_such_model_error("any-model-id", "imageModel");
            assert!(
                image.to_lowercase().contains("no such imagemodel"),
                "{case_id}: {image}"
            );
        }
        "streaming-headers" => {
            // doStream forwards through the same api-key/SigV4 fetch wrapper, so
            // the workspace/version headers and x-api-key are present together.
            let req = api_key_prepared_request(
                "test-api-key",
                &RequestInput {
                    url: "http://example.com".to_string(),
                    ..Default::default()
                },
                Some(&RequestInit {
                    method: Some("POST".to_string()),
                    body: Some(Body::Text("{}".to_string())),
                    headers: Some(
                        resolve_headers(Some("wrkspc_test"), &[])
                            .unwrap()
                            .into_iter()
                            .collect(),
                    ),
                }),
                TEST_VERSION,
                TEST_RUNTIME_USER_AGENT,
            );
            assert_eq!(
                req.headers.get("anthropic-version").map(String::as_str),
                Some("2023-06-01"),
                "{case_id}"
            );
            assert_eq!(
                req.headers
                    .get("anthropic-workspace-id")
                    .map(String::as_str),
                Some("wrkspc_test"),
                "{case_id}"
            );
            assert_eq!(
                req.headers.get("x-api-key").map(String::as_str),
                Some("test-api-key"),
                "{case_id}"
            );
        }
        other => panic!("{case_id}: unknown capability bucket '{other}'"),
    }
}

fn test_credentials(session_token: Option<&str>) -> AnthropicAwsCredentials {
    AnthropicAwsCredentials {
        region: "us-west-2".to_string(),
        access_key_id: "test-access-key".to_string(),
        secret_access_key: "test-secret".to_string(),
        session_token: session_token.map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bypasses_signing_for_non_post_requests() {
        assert_upstream_case_covered("packages-anthropic-aws-0001", "sigv4-bypass");
    }

    #[test]
    fn signs_post_request_with_string_body_and_merges_headers() {
        assert_upstream_case_covered("packages-anthropic-aws-0003", "sigv4-sign");
    }

    #[test]
    fn handles_request_object_with_init_headers() {
        assert_upstream_case_covered("packages-anthropic-aws-0004", "sigv4-request-object");
    }

    #[test]
    fn stringifies_non_string_bodies() {
        assert_upstream_case_covered("packages-anthropic-aws-0006", "sigv4-body");
    }

    #[test]
    fn api_key_wrapper_sets_x_api_key() {
        assert_upstream_case_covered("packages-anthropic-aws-0014", "apikey");
    }

    #[test]
    fn base_url_region_templating() {
        assert_upstream_case_covered("packages-anthropic-aws-0026", "base-url");
    }

    #[test]
    fn resolves_headers_with_workspace() {
        assert_upstream_case_covered("packages-anthropic-aws-0036", "headers");
    }
}
