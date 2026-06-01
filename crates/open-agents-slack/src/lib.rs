//! Slack HTTP ingress for the Rust Open Agents remote-agent service.
//!
//! The Open Agents web app starts or reconnects to a durable workflow from its
//! chat route, while the sessions route resolves the owning chat/session. This
//! crate provides the Slack-facing equivalent boundary: verify the raw request,
//! parse it through the chat-sdk Slack adapter helpers, dedupe Slack retries,
//! derive the `slack:<channel>:<thread_ts>` route, and hand the event to a
//! durable run router.

#![forbid(unsafe_code)]

use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use chat_sdk_adapter_slack::encode_thread_id;
use chat_sdk_adapter_slack::webhook::{
    SlackAction, SlackBlockActionsPayload, SlackDirectMessagePayload, SlackEventBase,
    SlackParseOptions, SlackRetry, SlackSlashCommandPayload, SlackVerifyOptions,
    SlackViewClosedPayload, SlackViewSubmissionPayload, SlackWebhookPayload,
    SlackWebhookVerificationError, get_header, parse_slack_webhook_body, verify_slack_signature,
};
use chat_sdk_chat::types::{StateAdapter, StateAdapterError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

/// Bucket that owns the first Slack ingress implementation.
pub const INGRESS_OWNER_BUCKET: u8 = 10;

/// Bucket that owns Slack outbound messages and interactions.
pub const OUTBOUND_OWNER_BUCKET: u8 = 11;

/// Bucket that owns Slack session lifecycle behavior.
pub const SESSION_OWNER_BUCKET: u8 = 12;

/// Bot OAuth token environment variable.
pub const SLACK_BOT_TOKEN_ENV: &str = "SLACK_BOT_TOKEN";

/// Request signing secret environment variable.
pub const SLACK_SIGNING_SECRET_ENV: &str = "SLACK_SIGNING_SECRET";

/// Optional Socket Mode app token environment variable.
pub const SLACK_APP_TOKEN_ENV: &str = "SLACK_APP_TOKEN";

/// Optional live-test channel id environment variable.
pub const SLACK_TEST_CHANNEL_ID_ENV: &str = "SLACK_TEST_CHANNEL_ID";

/// Optional live-test user id environment variable.
pub const SLACK_TEST_USER_ID_ENV: &str = "SLACK_TEST_USER_ID";

/// Slack thread identity used to route one Slack conversation to one session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackThreadAddress {
    /// Slack team/workspace id, when present in the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// Slack channel id.
    pub channel_id: String,
    /// Parent thread timestamp. Top-level messages use their own `ts`.
    pub thread_ts: String,
}

impl SlackThreadAddress {
    /// Creates a Slack thread address from channel and thread timestamp.
    pub fn new(channel_id: impl Into<String>, thread_ts: impl Into<String>) -> Self {
        Self {
            team_id: None,
            channel_id: channel_id.into(),
            thread_ts: thread_ts.into(),
        }
    }

    /// Attaches a Slack team/workspace id.
    pub fn with_team_id(mut self, team_id: impl Into<String>) -> Self {
        self.team_id = Some(team_id.into());
        self
    }

    /// Returns the `chat-sdk-adapter-slack` thread id shape.
    pub fn chat_thread_id(&self) -> String {
        encode_thread_id(&self.channel_id, &self.thread_ts)
    }
}

/// Slack delivery mode selected by service configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackIngressMode {
    /// HTTP webhook/Event API delivery.
    Webhook,
    /// Slack Socket Mode delivery.
    SocketMode,
}

/// Default retry-dedupe TTL. Mirrors chat-sdk's five-minute inbound-message
/// dedupe window and Slack's retry horizon.
pub const DEFAULT_SLACK_INGRESS_DEDUPE_TTL_MS: u64 = 5 * 60 * 1000;

/// Slack HTTP route being handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlackIngressRoute {
    /// Slack Events API endpoint.
    EventsApi,
    /// Slack slash-command endpoint.
    SlashCommand,
    /// Slack interactive payload endpoint.
    Interactions,
}

impl SlackIngressRoute {
    fn as_str(self) -> &'static str {
        match self {
            Self::EventsApi => "events_api",
            Self::SlashCommand => "slash_command",
            Self::Interactions => "interactions",
        }
    }
}

/// Configuration for [`SlackIngress`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackIngressOptions {
    /// Slack signing secret.
    pub signing_secret: String,
    /// Maximum allowed request timestamp skew in seconds. Defaults to 300.
    pub max_skew_seconds: Option<u64>,
    /// Test-only current time override for signature checks.
    pub now_seconds: Option<u64>,
    /// TTL for retry/idempotency markers in the state adapter.
    pub dedupe_ttl_ms: u64,
}

impl SlackIngressOptions {
    /// Construct options with the default dedupe TTL.
    pub fn new(signing_secret: impl Into<String>) -> Self {
        Self {
            signing_secret: signing_secret.into(),
            max_skew_seconds: None,
            now_seconds: None,
            dedupe_ttl_ms: DEFAULT_SLACK_INGRESS_DEDUPE_TTL_MS,
        }
    }

    /// Set the max signature skew.
    pub fn with_max_skew_seconds(mut self, max_skew_seconds: u64) -> Self {
        self.max_skew_seconds = Some(max_skew_seconds);
        self
    }

    /// Set the current-time override used by tests.
    pub fn with_now_seconds(mut self, now_seconds: u64) -> Self {
        self.now_seconds = Some(now_seconds);
        self
    }

    /// Set the retry-dedupe TTL.
    pub fn with_dedupe_ttl_ms(mut self, dedupe_ttl_ms: u64) -> Self {
        self.dedupe_ttl_ms = dedupe_ttl_ms;
        self
    }
}

/// Raw HTTP request data passed by an outer web framework.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackHttpRequest {
    /// Exact UTF-8 request body used for Slack signature verification.
    pub body: String,
    /// Header names and values. Lookups are case-insensitive.
    pub headers: Vec<(String, String)>,
}

impl SlackHttpRequest {
    /// Construct from the raw body.
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            headers: Vec::new(),
        }
    }

    /// Add one header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    fn header(&self, name: &str) -> Option<&str> {
        get_header(&self.headers, name)
    }
}

/// HTTP response data returned to the outer web framework.
#[derive(Debug, Clone, PartialEq)]
pub struct SlackHttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body.
    pub body: String,
    /// Optional response content type.
    pub content_type: Option<String>,
    /// Structured outcome for tests and embedders.
    pub outcome: SlackIngressOutcome,
}

impl SlackHttpResponse {
    fn text(status: u16, body: impl Into<String>, outcome: SlackIngressOutcome) -> Self {
        Self {
            status,
            body: body.into(),
            content_type: Some("text/plain; charset=utf-8".to_string()),
            outcome,
        }
    }

    fn empty(status: u16, outcome: SlackIngressOutcome) -> Self {
        Self {
            status,
            body: String::new(),
            content_type: None,
            outcome,
        }
    }

    fn json(status: u16, body: serde_json::Value, outcome: SlackIngressOutcome) -> Self {
        Self {
            status,
            body: serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string()),
            content_type: Some("application/json".to_string()),
            outcome,
        }
    }
}

/// Structured result of processing a Slack ingress request.
#[derive(Debug, Clone, PartialEq)]
pub enum SlackIngressOutcome {
    /// Events API URL verification challenge was returned.
    UrlVerified,
    /// A Slack app mention, DM, or slash command was handed to the run router.
    StartOrResume {
        dedupe_key: String,
        handoff: SlackRunHandoff,
    },
    /// A Slack interaction was handed to the run router.
    ResumeInteraction {
        dedupe_key: String,
        handoff: SlackRunHandoff,
    },
    /// The request was a retry or duplicate and was acknowledged without work.
    Duplicate { dedupe_key: String },
    /// Payload was valid but intentionally ignored.
    Ignored { reason: String },
    /// Payload was valid but not yet supported by this ingress service.
    Unsupported { payload_type: String },
    /// Signature verification failed.
    SignatureRejected {
        error: SlackWebhookVerificationError,
    },
    /// Body parsing failed.
    ParseRejected { error: String },
    /// Dedupe state failed.
    StateError { error: String },
    /// Durable run handoff failed.
    RouterError { error: String },
}

/// Result returned by a durable run router.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackRunHandoff {
    /// Durable run id, when the runtime has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Whether the call resumed an existing run.
    pub resumed: bool,
    /// Runtime-specific metadata for logs/tests.
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl SlackRunHandoff {
    /// Construct a handoff result.
    pub fn new(run_id: Option<String>, resumed: bool) -> Self {
        Self {
            run_id,
            resumed,
            metadata: serde_json::Value::Null,
        }
    }

    /// Attach metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Slack retry headers decoded into a serializable shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackRetryInfo {
    pub num: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl From<&SlackRetry> for SlackRetryInfo {
    fn from(value: &SlackRetry) -> Self {
        Self {
            num: value.num,
            reason: value.reason.clone(),
        }
    }
}

/// Resolved Slack route target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackRouteTarget {
    /// Platform channel id (`C...`, `G...`, or `D...`).
    pub channel_id: String,
    /// Slack thread timestamp used as the chat-sdk thread key.
    pub thread_ts: String,
    /// chat-sdk Slack thread id (`slack:<channel_id>:<thread_ts>`).
    pub slack_thread_id: String,
    /// Slack team id, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// Slack enterprise id, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_id: Option<String>,
}

impl SlackRouteTarget {
    fn new(
        channel_id: impl Into<String>,
        thread_ts: impl Into<String>,
        team_id: Option<String>,
        enterprise_id: Option<String>,
    ) -> Self {
        let channel_id = channel_id.into();
        let thread_ts = thread_ts.into();
        let slack_thread_id = encode_thread_id(&channel_id, &thread_ts);
        Self {
            channel_id,
            thread_ts,
            slack_thread_id,
            team_id,
            enterprise_id,
        }
    }
}

/// Source that started or resumed a durable run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackRunStartSource {
    AppMention,
    DirectMessage,
    SlashCommand,
}

/// Request passed to the durable run runtime for message-like ingress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackRunStartRequest {
    pub source: SlackRunStartSource,
    pub target: SlackRouteTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_ts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<SlackRetryInfo>,
    pub raw: serde_json::Value,
}

/// Slack interaction kind handed to the durable run runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackInteractionKind {
    BlockActions,
    ViewSubmission,
    ViewClosed,
}

/// Slack action entry normalized for durable resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackInteractionAction {
    pub action_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_option_value: Option<String>,
    #[serde(rename = "type")]
    pub action_type: String,
}

impl From<&SlackAction> for SlackInteractionAction {
    fn from(value: &SlackAction) -> Self {
        Self {
            action_id: value.action_id.clone(),
            block_id: value.block_id.clone(),
            value: value.value.clone(),
            selected_option_value: value.selected_option_value.clone(),
            action_type: value.r#type.clone(),
        }
    }
}

/// Request passed to the durable run runtime for interactive payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackInteractionRequest {
    pub kind: SlackInteractionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<SlackRouteTarget>,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_ts: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<SlackInteractionAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_callback_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_private_metadata: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<SlackRetryInfo>,
    pub raw: serde_json::Value,
}

/// Error-producing result used by [`SlackRunRouter`].
pub type SlackRunRouterResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Durable run handoff boundary.
#[async_trait]
pub trait SlackRunRouter: Send + Sync {
    /// Start or reconnect a durable run for an app mention, DM, or slash
    /// command.
    async fn start_or_resume(
        &self,
        request: SlackRunStartRequest,
    ) -> SlackRunRouterResult<SlackRunHandoff>;

    /// Resume a paused durable run from a Slack interaction.
    async fn resume_interaction(
        &self,
        request: SlackInteractionRequest,
    ) -> SlackRunRouterResult<SlackRunHandoff>;
}

/// Slack ingress service.
pub struct SlackIngress {
    options: SlackIngressOptions,
    state: Arc<dyn StateAdapter>,
    router: Arc<dyn SlackRunRouter>,
}

impl std::fmt::Debug for SlackIngress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlackIngress")
            .field("options", &self.options)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl SlackIngress {
    /// Construct a Slack ingress service.
    pub fn new(
        options: SlackIngressOptions,
        state: Arc<dyn StateAdapter>,
        router: Arc<dyn SlackRunRouter>,
    ) -> Self {
        Self {
            options,
            state,
            router,
        }
    }

    /// Handle a route-selected Slack request.
    pub async fn handle(
        &self,
        route: SlackIngressRoute,
        request: SlackHttpRequest,
    ) -> SlackHttpResponse {
        if let Err(error) = self.verify_request(&request) {
            if route == SlackIngressRoute::EventsApi
                && error == SlackWebhookVerificationError::MissingHeaders
                && let Some(response) = url_verification_response(&request)
            {
                return response;
            }
            return SlackHttpResponse::text(
                401,
                error.to_string(),
                SlackIngressOutcome::SignatureRejected { error },
            );
        }

        let content_type = request.header("content-type");
        let parse_options = SlackParseOptions {
            content_type,
            headers: Some(&request.headers),
        };
        let payload = match parse_slack_webhook_body(&request.body, &parse_options) {
            Ok(payload) => payload,
            Err(error) => {
                return SlackHttpResponse::text(
                    400,
                    error.to_string(),
                    SlackIngressOutcome::ParseRejected {
                        error: error.to_string(),
                    },
                );
            }
        };

        match (route, payload) {
            (_, SlackWebhookPayload::UrlVerification(payload)) => {
                SlackHttpResponse::text(200, payload.challenge, SlackIngressOutcome::UrlVerified)
            }
            (SlackIngressRoute::EventsApi, SlackWebhookPayload::AppMention(payload)) => {
                self.start_or_resume(
                    start_request_from_event(SlackRunStartSource::AppMention, &payload.base),
                    route,
                )
                .await
            }
            (SlackIngressRoute::EventsApi, SlackWebhookPayload::DirectMessage(payload)) => {
                if should_ignore_direct_message(&payload) {
                    return SlackHttpResponse::empty(
                        200,
                        SlackIngressOutcome::Ignored {
                            reason: "direct message from bot or unsupported subtype".to_string(),
                        },
                    );
                }
                self.start_or_resume(
                    start_request_from_event(SlackRunStartSource::DirectMessage, &payload.base),
                    route,
                )
                .await
            }
            (SlackIngressRoute::SlashCommand, SlackWebhookPayload::SlashCommand(payload)) => {
                self.start_or_resume(start_request_from_slash_command(&payload), route)
                    .await
            }
            (SlackIngressRoute::Interactions, SlackWebhookPayload::BlockActions(payload)) => {
                self.resume_interaction(interaction_request_from_block_actions(&payload), route)
                    .await
            }
            (SlackIngressRoute::Interactions, SlackWebhookPayload::ViewSubmission(payload)) => {
                self.resume_interaction(interaction_request_from_view_submission(&payload), route)
                    .await
            }
            (SlackIngressRoute::Interactions, SlackWebhookPayload::ViewClosed(payload)) => {
                self.resume_interaction(interaction_request_from_view_closed(&payload), route)
                    .await
            }
            (SlackIngressRoute::Interactions, SlackWebhookPayload::BlockSuggestion(_)) => {
                SlackHttpResponse::json(
                    200,
                    json!({ "options": [] }),
                    SlackIngressOutcome::Ignored {
                        reason: "block suggestion has no durable-run handoff yet".to_string(),
                    },
                )
            }
            (_, SlackWebhookPayload::Unsupported(payload)) => SlackHttpResponse::empty(
                200,
                SlackIngressOutcome::Unsupported {
                    payload_type: payload.r#type,
                },
            ),
            (_, payload) => SlackHttpResponse::empty(
                200,
                SlackIngressOutcome::Unsupported {
                    payload_type: payload.kind().to_string(),
                },
            ),
        }
    }

    /// Handle the Slack Events API route.
    pub async fn handle_events_api(&self, request: SlackHttpRequest) -> SlackHttpResponse {
        self.handle(SlackIngressRoute::EventsApi, request).await
    }

    /// Handle the Slack slash-command route.
    pub async fn handle_slash_command(&self, request: SlackHttpRequest) -> SlackHttpResponse {
        self.handle(SlackIngressRoute::SlashCommand, request).await
    }

    /// Handle the Slack interactions route.
    pub async fn handle_interactions(&self, request: SlackHttpRequest) -> SlackHttpResponse {
        self.handle(SlackIngressRoute::Interactions, request).await
    }

    fn verify_request(
        &self,
        request: &SlackHttpRequest,
    ) -> Result<(), SlackWebhookVerificationError> {
        let options = SlackVerifyOptions {
            signing_secret: self.options.signing_secret.clone(),
            max_skew_seconds: self.options.max_skew_seconds,
            now_seconds: self.options.now_seconds,
        };
        verify_slack_signature(
            &request.body,
            request.header("x-slack-request-timestamp"),
            request.header("x-slack-signature"),
            &options,
        )
    }

    async fn start_or_resume(
        &self,
        request: SlackRunStartRequest,
        route: SlackIngressRoute,
    ) -> SlackHttpResponse {
        let dedupe_key = dedupe_key_for_start(&request);
        match self
            .claim_dedupe(&dedupe_key, route, request.retry.as_ref())
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                return SlackHttpResponse::empty(
                    200,
                    SlackIngressOutcome::Duplicate { dedupe_key },
                );
            }
            Err(error) => {
                return state_error_response(error);
            }
        }

        match self.router.start_or_resume(request).await {
            Ok(handoff) => SlackHttpResponse::empty(
                200,
                SlackIngressOutcome::StartOrResume {
                    dedupe_key,
                    handoff,
                },
            ),
            Err(error) => SlackHttpResponse::text(
                500,
                error.to_string(),
                SlackIngressOutcome::RouterError {
                    error: error.to_string(),
                },
            ),
        }
    }

    async fn resume_interaction(
        &self,
        request: SlackInteractionRequest,
        route: SlackIngressRoute,
    ) -> SlackHttpResponse {
        let dedupe_key = dedupe_key_for_interaction(&request);
        match self
            .claim_dedupe(&dedupe_key, route, request.retry.as_ref())
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                return SlackHttpResponse::empty(
                    200,
                    SlackIngressOutcome::Duplicate { dedupe_key },
                );
            }
            Err(error) => {
                return state_error_response(error);
            }
        }

        match self.router.resume_interaction(request).await {
            Ok(handoff) => SlackHttpResponse::empty(
                200,
                SlackIngressOutcome::ResumeInteraction {
                    dedupe_key,
                    handoff,
                },
            ),
            Err(error) => SlackHttpResponse::text(
                500,
                error.to_string(),
                SlackIngressOutcome::RouterError {
                    error: error.to_string(),
                },
            ),
        }
    }

    async fn claim_dedupe(
        &self,
        dedupe_key: &str,
        route: SlackIngressRoute,
        retry: Option<&SlackRetryInfo>,
    ) -> Result<bool, StateAdapterError> {
        let value = json!({
            "route": route.as_str(),
            "retry": retry,
        });
        self.state
            .set_if_not_exists(dedupe_key, value, Some(self.options.dedupe_ttl_ms))
            .await
    }
}

fn url_verification_response(request: &SlackHttpRequest) -> Option<SlackHttpResponse> {
    let parse_options = SlackParseOptions {
        content_type: request.header("content-type"),
        headers: Some(&request.headers),
    };
    match parse_slack_webhook_body(&request.body, &parse_options).ok()? {
        SlackWebhookPayload::UrlVerification(payload) => Some(SlackHttpResponse::text(
            200,
            payload.challenge,
            SlackIngressOutcome::UrlVerified,
        )),
        _ => None,
    }
}

fn state_error_response(error: StateAdapterError) -> SlackHttpResponse {
    SlackHttpResponse::text(
        500,
        error.to_string(),
        SlackIngressOutcome::StateError {
            error: error.to_string(),
        },
    )
}

fn start_request_from_event(
    source: SlackRunStartSource,
    base: &SlackEventBase,
) -> SlackRunStartRequest {
    SlackRunStartRequest {
        source,
        target: SlackRouteTarget::new(
            &base.channel_id,
            &base.thread_ts,
            base.team_id.clone(),
            base.enterprise_id.clone(),
        ),
        user_id: base.user_id.clone(),
        text: base.text.clone(),
        message_ts: Some(base.ts.clone()),
        event_id: base.event_id.clone(),
        command: None,
        response_url: None,
        trigger_id: None,
        retry: base.retry.as_ref().map(SlackRetryInfo::from),
        raw: base.raw.clone(),
    }
}

fn start_request_from_slash_command(payload: &SlackSlashCommandPayload) -> SlackRunStartRequest {
    SlackRunStartRequest {
        source: SlackRunStartSource::SlashCommand,
        target: SlackRouteTarget::new(
            &payload.channel_id,
            "",
            payload.team_id.clone(),
            payload.enterprise_id.clone(),
        ),
        user_id: Some(payload.user_id.clone()),
        text: payload.text.clone(),
        message_ts: None,
        event_id: None,
        command: Some(payload.command.clone()),
        response_url: payload.response_url.clone(),
        trigger_id: payload.trigger_id.clone(),
        retry: payload.retry.as_ref().map(SlackRetryInfo::from),
        raw: serde_json::to_value(&payload.raw).unwrap_or_else(|_| json!({})),
    }
}

fn interaction_request_from_block_actions(
    payload: &SlackBlockActionsPayload,
) -> SlackInteractionRequest {
    SlackInteractionRequest {
        kind: SlackInteractionKind::BlockActions,
        target: payload.continuation.as_ref().map(|continuation| {
            SlackRouteTarget::new(
                &continuation.channel_id,
                &continuation.thread_ts,
                continuation.team_id.clone(),
                continuation.enterprise_id.clone(),
            )
        }),
        user_id: payload.user_id.clone(),
        trigger_id: payload.trigger_id.clone(),
        response_url: payload.response_url.clone(),
        message_ts: payload.message_ts.clone(),
        actions: payload
            .actions
            .iter()
            .map(SlackInteractionAction::from)
            .collect(),
        view_id: None,
        view_callback_id: None,
        view_private_metadata: None,
        retry: payload.retry.as_ref().map(SlackRetryInfo::from),
        raw: payload.raw.clone(),
    }
}

fn interaction_request_from_view_submission(
    payload: &SlackViewSubmissionPayload,
) -> SlackInteractionRequest {
    let view = serde_json::Value::Object(payload.view.clone());
    SlackInteractionRequest {
        kind: SlackInteractionKind::ViewSubmission,
        target: None,
        user_id: payload.user_id.clone(),
        trigger_id: None,
        response_url: None,
        message_ts: None,
        actions: Vec::new(),
        view_id: view_string(&view, "id"),
        view_callback_id: view_string(&view, "callback_id"),
        view_private_metadata: view_string(&view, "private_metadata"),
        retry: payload.retry.as_ref().map(SlackRetryInfo::from),
        raw: payload.raw.clone(),
    }
}

fn interaction_request_from_view_closed(
    payload: &SlackViewClosedPayload,
) -> SlackInteractionRequest {
    let view = serde_json::Value::Object(payload.view.clone());
    SlackInteractionRequest {
        kind: SlackInteractionKind::ViewClosed,
        target: None,
        user_id: payload.user_id.clone(),
        trigger_id: None,
        response_url: None,
        message_ts: None,
        actions: Vec::new(),
        view_id: view_string(&view, "id"),
        view_callback_id: view_string(&view, "callback_id"),
        view_private_metadata: view_string(&view, "private_metadata"),
        retry: payload.retry.as_ref().map(SlackRetryInfo::from),
        raw: payload.raw.clone(),
    }
}

fn view_string(view: &serde_json::Value, key: &str) -> Option<String> {
    view.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn should_ignore_direct_message(payload: &SlackDirectMessagePayload) -> bool {
    payload.bot_id.is_some()
        || matches!(
            payload.subtype.as_deref(),
            Some("bot_message" | "message_deleted" | "message_changed")
        )
}

fn dedupe_key_for_start(request: &SlackRunStartRequest) -> String {
    if let Some(event_id) = request.event_id.as_deref() {
        return format!("slack:ingress:event:{event_id}");
    }

    let payload = json!({
        "source": request.source,
        "channelId": request.target.channel_id,
        "threadTs": request.target.thread_ts,
        "messageTs": request.message_ts,
        "userId": request.user_id,
        "command": request.command,
        "text": request.text,
        "triggerId": request.trigger_id,
    });
    format!(
        "slack:ingress:start:{}:{}",
        request.target.channel_id,
        stable_digest(&payload)
    )
}

fn dedupe_key_for_interaction(request: &SlackInteractionRequest) -> String {
    let payload = json!({
        "kind": request.kind,
        "target": request.target,
        "userId": request.user_id,
        "messageTs": request.message_ts,
        "actions": request.actions,
        "viewId": request.view_id,
        "viewCallbackId": request.view_callback_id,
        "viewPrivateMetadata": request.view_private_metadata,
    });
    format!("slack:ingress:interaction:{}", stable_digest(&payload))
}

fn stable_digest(value: &serde_json::Value) -> String {
    use std::fmt::Write as _;

    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_state_memory::create_memory_state;
    use futures_executor::block_on;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[test]
    fn slack_thread_address_uses_adapter_thread_id_shape() {
        let address = SlackThreadAddress::new("C123", "1710000000.000100");

        assert_eq!(address.chat_thread_id(), "slack:C123:1710000000.000100");
    }

    #[test]
    fn ingress_mode_serializes_as_snake_case() {
        let encoded = serde_json::to_string(&SlackIngressMode::SocketMode).unwrap();

        assert_eq!(encoded, "\"socket_mode\"");
    }

    #[derive(Debug, Default)]
    struct RecordingRouter {
        starts: Mutex<Vec<SlackRunStartRequest>>,
        interactions: Mutex<Vec<SlackInteractionRequest>>,
    }

    #[async_trait]
    impl SlackRunRouter for RecordingRouter {
        async fn start_or_resume(
            &self,
            request: SlackRunStartRequest,
        ) -> SlackRunRouterResult<SlackRunHandoff> {
            self.starts.lock().unwrap().push(request);
            Ok(SlackRunHandoff::new(Some("run-start".to_string()), false))
        }

        async fn resume_interaction(
            &self,
            request: SlackInteractionRequest,
        ) -> SlackRunRouterResult<SlackRunHandoff> {
            self.interactions.lock().unwrap().push(request);
            Ok(SlackRunHandoff::new(Some("run-resume".to_string()), true))
        }
    }

    fn service() -> (SlackIngress, Arc<RecordingRouter>) {
        let state = create_memory_state(None);
        state.connect().unwrap();
        let state: Arc<dyn StateAdapter> = Arc::new(state);
        let router = Arc::new(RecordingRouter::default());
        let router_dyn: Arc<dyn SlackRunRouter> = router.clone();
        let service = SlackIngress::new(
            SlackIngressOptions::new("signing-secret")
                .with_now_seconds(1_700_000_000)
                .with_max_skew_seconds(300),
            state,
            router_dyn,
        );
        (service, router)
    }

    fn sign(body: &str, timestamp: &str) -> String {
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(b"signing-secret").unwrap();
        mac.update(b"v0:");
        mac.update(timestamp.as_bytes());
        mac.update(b":");
        mac.update(body.as_bytes());
        let bytes = mac.finalize().into_bytes();
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!("v0={hex}")
    }

    fn signed_json(body: &str) -> SlackHttpRequest {
        signed(body, "application/json")
    }

    fn signed_form(body: &str) -> SlackHttpRequest {
        signed(body, "application/x-www-form-urlencoded")
    }

    fn signed(body: &str, content_type: &str) -> SlackHttpRequest {
        let timestamp = "1700000000";
        SlackHttpRequest::new(body)
            .with_header("content-type", content_type)
            .with_header("x-slack-request-timestamp", timestamp)
            .with_header("x-slack-signature", sign(body, timestamp))
    }

    #[test]
    fn events_api_url_verification_returns_challenge() {
        let (service, router) = service();
        let body = r#"{"type":"url_verification","challenge":"challenge-code"}"#;

        let response = block_on(service.handle_events_api(signed_json(body)));

        assert_eq!(response.status, 200);
        assert_eq!(response.body, "challenge-code");
        assert_eq!(response.outcome, SlackIngressOutcome::UrlVerified);
        assert!(router.starts.lock().unwrap().is_empty());
    }

    #[test]
    fn events_api_url_verification_without_signature_returns_challenge() {
        let (service, router) = service();
        let body = r#"{"type":"url_verification","challenge":"manifest-challenge"}"#;
        let request = SlackHttpRequest::new(body).with_header("content-type", "application/json");

        let response = block_on(service.handle_events_api(request));

        assert_eq!(response.status, 200);
        assert_eq!(response.body, "manifest-challenge");
        assert_eq!(response.outcome, SlackIngressOutcome::UrlVerified);
        assert!(router.starts.lock().unwrap().is_empty());
    }

    #[test]
    fn events_api_rejects_unsigned_app_mentions() {
        let (service, router) = service();
        let body = r#"{"type":"event_callback","team_id":"T123","event_id":"EvUnsigned","event_time":1700000000,"event":{"type":"app_mention","user":"U123","text":"<@UBOT> ship it","ts":"1700000000.000100","channel":"C123","team":"T123"}}"#;
        let request = SlackHttpRequest::new(body).with_header("content-type", "application/json");

        let response = block_on(service.handle_events_api(request));

        assert_eq!(response.status, 401);
        assert_eq!(
            response.outcome,
            SlackIngressOutcome::SignatureRejected {
                error: SlackWebhookVerificationError::MissingHeaders
            }
        );
        assert!(router.starts.lock().unwrap().is_empty());
    }

    #[test]
    fn events_api_rejects_invalid_signature_before_parse() {
        let (service, router) = service();
        let request = SlackHttpRequest::new("not-json")
            .with_header("content-type", "application/json")
            .with_header("x-slack-request-timestamp", "1700000000")
            .with_header("x-slack-signature", "v0=bad");

        let response = block_on(service.handle_events_api(request));

        assert_eq!(response.status, 401);
        assert!(matches!(
            response.outcome,
            SlackIngressOutcome::SignatureRejected { .. }
        ));
        assert!(router.starts.lock().unwrap().is_empty());
    }

    #[test]
    fn events_api_rejects_stale_timestamp() {
        let (service, router) = service();
        let body = r#"{"type":"url_verification","challenge":"challenge-code"}"#;
        let request = SlackHttpRequest::new(body)
            .with_header("content-type", "application/json")
            .with_header("x-slack-request-timestamp", "1699999000")
            .with_header("x-slack-signature", sign(body, "1699999000"));

        let response = block_on(service.handle_events_api(request));

        assert_eq!(response.status, 401);
        assert_eq!(
            response.outcome,
            SlackIngressOutcome::SignatureRejected {
                error: SlackWebhookVerificationError::TimestampTooOld
            }
        );
        assert!(router.starts.lock().unwrap().is_empty());
    }

    #[test]
    fn app_mention_starts_run_with_top_level_thread_id() {
        let (service, router) = service();
        let body = r#"{"type":"event_callback","team_id":"T123","event_id":"Ev123","event_time":1700000000,"event":{"type":"app_mention","user":"U123","text":"<@UBOT> ship it","ts":"1700000000.000100","channel":"C123","team":"T123"}}"#;

        let response = block_on(service.handle_events_api(signed_json(body)));

        assert_eq!(response.status, 200);
        let starts = router.starts.lock().unwrap();
        assert_eq!(starts.len(), 1);
        let start = &starts[0];
        assert_eq!(start.source, SlackRunStartSource::AppMention);
        assert_eq!(start.target.slack_thread_id, "slack:C123:1700000000.000100");
        assert_eq!(start.user_id.as_deref(), Some("U123"));
        assert_eq!(start.event_id.as_deref(), Some("Ev123"));
        assert!(matches!(
            response.outcome,
            SlackIngressOutcome::StartOrResume { .. }
        ));
    }

    #[test]
    fn app_mention_threaded_reply_routes_to_parent_thread_ts() {
        let (service, router) = service();
        let body = r#"{"type":"event_callback","team_id":"T123","event_id":"Ev124","event_time":1700000000,"event":{"type":"app_mention","user":"U123","text":"follow up","ts":"1700000000.000200","thread_ts":"1699999999.000100","channel":"C123","team":"T123"}}"#;

        let response = block_on(service.handle_events_api(signed_json(body)));

        assert_eq!(response.status, 200);
        let starts = router.starts.lock().unwrap();
        assert_eq!(
            starts[0].target.slack_thread_id,
            "slack:C123:1699999999.000100"
        );
        assert_eq!(starts[0].message_ts.as_deref(), Some("1700000000.000200"));
    }

    #[test]
    fn dm_event_starts_run_and_routes_as_dm_thread() {
        let (service, router) = service();
        let body = r#"{"type":"event_callback","team_id":"T123","event_id":"EvDM","event_time":1700000000,"event":{"type":"message","channel_type":"im","user":"U123","text":"hello","ts":"1700000000.000300","channel":"D123","team":"T123"}}"#;

        let response = block_on(service.handle_events_api(signed_json(body)));

        assert_eq!(response.status, 200);
        let starts = router.starts.lock().unwrap();
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].source, SlackRunStartSource::DirectMessage);
        assert_eq!(
            starts[0].target.slack_thread_id,
            "slack:D123:1700000000.000300"
        );
    }

    #[test]
    fn mpim_event_starts_run_and_routes_as_group_dm_thread() {
        let (service, router) = service();
        let body = r#"{"type":"event_callback","team_id":"T123","event_id":"EvMpim","event_time":1700000000,"event":{"type":"message","channel_type":"mpim","user":"U123","text":"help the group","ts":"1700000000.000350","channel":"G123","team":"T123"}}"#;

        let response = block_on(service.handle_events_api(signed_json(body)));

        assert_eq!(response.status, 200);
        let starts = router.starts.lock().unwrap();
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].source, SlackRunStartSource::DirectMessage);
        assert_eq!(
            starts[0].target.slack_thread_id,
            "slack:G123:1700000000.000350"
        );
        assert_eq!(starts[0].text, "help the group");
    }

    #[test]
    fn dm_bot_message_is_acked_without_run() {
        let (service, router) = service();
        let body = r#"{"type":"event_callback","team_id":"T123","event_id":"EvBot","event_time":1700000000,"event":{"type":"message","channel_type":"im","user":"U123","bot_id":"B123","subtype":"bot_message","text":"bot echo","ts":"1700000000.000300","channel":"D123","team":"T123"}}"#;

        let response = block_on(service.handle_events_api(signed_json(body)));

        assert_eq!(response.status, 200);
        assert!(matches!(
            response.outcome,
            SlackIngressOutcome::Ignored { .. }
        ));
        assert!(router.starts.lock().unwrap().is_empty());
    }

    #[test]
    fn slash_command_starts_run_from_form_body() {
        let (service, router) = service();
        let body = "team_id=T123&channel_id=C123&channel_name=general&user_id=U123&user_name=ada&command=%2Fagent&text=fix+tests&trigger_id=1337.abc&response_url=https%3A%2F%2Fhooks.slack.com%2Fcommands%2F123";

        let response = block_on(service.handle_slash_command(signed_form(body)));

        assert_eq!(response.status, 200);
        let starts = router.starts.lock().unwrap();
        assert_eq!(starts.len(), 1);
        let start = &starts[0];
        assert_eq!(start.source, SlackRunStartSource::SlashCommand);
        assert_eq!(start.command.as_deref(), Some("/agent"));
        assert_eq!(start.text, "fix tests");
        assert_eq!(start.target.slack_thread_id, "slack:C123:");
        assert_eq!(start.trigger_id.as_deref(), Some("1337.abc"));
    }

    #[test]
    fn block_action_resumes_run_with_thread_route_and_action_value() {
        let (service, router) = service();
        let payload = json!({
            "type": "block_actions",
            "team": { "id": "T123" },
            "user": { "id": "U123", "username": "ada" },
            "channel": { "id": "C123" },
            "message": { "ts": "1700000000.000400", "thread_ts": "1699999999.000100" },
            "trigger_id": "trig.1",
            "actions": [
                { "type": "button", "action_id": "approve", "block_id": "approval", "value": "run-1:tool-1" }
            ]
        });
        let body = format!("payload={}", form_encode(&payload.to_string()));

        let response = block_on(service.handle_interactions(signed_form(&body)));

        assert_eq!(response.status, 200);
        let interactions = router.interactions.lock().unwrap();
        assert_eq!(interactions.len(), 1);
        let interaction = &interactions[0];
        assert_eq!(interaction.kind, SlackInteractionKind::BlockActions);
        assert_eq!(
            interaction.target.as_ref().unwrap().slack_thread_id,
            "slack:C123:1699999999.000100"
        );
        assert_eq!(interaction.actions[0].action_id, "approve");
        assert_eq!(
            interaction.actions[0].value.as_deref(),
            Some("run-1:tool-1")
        );
        assert!(matches!(
            response.outcome,
            SlackIngressOutcome::ResumeInteraction { .. }
        ));
    }

    #[test]
    fn view_submission_resumes_with_view_metadata() {
        let (service, router) = service();
        let payload = json!({
            "type": "view_submission",
            "team": { "id": "T123" },
            "user": { "id": "U123" },
            "view": {
                "id": "V123",
                "callback_id": "answer_question",
                "private_metadata": "run-1:question-1",
                "state": { "values": {} }
            }
        });
        let body = format!("payload={}", form_encode(&payload.to_string()));

        let response = block_on(service.handle_interactions(signed_form(&body)));

        assert_eq!(response.status, 200);
        let interactions = router.interactions.lock().unwrap();
        assert_eq!(interactions.len(), 1);
        assert_eq!(interactions[0].kind, SlackInteractionKind::ViewSubmission);
        assert_eq!(interactions[0].view_id.as_deref(), Some("V123"));
        assert_eq!(
            interactions[0].view_private_metadata.as_deref(),
            Some("run-1:question-1")
        );
    }

    #[test]
    fn retry_dedupe_acks_duplicate_without_second_handoff() {
        let (service, router) = service();
        let body = r#"{"type":"event_callback","team_id":"T123","event_id":"EvRetry","event_time":1700000000,"event":{"type":"app_mention","user":"U123","text":"again","ts":"1700000000.000500","channel":"C123","team":"T123"}}"#;
        let first = signed_json(body);
        let retry = signed_json(body)
            .with_header("x-slack-retry-num", "1")
            .with_header("x-slack-retry-reason", "http_timeout");

        let response1 = block_on(service.handle_events_api(first));
        let response2 = block_on(service.handle_events_api(retry));

        assert_eq!(response1.status, 200);
        assert_eq!(response2.status, 200);
        assert!(matches!(
            response2.outcome,
            SlackIngressOutcome::Duplicate { .. }
        ));
        assert_eq!(router.starts.lock().unwrap().len(), 1);
    }

    #[test]
    fn unsupported_payload_is_acked_without_handoff() {
        let (service, router) = service();
        let body = r#"{"type":"event_callback","team_id":"T123","event_id":"EvReaction","event":{"type":"reaction_added","user":"U123","reaction":"eyes","item":{"channel":"C123","ts":"1700000000.1"}}}"#;

        let response = block_on(service.handle_events_api(signed_json(body)));

        assert_eq!(response.status, 200);
        assert_eq!(
            response.outcome,
            SlackIngressOutcome::Unsupported {
                payload_type: "reaction_added".to_string()
            }
        );
        assert!(router.starts.lock().unwrap().is_empty());
    }

    fn form_encode(input: &str) -> String {
        let mut out = String::new();
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char);
                }
                b' ' => out.push('+'),
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    }

    #[test]
    fn digest_is_stable_for_sorted_json_objects() {
        let mut left = BTreeMap::new();
        left.insert("b", json!(2));
        left.insert("a", json!(1));
        let mut right = BTreeMap::new();
        right.insert("a", json!(1));
        right.insert("b", json!(2));

        assert_eq!(
            stable_digest(&serde_json::to_value(left).unwrap()),
            stable_digest(&serde_json::to_value(right).unwrap())
        );
    }
}
