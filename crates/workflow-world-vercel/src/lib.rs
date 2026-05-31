//! Vercel World implementation contracts for the standalone Workflow SDK port.
//!
//! This crate maps to upstream `packages/world-vercel`. The default surface is
//! deterministic and service-free: it ports run-id encoding, encryption key
//! derivation, HTTP request contracts, VQS queue planning, storage endpoints,
//! and stream chunk framing without requiring live Vercel credentials.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use base64::Engine;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use workflow_world::{Headers, sanitize_queue_name};

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for this implementation slice.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/world-vercel";

/// Upstream package version inventoried for this implementation slice.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.9";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Maximum chunks per writeMulti request.
pub const MAX_CHUNKS_PER_REQUEST: usize = 1000;

/// Default retry delay for handler errors.
pub const HANDLER_ERROR_RETRY_AFTER_SECONDS: u64 = 1;

/// Maximum retry delay for handler errors.
pub const HANDLER_ERROR_MAX_RETRY_AFTER_SECONDS: u64 = 60;

/// Default maximum delay used for VQS re-enqueue sleeps.
pub const MAX_DELAY_SECONDS: u64 = 82_800;

const SPEC_VERSION_CURRENT: u16 = 3;
const SPEC_VERSION_SUPPORTS_CBOR_QUEUE_TRANSPORT: u16 = 3;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QueueOptions {
    pub deployment_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub delay_seconds: Option<u64>,
    pub headers: Headers,
    pub spec_version: Option<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueuePayload {
    Workflow {
        run_id: String,
    },
    Step {
        workflow_name: String,
        workflow_run_id: String,
        workflow_started_at_ms: u64,
        step_id: String,
    },
    HealthCheck {
        correlation_id: String,
    },
}

impl QueuePayload {
    pub fn workflow_headers(&self) -> Headers {
        let mut headers = Headers::new();
        match self {
            Self::Workflow { run_id } => {
                headers.insert("x-vercel-workflow-run-id".into(), run_id.clone());
            }
            Self::Step {
                workflow_run_id,
                step_id,
                ..
            } => {
                headers.insert("x-vercel-workflow-run-id".into(), workflow_run_id.clone());
                headers.insert("x-vercel-workflow-step-id".into(), step_id.clone());
            }
            Self::HealthCheck { .. } => {}
        }
        headers
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueEnvelope {
    pub payload: QueuePayload,
    pub queue_name: String,
    pub deployment_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueSendPlan {
    pub deployment_id: String,
    pub topic_name: String,
    pub envelope: QueueEnvelope,
    pub idempotency_key: Option<String>,
    pub delay_seconds: Option<u64>,
    pub headers: Headers,
    pub content_type: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueHandlerMetadata {
    pub queue_name: String,
    pub message_id: String,
    pub attempt: u32,
    pub request_id: Option<String>,
}

/// General Vercel world error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VercelWorldError {
    message: String,
}

impl VercelWorldError {
    /// Create a new error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for VercelWorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for VercelWorldError {}

/// Low-level tagged ULID codec and region metadata.
pub mod run_id {
    use super::VercelWorldError;

    /// Number of characters in a ULID string.
    pub const ULID_LENGTH: usize = 26;
    /// Number of bytes in the decoded ULID value.
    pub const ULID_BYTE_LENGTH: usize = 16;
    /// Tag bit in byte 0.
    pub const TAG_BIT_MASK: u8 = 0x80;
    /// Region id mask in byte 6.
    pub const REGION_MASK: u8 = 0xfc;
    /// High version bits in byte 6.
    pub const VERSION_HIGH_MASK: u8 = 0x03;
    /// Low version bits in byte 7.
    pub const VERSION_LOW_MASK: u8 = 0xe0;
    /// Metadata byte carrying region id and high version bits.
    pub const REGION_BYTE_INDEX: usize = 6;
    /// Metadata byte carrying low version bits.
    pub const VERSION_LOW_BYTE_INDEX: usize = 7;
    /// Current encoding version.
    pub const CURRENT_VERSION: u8 = 1;
    /// Maximum encodable version.
    pub const MAX_VERSION: u8 = 31;
    /// Maximum encodable region id.
    pub const MAX_REGION_ID: u8 = 63;

    const ENCODING: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

    /// Stable region table from upstream `run-id/regions.ts`.
    pub const REGION_IDS: &[(&str, u8)] = &[
        ("unknown", 0),
        ("iad1", 1),
        ("sfo1", 2),
        ("pdx1", 3),
        ("cle1", 4),
        ("yul1", 5),
        ("gru1", 6),
        ("dub1", 7),
        ("lhr1", 8),
        ("cdg1", 9),
        ("fra1", 10),
        ("bru1", 11),
        ("arn1", 12),
        ("hel1", 13),
        ("zrh1", 14),
        ("cpt1", 15),
        ("dxb1", 16),
        ("bom1", 17),
        ("sin1", 18),
        ("hkg1", 19),
        ("hnd1", 20),
        ("icn1", 21),
        ("kix1", 22),
        ("syd1", 23),
    ];

    /// Region selector accepted by [`encode`].
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Region<'a> {
        /// Numeric region id in `[0, 63]`.
        Id(u8),
        /// Known Vercel region code. The `"unknown"` sentinel is rejected.
        Code(&'a str),
    }

    impl<'a> From<&'a str> for Region<'a> {
        fn from(value: &'a str) -> Self {
            Self::Code(value)
        }
    }

    impl<'a> From<u8> for Region<'a> {
        fn from(value: u8) -> Self {
            Self::Id(value)
        }
    }

    /// Decoded tagged or untagged run id.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct DecodedRunId {
        pub tagged: bool,
        pub ulid: String,
        pub version: Option<u8>,
        pub region_id: Option<u8>,
        pub region: Option<&'static str>,
    }

    fn decode_char(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'A'..=b'H' => Some(byte - b'A' + 10),
            b'J'..=b'K' => Some(byte - b'J' + 18),
            b'M'..=b'N' => Some(byte - b'M' + 20),
            b'P'..=b'T' => Some(byte - b'P' + 22),
            b'V'..=b'Z' => Some(byte - b'V' + 27),
            b'a'..=b'h' => Some(byte - b'a' + 10),
            b'j'..=b'k' => Some(byte - b'j' + 18),
            b'm'..=b'n' => Some(byte - b'm' + 20),
            b'p'..=b't' => Some(byte - b'p' + 22),
            b'v'..=b'z' => Some(byte - b'v' + 27),
            _ => None,
        }
    }

    /// Decode a Crockford-Base32 ULID into bytes.
    pub fn ulid_to_bytes(ulid: &str) -> Result<[u8; ULID_BYTE_LENGTH], VercelWorldError> {
        if ulid.len() != ULID_LENGTH {
            return Err(VercelWorldError::new(format!(
                "Invalid ULID length: expected {ULID_LENGTH}, got {}",
                ulid.len()
            )));
        }

        let mut values = [0_u8; ULID_LENGTH];
        for (index, byte) in ulid.bytes().enumerate() {
            values[index] = decode_char(byte).ok_or_else(|| {
                VercelWorldError::new(format!(
                    "Invalid Crockford-Base32 character at index {index}"
                ))
            })?;
        }

        if values[0] & 0x18 != 0 {
            return Err(VercelWorldError::new(
                "Invalid ULID: top 2 bits must be zero (first char > '7')",
            ));
        }

        let mut out = [0_u8; ULID_BYTE_LENGTH];
        let mut bit_buf = u32::from(values[0] & 0x07);
        let mut bit_count = 3_u8;
        let mut out_index = 0_usize;

        for value in values.iter().skip(1) {
            bit_buf = (bit_buf << 5) | u32::from(*value);
            bit_count += 5;
            while bit_count >= 8 {
                bit_count -= 8;
                out[out_index] = ((bit_buf >> bit_count) & 0xff) as u8;
                out_index += 1;
            }
        }

        if out_index != ULID_BYTE_LENGTH || bit_count != 0 {
            return Err(VercelWorldError::new(
                "Internal error: ULID bit packing did not consume cleanly",
            ));
        }

        Ok(out)
    }

    /// Encode 16 bytes into an uppercase Crockford-Base32 ULID.
    pub fn bytes_to_ulid(bytes: &[u8]) -> Result<String, VercelWorldError> {
        if bytes.len() != ULID_BYTE_LENGTH {
            return Err(VercelWorldError::new(format!(
                "Invalid byte length: expected {ULID_BYTE_LENGTH}, got {}",
                bytes.len()
            )));
        }

        let mut out = String::with_capacity(ULID_LENGTH);
        let mut bit_buf = 0_u32;
        let mut bit_count = 2_u8;

        for byte in bytes {
            bit_buf = (bit_buf << 8) | u32::from(*byte);
            bit_count += 8;
            while bit_count >= 5 {
                bit_count -= 5;
                out.push(ENCODING[((bit_buf >> bit_count) & 0x1f) as usize] as char);
            }
        }

        if out.len() != ULID_LENGTH || bit_count != 0 {
            return Err(VercelWorldError::new(
                "Internal error: ULID bit packing did not flush cleanly",
            ));
        }

        Ok(out)
    }

    /// Look up a region code by id. Returns `None` for id 0 and unknown ids.
    pub fn lookup_region(region_id: u8) -> Option<&'static str> {
        REGION_IDS
            .iter()
            .find(|(code, id)| *id == region_id && *code != "unknown")
            .map(|(code, _)| *code)
    }

    /// Look up a known region id by code.
    pub fn region_id_for(code: &str) -> Result<u8, VercelWorldError> {
        REGION_IDS
            .iter()
            .find(|(candidate, _)| *candidate == code && *candidate != "unknown")
            .map(|(_, id)| *id)
            .ok_or_else(|| VercelWorldError::new(format!("Unknown region: {code}")))
    }

    fn resolve_region(region: Region<'_>) -> Result<u8, VercelWorldError> {
        match region {
            Region::Id(id) if id <= MAX_REGION_ID => Ok(id),
            Region::Id(id) => Err(VercelWorldError::new(format!(
                "regionId must be an integer in [0, {MAX_REGION_ID}]; got {id}"
            ))),
            Region::Code(code) => region_id_for(code),
        }
    }

    /// Encode a region id and version into a tagged ULID.
    pub fn encode<'a>(
        ulid: &str,
        region: impl Into<Region<'a>>,
        version: Option<u8>,
    ) -> Result<String, VercelWorldError> {
        let region_id = resolve_region(region.into())?;
        let version = version.unwrap_or(CURRENT_VERSION);
        if version > MAX_VERSION {
            return Err(VercelWorldError::new(format!(
                "version must be an integer in [0, {MAX_VERSION}]; got {version}"
            )));
        }

        let mut bytes = ulid_to_bytes(ulid)?;
        bytes[0] |= TAG_BIT_MASK;

        let region_shifted = (region_id & MAX_REGION_ID) << 2;
        let version_high = (version >> 3) & VERSION_HIGH_MASK;
        let version_low = (version & 0x07) << 5;

        bytes[REGION_BYTE_INDEX] = (bytes[REGION_BYTE_INDEX] & !(REGION_MASK | VERSION_HIGH_MASK))
            | region_shifted
            | version_high;
        bytes[VERSION_LOW_BYTE_INDEX] =
            (bytes[VERSION_LOW_BYTE_INDEX] & !VERSION_LOW_MASK) | version_low;

        bytes_to_ulid(&bytes)
    }

    /// Decode a tagged or untagged ULID.
    pub fn decode(tagged_ulid: &str) -> Result<DecodedRunId, VercelWorldError> {
        let mut bytes = ulid_to_bytes(tagged_ulid)?;
        let tagged = bytes[0] & TAG_BIT_MASK != 0;
        bytes[0] &= !TAG_BIT_MASK;
        let ulid = bytes_to_ulid(&bytes)?;

        if !tagged {
            return Ok(DecodedRunId {
                tagged: false,
                ulid,
                version: None,
                region_id: None,
                region: None,
            });
        }

        let region_id = (bytes[REGION_BYTE_INDEX] & REGION_MASK) >> 2;
        let version = ((bytes[REGION_BYTE_INDEX] & VERSION_HIGH_MASK) << 3)
            | ((bytes[VERSION_LOW_BYTE_INDEX] & VERSION_LOW_MASK) >> 5);

        Ok(DecodedRunId {
            tagged: true,
            ulid,
            version: Some(version),
            region_id: Some(region_id),
            region: lookup_region(region_id),
        })
    }

    /// Test whether a value is a valid tagged ULID.
    pub fn is_tagged_string(value: &str) -> bool {
        ulid_to_bytes(value)
            .map(|bytes| bytes[0] & TAG_BIT_MASK != 0)
            .unwrap_or(false)
    }
}

/// Deployment and per-run encryption helpers.
pub mod encryption {
    use super::*;

    const KEY_BYTES: usize = 32;
    type HmacSha256 = Hmac<Sha256>;

    fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; KEY_BYTES] {
        let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
        for part in parts {
            mac.update(part);
        }
        mac.finalize().into_bytes().into()
    }

    /// Derive a per-run AES-256 key using HKDF-SHA256.
    pub fn derive_run_key(
        deployment_key: &[u8],
        project_id: &str,
        run_id: &str,
    ) -> Result<[u8; KEY_BYTES], VercelWorldError> {
        if deployment_key.len() != KEY_BYTES {
            return Err(VercelWorldError::new(format!(
                "Invalid deployment key length: expected {KEY_BYTES} bytes for AES-256, got {} bytes",
                deployment_key.len()
            )));
        }
        if project_id.is_empty() {
            return Err(VercelWorldError::new(
                "projectId must be a non-empty string",
            ));
        }

        let salt = [0_u8; KEY_BYTES];
        let prk = hmac_sha256(&salt, &[deployment_key]);
        let info = format!("{project_id}|{run_id}");
        Ok(hmac_sha256(&prk, &[info.as_bytes(), &[1]]))
    }

    /// HTTP request needed to fetch a server-derived run key.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct FetchRunKeyRequest {
        pub url: String,
        pub headers: Headers,
    }

    /// Build the Vercel API run-key request.
    pub fn fetch_run_key_request(
        deployment_id: &str,
        project_id: &str,
        run_id: &str,
        token: Option<&str>,
        team_id: Option<&str>,
    ) -> Result<FetchRunKeyRequest, VercelWorldError> {
        let token = token.ok_or_else(|| {
            VercelWorldError::new("Cannot fetch run key: no OIDC token or VERCEL_TOKEN available")
        })?;
        let mut url = format!(
            "https://api.vercel.com/v1/workflow/run-key/{}?projectId={}&runId={}",
            percent_encode(deployment_id),
            percent_encode(project_id),
            percent_encode(run_id)
        );
        if let Some(team_id) = team_id {
            url.push_str("&teamId=");
            url.push_str(&percent_encode(team_id));
        }
        let mut headers = Headers::new();
        headers.insert("Authorization".into(), format!("Bearer {token}"));
        Ok(FetchRunKeyRequest { url, headers })
    }

    /// Parse a run-key API response body.
    pub fn parse_fetch_run_key_response(
        status: u16,
        status_text: &str,
        body: &str,
    ) -> Result<Option<Vec<u8>>, VercelWorldError> {
        if !(200..300).contains(&status) {
            return Err(VercelWorldError::new(format!(
                "Failed to fetch run key: HTTP {status} {status_text}{}",
                if body.is_empty() {
                    String::new()
                } else {
                    format!(" - {body}")
                }
            )));
        }
        let value: Value = serde_json::from_str(body).map_err(|error| {
            VercelWorldError::new(format!(
                "Invalid response from Vercel API: expected {{ key: string | null }}. JSON error: {error}"
            ))
        })?;
        match value.get("key") {
            Some(Value::Null) => Ok(None),
            Some(Value::String(key)) => base64::engine::general_purpose::STANDARD
                .decode(key)
                .map(Some)
                .map_err(|error| VercelWorldError::new(error.to_string())),
            _ => Err(VercelWorldError::new(
                "Invalid response from Vercel API: expected { key: string | null }",
            )),
        }
    }
}

/// Vercel API configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ApiConfig {
    pub token: Option<String>,
    pub headers: Headers,
    pub project_config: Option<ProjectConfig>,
}

/// Project routing configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectConfig {
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub team_id: Option<String>,
    pub environment: Option<String>,
}

/// Environment values read by upstream from `process.env`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VercelEnv {
    pub vercel_workflow_server_url: Option<String>,
    pub workflow_vercel_backend_url: Option<String>,
    pub vercel_deployment_id: Option<String>,
    pub vercel_token: Option<String>,
    pub oidc_token: Option<String>,
}

/// HTTP base URL decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpUrl {
    pub base_url: String,
    pub using_proxy: bool,
}

/// Full HTTP config including headers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpConfig {
    pub base_url: String,
    pub headers: Headers,
    pub using_proxy: bool,
}

/// Get the workflow-server or proxy URL.
pub fn get_http_url(config: Option<&ApiConfig>, env: &VercelEnv) -> HttpUrl {
    let project = config.and_then(|c| c.project_config.as_ref());
    let using_proxy = project
        .and_then(|p| p.project_id.as_ref().zip(p.team_id.as_ref()))
        .is_some();
    if using_proxy {
        HttpUrl {
            base_url: env
                .workflow_vercel_backend_url
                .clone()
                .unwrap_or_else(|| "https://api.vercel.com/v1/workflow".into()),
            using_proxy: true,
        }
    } else {
        let default_host = env
            .vercel_workflow_server_url
            .clone()
            .unwrap_or_else(|| "https://vercel-workflow.com".into());
        HttpUrl {
            base_url: format!("{default_host}/api"),
            using_proxy: false,
        }
    }
}

/// Build HTTP headers without auth side effects.
pub fn get_headers(config: Option<&ApiConfig>, env: &VercelEnv, using_proxy: bool) -> Headers {
    let mut headers = config.map(|c| c.headers.clone()).unwrap_or_default();
    headers.insert(
        "User-Agent".into(),
        format!("@workflow/world-vercel/{UPSTREAM_VERSION} rust"),
    );
    if let Some(project) = config.and_then(|c| c.project_config.as_ref()) {
        headers.insert(
            "x-vercel-environment".into(),
            project
                .environment
                .clone()
                .unwrap_or_else(|| "production".into()),
        );
        if let Some(project_id) = &project.project_id {
            headers.insert("x-vercel-project-id".into(), project_id.clone());
        }
        if let Some(team_id) = &project.team_id {
            headers.insert("x-vercel-team-id".into(), team_id.clone());
        }
    }
    if using_proxy {
        if let Some(override_url) = &env.vercel_workflow_server_url {
            headers.insert("x-vercel-workflow-api-url".into(), override_url.clone());
        }
    }
    headers
}

/// Build full HTTP config and auth headers.
pub fn get_http_config(
    config: Option<&ApiConfig>,
    env: &VercelEnv,
) -> Result<HttpConfig, VercelWorldError> {
    let url = get_http_url(config, env);
    let mut headers = get_headers(config, env, url.using_proxy);

    if url.using_proxy {
        let token = config.and_then(|c| c.token.as_ref()).ok_or_else(|| {
            VercelWorldError::new(
                "world-vercel: api-workflow proxy requested but no Vercel auth token was provided",
            )
        })?;
        headers.insert("Authorization".into(), format!("Bearer {token}"));
    } else {
        let auth_token = config
            .and_then(|c| c.token.as_ref())
            .or(env.oidc_token.as_ref());
        if let Some(token) = auth_token {
            headers.insert("Authorization".into(), format!("Bearer {token}"));
        }
        if let Some(token) = &env.oidc_token {
            headers.insert("x-vercel-trusted-oidc-idp-token".into(), token.clone());
        }
    }

    Ok(HttpConfig {
        base_url: url.base_url,
        headers,
        using_proxy: url.using_proxy,
    })
}

/// Planned HTTP request contract for storage calls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpRequestPlan {
    pub method: String,
    pub url: String,
    pub headers: Headers,
    pub body: Option<Vec<u8>>,
}

/// Build a deterministic request plan matching upstream `makeRequest` headers.
pub fn make_request_plan(
    endpoint: &str,
    method: &str,
    data: Option<&[u8]>,
    config: Option<&ApiConfig>,
    env: &VercelEnv,
    request_time_ms: u64,
) -> Result<HttpRequestPlan, VercelWorldError> {
    let mut http = get_http_config(config, env)?;
    http.headers
        .insert("Accept".into(), "application/cbor".into());
    http.headers
        .insert("X-Request-Time".into(), request_time_ms.to_string());
    if data.is_some() {
        http.headers
            .insert("Content-Type".into(), "application/cbor".into());
    }
    Ok(HttpRequestPlan {
        method: method.into(),
        url: format!("{}{}", http.base_url, endpoint),
        headers: http.headers,
        body: data.map(Vec::from),
    })
}

/// Queue send outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueueSendOutcome {
    Sent(QueueSendPlan),
    Duplicate { message_id: String },
}

/// Plan a Vercel queue send without touching VQS.
pub fn plan_queue_send(
    queue_name: &str,
    payload: QueuePayload,
    options: QueueOptions,
    env_deployment_id: Option<&str>,
    duplicate_idempotency_key: Option<&str>,
) -> Result<QueueSendOutcome, VercelWorldError> {
    let deployment_id = options
        .deployment_id
        .clone()
        .or_else(|| env_deployment_id.map(str::to_string))
        .ok_or_else(|| {
            VercelWorldError::new(
                "No deploymentId provided and VERCEL_DEPLOYMENT_ID environment variable is not set",
            )
        })?;

    if let Some(duplicate_key) = duplicate_idempotency_key {
        return Ok(QueueSendOutcome::Duplicate {
            message_id: format!("msg_duplicate_{duplicate_key}"),
        });
    }

    let mut headers = payload.workflow_headers();
    headers.extend(options.headers.clone());
    let use_cbor = options.spec_version.unwrap_or(SPEC_VERSION_CURRENT)
        >= SPEC_VERSION_SUPPORTS_CBOR_QUEUE_TRANSPORT;

    Ok(QueueSendOutcome::Sent(QueueSendPlan {
        deployment_id,
        topic_name: sanitize_queue_name(queue_name),
        envelope: QueueEnvelope {
            payload,
            queue_name: queue_name.into(),
            deployment_id: options.deployment_id.clone(),
        },
        idempotency_key: options.idempotency_key,
        delay_seconds: options.delay_seconds,
        headers,
        content_type: if use_cbor {
            "application/cbor"
        } else {
            "application/json"
        },
    }))
}

/// Compute bounded handler retry delay. `random` should be in `[0, 1)`.
pub fn handler_error_retry_after_seconds(delivery_count: u32, random: f64) -> u64 {
    let exponent = delivery_count.saturating_sub(1).min(63);
    let backoff = (1_u64 << exponent)
        .max(HANDLER_ERROR_RETRY_AFTER_SECONDS)
        .min(HANDLER_ERROR_MAX_RETRY_AFTER_SECONDS);
    let jitter_bound = ((backoff as f64 * 0.25).ceil() as u64) + 1;
    let jitter = (random.clamp(0.0, 0.999_999) * jitter_bound as f64).floor() as u64;
    HANDLER_ERROR_RETRY_AFTER_SECONDS.max(backoff.saturating_sub(jitter))
}

/// Clamp a timeout result into VQS delaySeconds.
pub fn delay_seconds_for_timeout(timeout_seconds: u64) -> Option<u64> {
    if timeout_seconds == 0 {
        None
    } else {
        Some(timeout_seconds.min(MAX_DELAY_SECONDS))
    }
}

/// Extract Vercel request id from the `x-vercel-id` header.
pub fn request_id_from_header(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Build handler metadata.
pub fn queue_handler_metadata(
    queue_name: impl Into<String>,
    message_id: impl Into<String>,
    attempt: u32,
    x_vercel_id: Option<&str>,
) -> QueueHandlerMetadata {
    QueueHandlerMetadata {
        queue_name: queue_name.into(),
        message_id: message_id.into(),
        attempt,
        request_id: request_id_from_header(x_vercel_id),
    }
}

/// Chunk variant accepted by stream encoders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamChunk {
    Text(String),
    Bytes(Vec<u8>),
}

impl From<&str> for StreamChunk {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}

impl From<String> for StreamChunk {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<Vec<u8>> for StreamChunk {
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(value)
    }
}

impl StreamChunk {
    fn into_bytes(self) -> Vec<u8> {
        match self {
            StreamChunk::Text(text) => text.into_bytes(),
            StreamChunk::Bytes(bytes) => bytes,
        }
    }
}

/// Encode chunks as `[4 byte big-endian length][bytes]...`.
pub fn encode_multi_chunks(chunks: impl IntoIterator<Item = StreamChunk>) -> Vec<u8> {
    let chunks: Vec<Vec<u8>> = chunks.into_iter().map(StreamChunk::into_bytes).collect();
    let mut result = Vec::with_capacity(chunks.iter().map(|c| c.len() + 4).sum());
    for chunk in chunks {
        result.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
        result.extend_from_slice(&chunk);
    }
    result
}

/// Decode the multi-chunk format for deterministic tests.
pub fn decode_multi_chunks(encoded: &[u8]) -> Result<Vec<Vec<u8>>, VercelWorldError> {
    let mut chunks = Vec::new();
    let mut offset = 0_usize;
    while offset < encoded.len() {
        if encoded.len() - offset < 4 {
            return Err(VercelWorldError::new("truncated chunk length"));
        }
        let length = u32::from_be_bytes(
            encoded[offset..offset + 4]
                .try_into()
                .expect("slice length checked"),
        ) as usize;
        offset += 4;
        if encoded.len() - offset < length {
            return Err(VercelWorldError::new("truncated chunk data"));
        }
        chunks.push(encoded[offset..offset + length].to_vec());
        offset += length;
    }
    Ok(chunks)
}

/// Build the direct stream URL.
pub fn stream_url(base_url: &str, run_id: &str, name: &str, start_index: Option<i64>) -> String {
    let mut url = format!(
        "{}/v2/runs/{}/stream/{}",
        base_url.trim_end_matches('/'),
        percent_encode(run_id),
        percent_encode(name)
    );
    if let Some(start_index) = start_index {
        url.push_str("?startIndex=");
        url.push_str(&start_index.to_string());
    }
    url
}

/// Error string for failed stream write/close responses.
pub fn stream_request_error(
    operation: &str,
    url: &str,
    status: u16,
    headers: &Headers,
    text: &str,
) -> String {
    let parsed = parse_origin_path(url);
    let mut context = vec![format!("PUT {parsed}")];
    for header in ["x-vercel-id", "x-vercel-error"] {
        if let Some(value) = headers.get(header) {
            context.push(format!("{header}={value}"));
        }
    }
    format!(
        "Stream {operation} failed: HTTP {status} ({}): {text}",
        context.join("; ")
    )
}

/// Return per-request chunk counts for writeMulti pagination.
pub fn write_multi_page_counts(total_chunks: usize) -> Vec<usize> {
    if total_chunks == 0 {
        return Vec::new();
    }
    let mut remaining = total_chunks;
    let mut pages = Vec::new();
    while remaining > 0 {
        let count = remaining.min(MAX_CHUNKS_PER_REQUEST);
        pages.push(count);
        remaining -= count;
    }
    pages
}

/// Build the latest-deployment Vercel API request.
pub fn resolve_latest_deployment_request(
    current_deployment_id: Option<&str>,
    config_token: Option<&str>,
    env_token: Option<&str>,
    oidc_token: Option<&str>,
) -> Result<HttpRequestPlan, VercelWorldError> {
    let current = current_deployment_id.ok_or_else(|| {
        VercelWorldError::new(
            "Cannot resolve latest deployment: VERCEL_DEPLOYMENT_ID environment variable is not set",
        )
    })?;
    let token = config_token.or(env_token).or(oidc_token).ok_or_else(|| {
        VercelWorldError::new(
            "Cannot resolve latest deployment: no OIDC token or VERCEL_TOKEN available",
        )
    })?;
    let mut headers = Headers::new();
    headers.insert("Authorization".into(), format!("Bearer {token}"));
    Ok(HttpRequestPlan {
        method: "GET".into(),
        url: format!(
            "https://api.vercel.com/v1/workflow/resolve-latest-deployment/{}",
            percent_encode(current)
        ),
        headers,
        body: None,
    })
}

/// Parse latest-deployment API response body.
pub fn parse_resolve_latest_deployment_response(
    status: u16,
    status_text: &str,
    body: &str,
) -> Result<String, VercelWorldError> {
    if !(200..300).contains(&status) {
        return Err(VercelWorldError::new(format!(
            "Failed to resolve latest deployment: HTTP {status} {status_text}{}",
            if body.is_empty() {
                String::new()
            } else {
                format!(" - {body}")
            }
        )));
    }
    let value: Value = serde_json::from_str(body).map_err(|error| {
        VercelWorldError::new(format!(
            "Invalid response from Vercel API: expected {{ id: string }}. JSON error: {error}"
        ))
    })?;
    value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            VercelWorldError::new("Invalid response from Vercel API: expected { id: string }")
        })
}

/// Build a run list endpoint.
pub fn list_runs_endpoint(
    workflow_name: Option<&str>,
    status: Option<&str>,
    limit: Option<usize>,
    cursor: Option<&str>,
    sort_order: Option<&str>,
    resolve_data: Option<&str>,
) -> String {
    let mut params = Vec::new();
    push_param(&mut params, "workflowName", workflow_name);
    push_param(&mut params, "status", status);
    if let Some(limit) = limit {
        params.push(("limit".into(), limit.to_string()));
    }
    push_param(&mut params, "cursor", cursor);
    push_param(&mut params, "sortOrder", sort_order);
    params.push((
        "remoteRefBehavior".into(),
        if resolve_data == Some("none") {
            "lazy".into()
        } else {
            "resolve".into()
        },
    ));
    endpoint_with_params("/v2/runs", params)
}

/// Build a run get endpoint.
pub fn get_run_endpoint(run_id: &str, resolve_data: Option<&str>) -> String {
    endpoint_with_params(
        &format!("/v2/runs/{}", percent_encode(run_id)),
        vec![(
            "remoteRefBehavior".into(),
            if resolve_data == Some("none") {
                "lazy".into()
            } else {
                "resolve".into()
            },
        )],
    )
}

/// Build a step endpoint.
pub fn step_endpoint(run_id: &str, step_id: Option<&str>, resolve_data: Option<&str>) -> String {
    let base = if let Some(step_id) = step_id {
        format!(
            "/v2/runs/{}/steps/{}",
            percent_encode(run_id),
            percent_encode(step_id)
        )
    } else {
        format!("/v2/runs/{}/steps", percent_encode(run_id))
    };
    endpoint_with_params(
        &base,
        vec![(
            "remoteRefBehavior".into(),
            if resolve_data == Some("none") {
                "lazy".into()
            } else {
                "resolve".into()
            },
        )],
    )
}

fn push_param(params: &mut Vec<(String, String)>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        params.push((key.into(), value.into()));
    }
}

fn endpoint_with_params(base: &str, params: Vec<(String, String)>) -> String {
    if params.is_empty() {
        return base.into();
    }
    let query = params
        .into_iter()
        .map(|(key, value)| format!("{}={}", percent_encode(&key), percent_encode(&value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{query}")
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn parse_origin_path(url: &str) -> String {
    let Some(scheme_index) = url.find("://") else {
        return url.into();
    };
    let rest = &url[scheme_index + 3..];
    let path_start = rest.find('/').unwrap_or(rest.len());
    let origin = &url[..scheme_index + 3 + path_start];
    let path = &rest[path_start..];
    let path_without_query = path.split('?').next().unwrap_or(path);
    format!("{origin}{path_without_query}")
}

/// Build a header map for tests and callers.
pub fn headers(
    entries: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
) -> Headers {
    entries
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect::<BTreeMap<_, _>>()
}

#[cfg(test)]
mod tests {
    use super::encryption::*;
    use super::run_id::*;
    use super::*;

    const SAMPLE_ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    macro_rules! parity_test {
        ($name:ident, $helper:ident) => {
            #[test]
            fn $name() {
                $helper();
            }
        };
    }

    fn deployment_key() -> [u8; 32] {
        [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ]
    }

    fn assert_encryption_derivation_contract() {
        let key = deployment_key();
        let derived = derive_run_key(&key, "prj_test123", "wrun_abc123").unwrap();
        assert_eq!(derived.len(), 32);
        assert_eq!(
            derive_run_key(&key, "prj_test123", "wrun_abc123").unwrap(),
            derived
        );
        assert_ne!(
            derive_run_key(&key, "prj_test123", "wrun_run1").unwrap(),
            derive_run_key(&key, "prj_test123", "wrun_run2").unwrap()
        );
        assert_ne!(
            derive_run_key(&key, "prj_project1", "wrun_abc123").unwrap(),
            derive_run_key(&key, "prj_project2", "wrun_abc123").unwrap()
        );
        let mut other = key;
        other[31] ^= 0xff;
        assert_ne!(
            derive_run_key(&key, "prj_test123", "wrun_abc123").unwrap(),
            derive_run_key(&other, "prj_test123", "wrun_abc123").unwrap()
        );
        assert!(derive_run_key(&key[..16], "prj_test123", "wrun_abc123").is_err());
        assert!(derive_run_key(&key, "", "wrun_abc123").is_err());
    }

    fn assert_fetch_run_key_contract() {
        let req = fetch_run_key_request(
            "dpl_test123",
            "prj_test123",
            "wrun_abc123",
            Some("test-token"),
            Some("team_123"),
        )
        .unwrap();
        assert_eq!(
            req.url,
            "https://api.vercel.com/v1/workflow/run-key/dpl_test123?projectId=prj_test123&runId=wrun_abc123&teamId=team_123"
        );
        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Bearer test-token".to_string())
        );
        assert!(fetch_run_key_request("dpl", "prj", "run", None, None).is_err());
        assert_eq!(
            parse_fetch_run_key_response(200, "OK", r#"{"key":null}"#).unwrap(),
            None
        );
        let encoded = base64::engine::general_purpose::STANDARD.encode(deployment_key());
        assert_eq!(
            parse_fetch_run_key_response(200, "OK", &format!(r#"{{"key":"{encoded}"}}"#))
                .unwrap()
                .unwrap(),
            deployment_key()
        );
        assert!(
            parse_fetch_run_key_response(404, "Not Found", "Not found")
                .unwrap_err()
                .message()
                .contains("HTTP 404")
        );
        assert!(parse_fetch_run_key_response(200, "OK", r#"{"bad":"field"}"#).is_err());
    }

    fn assert_queue_send_contract() {
        let sent = plan_queue_send(
            "__wkf_workflow_test",
            QueuePayload::Workflow {
                run_id: "wrun_abc123".into(),
            },
            QueueOptions {
                deployment_id: Some("dpl_123".into()),
                idempotency_key: Some("my-key".into()),
                headers: headers([("x-vercel-workflow-run-id", "override")]),
                ..QueueOptions::default()
            },
            None,
            None,
        )
        .unwrap();
        let QueueSendOutcome::Sent(plan) = sent else {
            panic!("expected send plan");
        };
        assert_eq!(plan.deployment_id, "dpl_123");
        assert_eq!(plan.topic_name, "__wkf_workflow_test");
        assert_eq!(plan.envelope.queue_name, "__wkf_workflow_test");
        assert_eq!(plan.envelope.deployment_id, Some("dpl_123".into()));
        assert_eq!(
            plan.headers.get("x-vercel-workflow-run-id"),
            Some(&"override".into())
        );
        assert_eq!(plan.content_type, "application/cbor");

        let env_plan = plan_queue_send(
            "__wkf_step_myStep",
            QueuePayload::Step {
                workflow_name: "wf".into(),
                workflow_run_id: "wrun_abc123".into(),
                workflow_started_at_ms: 1,
                step_id: "step_xyz789".into(),
            },
            QueueOptions::default(),
            Some("dpl_env_123"),
            None,
        )
        .unwrap();
        let QueueSendOutcome::Sent(env_plan) = env_plan else {
            panic!("expected send plan");
        };
        assert_eq!(env_plan.deployment_id, "dpl_env_123");
        assert_eq!(
            env_plan.headers.get("x-vercel-workflow-step-id"),
            Some(&"step_xyz789".into())
        );

        let health = plan_queue_send(
            "__wkf_workflow_health_check",
            QueuePayload::HealthCheck {
                correlation_id: "corr_123".into(),
            },
            QueueOptions::default(),
            Some("dpl"),
            None,
        )
        .unwrap();
        let QueueSendOutcome::Sent(health) = health else {
            panic!("expected send plan");
        };
        assert!(health.headers.is_empty());

        let duplicate = plan_queue_send(
            "__wkf_workflow_test",
            QueuePayload::Workflow {
                run_id: "run-123".into(),
            },
            QueueOptions {
                idempotency_key: Some("my-key".into()),
                ..QueueOptions::default()
            },
            Some("dpl"),
            Some("my-key"),
        )
        .unwrap();
        assert_eq!(
            duplicate,
            QueueSendOutcome::Duplicate {
                message_id: "msg_duplicate_my-key".into()
            }
        );
        assert!(
            plan_queue_send(
                "__wkf_workflow_test",
                QueuePayload::Workflow {
                    run_id: "run-123".into()
                },
                QueueOptions::default(),
                None,
                None
            )
            .is_err()
        );
    }

    fn assert_queue_handler_contract() {
        assert_eq!(handler_error_retry_after_seconds(1, 0.0), 1);
        assert_eq!(handler_error_retry_after_seconds(2, 0.0), 2);
        assert_eq!(handler_error_retry_after_seconds(4, 0.0), 8);
        assert_eq!(handler_error_retry_after_seconds(8, 0.0), 60);
        assert_eq!(handler_error_retry_after_seconds(4, 0.999), 6);
        assert_eq!(handler_error_retry_after_seconds(8, 0.999), 45);
        assert_eq!(delay_seconds_for_timeout(300), Some(300));
        assert_eq!(delay_seconds_for_timeout(100_000), Some(MAX_DELAY_SECONDS));
        assert_eq!(delay_seconds_for_timeout(0), None);
        assert_eq!(
            queue_handler_metadata("__wkf_workflow_test", "msg-123", 1, Some(" iad1::abc123 "))
                .request_id,
            Some("iad1::abc123".into())
        );
        assert_eq!(
            queue_handler_metadata("__wkf_workflow_test", "msg-123", 1, None).request_id,
            None
        );
    }

    fn assert_resolve_latest_contract() {
        let request = resolve_latest_deployment_request(
            Some("dpl_current_abc123"),
            Some("test-token"),
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            request.url,
            "https://api.vercel.com/v1/workflow/resolve-latest-deployment/dpl_current_abc123"
        );
        assert_eq!(
            request.headers.get("Authorization"),
            Some(&"Bearer test-token".to_string())
        );
        let env_request =
            resolve_latest_deployment_request(Some("dpl"), None, Some("env-token"), None).unwrap();
        assert_eq!(
            env_request.headers.get("Authorization"),
            Some(&"Bearer env-token".to_string())
        );
        let oidc_request =
            resolve_latest_deployment_request(Some("dpl"), None, None, Some("oidc")).unwrap();
        assert_eq!(
            oidc_request.headers.get("Authorization"),
            Some(&"Bearer oidc".to_string())
        );
        assert!(resolve_latest_deployment_request(None, Some("token"), None, None).is_err());
        assert!(resolve_latest_deployment_request(Some("dpl"), None, None, None).is_err());
        assert_eq!(
            parse_resolve_latest_deployment_response(200, "OK", r#"{"id":"dpl_latest_xyz789"}"#)
                .unwrap(),
            "dpl_latest_xyz789"
        );
        assert!(
            parse_resolve_latest_deployment_response(404, "Not Found", "Not found")
                .unwrap_err()
                .message()
                .contains("HTTP 404")
        );
        assert!(
            parse_resolve_latest_deployment_response(500, "Server Error", "boom")
                .unwrap_err()
                .message()
                .contains("HTTP 500")
        );
        assert!(
            parse_resolve_latest_deployment_response(200, "OK", r#"{"deploymentId":"x"}"#).is_err()
        );
    }

    fn assert_run_id_codec_contract() {
        let zero = "0".repeat(ULID_LENGTH);
        let max = "7ZZZZZZZZZZZZZZZZZZZZZZZZZ";
        assert_eq!(ulid_to_bytes(&zero).unwrap(), [0; ULID_BYTE_LENGTH]);
        assert_eq!(bytes_to_ulid(&[0; ULID_BYTE_LENGTH]).unwrap(), zero);
        assert_eq!(ulid_to_bytes(&max).unwrap(), [0xff; ULID_BYTE_LENGTH]);
        assert_eq!(bytes_to_ulid(&[0xff; ULID_BYTE_LENGTH]).unwrap(), max);
        assert_eq!(
            ulid_to_bytes(SAMPLE_ULID).unwrap(),
            [
                0x01, 0x56, 0x3e, 0x3a, 0xb5, 0xd3, 0xd6, 0x76, 0x4c, 0x61, 0xef, 0xb9, 0x93, 0x02,
                0xbd, 0x5b
            ]
        );
        assert_eq!(
            bytes_to_ulid(&ulid_to_bytes(&SAMPLE_ULID.to_lowercase()).unwrap()).unwrap(),
            SAMPLE_ULID
        );
        assert!(ulid_to_bytes("").is_err());
        assert!(ulid_to_bytes(&"0".repeat(25)).is_err());
        assert!(ulid_to_bytes(&"0".repeat(27)).is_err());
        assert!(ulid_to_bytes(&format!("U{}", &zero[1..])).is_err());
        assert!(ulid_to_bytes(&format!("L{}", &zero[1..])).is_err());
        assert!(ulid_to_bytes(&format!("8{}", "0".repeat(25))).is_err());
        assert!(bytes_to_ulid(&[0; 15]).is_err());
        assert!(bytes_to_ulid(&[0; 17]).is_err());
        assert!(!is_tagged_string(&zero));
        let mut tagged_bytes = [0; ULID_BYTE_LENGTH];
        tagged_bytes[0] = TAG_BIT_MASK;
        let tagged = bytes_to_ulid(&tagged_bytes).unwrap();
        assert_eq!(tagged, "40000000000000000000000000");
        assert!(is_tagged_string(&tagged));
        assert!(is_tagged_string(&format!("4{}", "0".repeat(25))));
        assert!(is_tagged_string(&format!("7{}", "Z".repeat(25))));
        assert!(!is_tagged_string(&format!("4{}", "U".repeat(25))));
        assert!(!is_tagged_string(&format!("8{}", "0".repeat(25))));
    }

    fn assert_run_id_index_contract() {
        let tagged = encode(SAMPLE_ULID, "iad1", None).unwrap();
        assert_eq!(tagged, "41ARZ3NDEK0GV4RRFFQ69G5FAV");
        assert!(is_tagged_string(&tagged));
        let decoded = decode(&tagged).unwrap();
        assert_eq!(decoded.region, Some("iad1"));
        assert_eq!(decoded.region_id, Some(1));
        assert_eq!(decoded.version, Some(CURRENT_VERSION));
        assert_eq!(decoded.ulid, "01ARZ3NDEK0GV4RRFFQ69G5FAV");
        assert_eq!(
            encode(SAMPLE_ULID, 7_u8, None).unwrap(),
            "41ARZ3NDEK3GV4RRFFQ69G5FAV"
        );
        assert_eq!(
            decode(&encode(SAMPLE_ULID, 63_u8, None).unwrap())
                .unwrap()
                .region,
            None
        );
        assert_eq!(
            decode(&encode(SAMPLE_ULID, 0_u8, None).unwrap())
                .unwrap()
                .region_id,
            Some(0)
        );
        assert_eq!(
            encode(SAMPLE_ULID, "iad1", Some(0)).unwrap(),
            "41ARZ3NDEK0GB4RRFFQ69G5FAV"
        );
        assert_eq!(
            encode(SAMPLE_ULID, "iad1", Some(MAX_VERSION)).unwrap(),
            "41ARZ3NDEK0ZV4RRFFQ69G5FAV"
        );
        for region in [0_u8, 1, 17, 31, 32, 63] {
            for version in [0_u8, 1, 7, 16, 31] {
                let tagged = encode(SAMPLE_ULID, region, Some(version)).unwrap();
                let decoded = decode(&tagged).unwrap();
                assert_eq!(decoded.region_id, Some(region));
                assert_eq!(decoded.version, Some(version));
                assert_eq!(
                    encode(&decoded.ulid, region, Some(version)).unwrap(),
                    tagged
                );
            }
        }
        assert_eq!(
            encode("0".repeat(26).as_str(), 0_u8, Some(0)).unwrap(),
            "40000000000000000000000000"
        );
        assert_eq!(
            encode("0".repeat(26).as_str(), 1_u8, Some(1)).unwrap(),
            "40000000000GG0000000000000"
        );
        assert_eq!(
            encode("0".repeat(26).as_str(), 63_u8, Some(31)).unwrap(),
            "4000000000ZZG0000000000000"
        );
        assert_eq!(
            encode("7ZZZZZZZZZZZZZZZZZZZZZZZZZ", 0_u8, Some(0)).unwrap(),
            "7ZZZZZZZZZ00FZZZZZZZZZZZZZ"
        );
        let plain = decode(SAMPLE_ULID).unwrap();
        assert!(!plain.tagged);
        assert_eq!(plain.region_id, None);
        assert_eq!(plain.version, None);
        assert_eq!(plain.region, None);
        assert!(encode("not-a-ulid", "iad1", None).is_err());
        assert!(encode(SAMPLE_ULID, "xxx1", None).is_err());
        assert!(encode(SAMPLE_ULID, "unknown", None).is_err());
        assert!(encode(SAMPLE_ULID, "iad1", Some(32)).is_err());
        assert_eq!(REGION_IDS.len(), 24);
        let ids = REGION_IDS
            .iter()
            .map(|(_, id)| *id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), REGION_IDS.len());
        for (code, id) in REGION_IDS {
            assert!(*id <= MAX_REGION_ID);
            if *code != "unknown" {
                assert_eq!(
                    decode(&encode(SAMPLE_ULID, *code, None).unwrap())
                        .unwrap()
                        .region,
                    Some(*code)
                );
            }
        }
        assert!("40000000000000000000000000" > "33333333333333333333333333");
        assert!(
            encode("01ARZ3NDEKTSV4RRFFQ69G5FAV", "iad1", None).unwrap()
                < encode("01ARZ3NDEMTSV4RRFFQ69G5FAV", "iad1", None).unwrap()
        );
        assert!(
            encode("01ARZ3NDEKTSV4RRFFQ69G5FAV", "iad1", None).unwrap()
                < encode("01ARZ3NDEKTSV4RRFFQ69G5FAW", "iad1", None).unwrap()
        );
    }

    fn assert_streamer_contract() {
        assert!(encode_multi_chunks([]).is_empty());
        assert_eq!(
            decode_multi_chunks(&encode_multi_chunks(["hello".into()])).unwrap(),
            vec![b"hello".to_vec()]
        );
        assert_eq!(
            decode_multi_chunks(&encode_multi_chunks([vec![1, 2, 3, 4, 5].into()])).unwrap(),
            vec![vec![1, 2, 3, 4, 5]]
        );
        assert_eq!(
            decode_multi_chunks(&encode_multi_chunks([
                "hello".into(),
                "world".into(),
                "test".into()
            ]))
            .unwrap(),
            vec![b"hello".to_vec(), b"world".to_vec(), b"test".to_vec()]
        );
        assert_eq!(
            decode_multi_chunks(&encode_multi_chunks([
                vec![1, 2, 3].into(),
                vec![4, 5].into(),
                vec![6, 7, 8, 9].into(),
            ]))
            .unwrap(),
            vec![vec![1, 2, 3], vec![4, 5], vec![6, 7, 8, 9]]
        );
        assert_eq!(
            decode_multi_chunks(&encode_multi_chunks([
                "hello".into(),
                vec![1, 2, 3].into(),
                "world".into()
            ]))
            .unwrap()[1],
            vec![1, 2, 3]
        );
        assert_eq!(
            decode_multi_chunks(&encode_multi_chunks(["".into(), "hello".into(), "".into()]))
                .unwrap()[0]
                .len(),
            0
        );
        let encoded = encode_multi_chunks(["hello".into(), "world".into()]);
        assert_eq!(encoded.len(), 18);
        assert_eq!(&encoded[0..4], &[0, 0, 0, 5]);
        let large = (0..10 * 1024).map(|i| (i % 256) as u8).collect::<Vec<_>>();
        assert_eq!(
            decode_multi_chunks(&encode_multi_chunks([large.clone().into()])).unwrap()[0],
            large
        );
        let many = (0..100)
            .map(|i| format!("chunk{i}").into())
            .collect::<Vec<StreamChunk>>();
        assert_eq!(
            decode_multi_chunks(&encode_multi_chunks(many))
                .unwrap()
                .len(),
            100
        );
        assert_eq!(
            String::from_utf8(
                decode_multi_chunks(&encode_multi_chunks([
                    "hello".into(),
                    "世界".into(),
                    "rocket".into()
                ]))
                .unwrap()[1]
                    .clone()
            )
            .unwrap(),
            "世界"
        );
        assert_eq!(
            stream_url("https://test.example.com", "run-123", "my-stream", None),
            "https://test.example.com/v2/runs/run-123/stream/my-stream"
        );
        assert_eq!(
            stream_url("https://test.example.com", "run-123", "my-stream", Some(5)),
            "https://test.example.com/v2/runs/run-123/stream/my-stream?startIndex=5"
        );
        let error = stream_request_error(
            "write",
            "https://test.example.com/v2/runs/wrun_test/stream/user",
            500,
            &headers([
                ("x-vercel-id", "sfo1::abc"),
                ("x-vercel-error", "FUNCTION_INVOCATION_FAILED"),
            ]),
            "Internal Server Error\nrequest-token",
        );
        assert!(error.contains("PUT https://test.example.com/v2/runs/wrun_test/stream/user"));
        assert!(error.contains("x-vercel-id=sfo1::abc"));
        assert_eq!(
            write_multi_page_counts(MAX_CHUNKS_PER_REQUEST),
            vec![MAX_CHUNKS_PER_REQUEST]
        );
        assert_eq!(
            write_multi_page_counts(MAX_CHUNKS_PER_REQUEST + 1),
            vec![MAX_CHUNKS_PER_REQUEST, 1]
        );
        assert_eq!(
            write_multi_page_counts(MAX_CHUNKS_PER_REQUEST * 2 + 5),
            vec![MAX_CHUNKS_PER_REQUEST, MAX_CHUNKS_PER_REQUEST, 5]
        );
    }

    fn assert_utils_contract() {
        let env = VercelEnv::default();
        assert_eq!(
            get_http_url(None, &env),
            HttpUrl {
                base_url: "https://vercel-workflow.com/api".into(),
                using_proxy: false
            }
        );
        let env = VercelEnv {
            vercel_workflow_server_url: Some("https://custom-host.example.com".into()),
            ..VercelEnv::default()
        };
        assert_eq!(
            get_http_url(None, &env).base_url,
            "https://custom-host.example.com/api"
        );
        let config = ApiConfig {
            project_config: Some(ProjectConfig {
                project_id: Some("prj_123".into()),
                team_id: Some("team_456".into()),
                environment: Some("preview".into()),
                ..ProjectConfig::default()
            }),
            ..ApiConfig::default()
        };
        assert_eq!(
            get_http_url(Some(&config), &VercelEnv::default()).base_url,
            "https://api.vercel.com/v1/workflow"
        );
        let env = VercelEnv {
            workflow_vercel_backend_url: Some("https://proxy.example.com/v1".into()),
            vercel_workflow_server_url: Some("https://custom.example.com".into()),
            ..VercelEnv::default()
        };
        assert_eq!(
            get_http_url(Some(&config), &env).base_url,
            "https://proxy.example.com/v1"
        );
        let direct_headers = get_headers(None, &env, false);
        assert!(!direct_headers.contains_key("x-vercel-trusted-oidc-idp-token"));
        assert!(!direct_headers.contains_key("x-vercel-workflow-api-url"));
        let proxy_headers = get_headers(Some(&config), &env, true);
        assert_eq!(
            proxy_headers.get("x-vercel-workflow-api-url"),
            Some(&"https://custom.example.com".into())
        );
        assert_eq!(
            proxy_headers.get("x-vercel-project-id"),
            Some(&"prj_123".into())
        );
        assert_eq!(
            proxy_headers.get("x-vercel-team-id"),
            Some(&"team_456".into())
        );
        assert_eq!(
            proxy_headers.get("x-vercel-environment"),
            Some(&"preview".into())
        );
        assert!(get_http_config(Some(&config), &env).is_err());
        let authed = ApiConfig {
            token: Some("my-vercel-auth-token".into()),
            ..config
        };
        let http = get_http_config(Some(&authed), &env).unwrap();
        assert_eq!(
            http.headers.get("Authorization"),
            Some(&"Bearer my-vercel-auth-token".into())
        );
        assert!(!http.headers.contains_key("x-vercel-trusted-oidc-idp-token"));
        assert_eq!(
            list_runs_endpoint(
                Some("wf"),
                Some("running"),
                Some(10),
                Some("cursor"),
                Some("desc"),
                Some("none")
            ),
            "/v2/runs?workflowName=wf&status=running&limit=10&cursor=cursor&sortOrder=desc&remoteRefBehavior=lazy"
        );
        assert_eq!(
            get_run_endpoint("run/1", Some("all")),
            "/v2/runs/run%2F1?remoteRefBehavior=resolve"
        );
        assert_eq!(
            step_endpoint("run", Some("step id"), Some("none")),
            "/v2/runs/run/steps/step%20id?remoteRefBehavior=lazy"
        );
    }

    parity_test!(
        vercel_src_encryption_l23,
        assert_encryption_derivation_contract
    );
    parity_test!(
        vercel_src_encryption_l29,
        assert_encryption_derivation_contract
    );
    parity_test!(
        vercel_src_encryption_l43,
        assert_encryption_derivation_contract
    );
    parity_test!(
        vercel_src_encryption_l57,
        assert_encryption_derivation_contract
    );
    parity_test!(
        vercel_src_encryption_l71,
        assert_encryption_derivation_contract
    );
    parity_test!(
        vercel_src_encryption_l84,
        assert_encryption_derivation_contract
    );
    parity_test!(
        vercel_src_encryption_l90,
        assert_encryption_derivation_contract
    );
    parity_test!(vercel_src_encryption_l105, assert_fetch_run_key_contract);
    parity_test!(vercel_src_encryption_l117, assert_fetch_run_key_contract);
    parity_test!(vercel_src_encryption_l131, assert_fetch_run_key_contract);

    parity_test!(vercel_src_queue_l59, assert_queue_send_contract);
    parity_test!(vercel_src_queue_l85, assert_queue_send_contract);
    parity_test!(vercel_src_queue_l105, assert_queue_send_contract);
    parity_test!(vercel_src_queue_l131, assert_queue_send_contract);
    parity_test!(vercel_src_queue_l155, assert_queue_send_contract);
    parity_test!(vercel_src_queue_l184, assert_queue_send_contract);
    parity_test!(vercel_src_queue_l213, assert_queue_send_contract);
    parity_test!(vercel_src_queue_l248, assert_queue_send_contract);
    parity_test!(vercel_src_queue_l274, assert_queue_send_contract);
    parity_test!(vercel_src_queue_l309, assert_queue_send_contract);
    parity_test!(vercel_src_queue_l349, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l361, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l413, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l462, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l511, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l560, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l590, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l608, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l633, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l666, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l703, assert_queue_handler_contract);
    parity_test!(vercel_src_queue_l735, assert_queue_handler_contract);

    parity_test!(
        vercel_src_resolve_latest_deployment_l32,
        assert_resolve_latest_contract
    );
    parity_test!(
        vercel_src_resolve_latest_deployment_l65,
        assert_resolve_latest_contract
    );
    parity_test!(
        vercel_src_resolve_latest_deployment_l77,
        assert_resolve_latest_contract
    );
    parity_test!(
        vercel_src_resolve_latest_deployment_l89,
        assert_resolve_latest_contract
    );
    parity_test!(
        vercel_src_resolve_latest_deployment_l123,
        assert_resolve_latest_contract
    );
    parity_test!(
        vercel_src_resolve_latest_deployment_l161,
        assert_resolve_latest_contract
    );
    parity_test!(
        vercel_src_resolve_latest_deployment_l171,
        assert_resolve_latest_contract
    );
    parity_test!(
        vercel_src_resolve_latest_deployment_l183,
        assert_resolve_latest_contract
    );

    parity_test!(vercel_src_run_id_codec_l25, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l31, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l37, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l49, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l54, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l60, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l78, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l89, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l97, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l108, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l112, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l122, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l131, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l142, assert_run_id_codec_contract);
    parity_test!(vercel_src_run_id_codec_l151, assert_run_id_codec_contract);

    parity_test!(vercel_src_run_id_index_l24, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l40, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l49, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l57, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l65, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l75, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l90, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l114, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l134, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l140, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l166, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l173, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l186, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l210, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l218, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l224, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l231, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l245, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l275, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l284, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l295, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l305, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l313, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l327, assert_run_id_index_contract);
    parity_test!(vercel_src_run_id_index_l347, assert_run_id_index_contract);

    parity_test!(vercel_src_streamer_l27, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l32, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l40, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l49, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l59, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l74, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l88, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l98, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l112, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l120, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l136, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l150, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l161, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l193, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l208, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l234, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l286, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l302, assert_streamer_contract);
    parity_test!(vercel_src_streamer_l320, assert_streamer_contract);

    parity_test!(vercel_src_utils_l21, assert_utils_contract);
    parity_test!(vercel_src_utils_l28, assert_utils_contract);
    parity_test!(vercel_src_utils_l36, assert_utils_contract);
    parity_test!(vercel_src_utils_l47, assert_utils_contract);
    parity_test!(vercel_src_utils_l73, assert_utils_contract);
    parity_test!(vercel_src_utils_l79, assert_utils_contract);
    parity_test!(vercel_src_utils_l84, assert_utils_contract);
    parity_test!(vercel_src_utils_l92, assert_utils_contract);
    parity_test!(vercel_src_utils_l99, assert_utils_contract);
    parity_test!(vercel_src_utils_l130, assert_utils_contract);
    parity_test!(vercel_src_utils_l138, assert_utils_contract);
}
