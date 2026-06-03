//! Upstream parity-proof tests for `@ai-sdk/gladia`.
//!
//! Each test below maps 1:1 to an upstream Vitest case recorded in
//! `docs/ai-06-concrete-provider-mappings.md` and exercises the real Gladia
//! provider behavior through the crate's public API. The transports return the
//! same fixtures the upstream suite serves from
//! `packages/gladia/src/__fixtures__`, so every assertion fails if the ported
//! behavior diverges from upstream.

use std::future::{Future, ready};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use ai_sdk_gladia::{
    GladiaProviderSettings, GladiaTransport, GladiaTransportFuture, create_gladia,
};
use ai_sdk_rust::{
    FileDataContent, Headers, ProviderApiRequest, ProviderApiRequestBody, ProviderApiResponse,
    TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelResult,
};
use serde_json::{Value, json};
use time::OffsetDateTime;

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn poll_ready<F>(future: F) -> F::Output
where
    F: Future,
{
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);

    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("test futures use ready transports"),
    }
}

fn capture_transport<F>(handler: F) -> (Arc<Mutex<Vec<ProviderApiRequest>>>, GladiaTransport)
where
    F: Fn(&ProviderApiRequest) -> ProviderApiResponse + Send + Sync + 'static,
{
    let requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
    let captured = Arc::clone(&requests);
    let handler = Arc::new(handler);
    let transport = Arc::new(
        move |request: ProviderApiRequest| -> GladiaTransportFuture {
            captured
                .lock()
                .expect("requests lock")
                .push(request.clone());
            let response = handler(&request);
            Box::pin(ready(Ok(response)))
        },
    );

    (requests, transport)
}

/// Upload fixture matching `packages/gladia/src/__fixtures__/gladia-upload.json`.
fn upload_fixture() -> Value {
    json!({ "audio_url": "https://api.gladia.io/file/audio-123" })
}

/// Initiate fixture matching `packages/gladia/src/__fixtures__/gladia-initiate.json`.
fn initiate_fixture() -> Value {
    json!({ "result_url": "https://api.gladia.io/v2/pre-recorded/job-123" })
}

/// Result fixture matching `packages/gladia/src/__fixtures__/gladia-result.json`.
fn result_fixture() -> Value {
    json!({
        "status": "done",
        "result": {
            "metadata": { "audio_duration": 36.74 },
            "transcription": {
                "languages": ["en"],
                "utterances": [
                    {
                        "text": "Galileo was an American robotic space program.",
                        "start": 0.14,
                        "end": 5.341
                    },
                    { "text": "It studied Jupiter.", "start": 5.662, "end": 8.099 }
                ],
                "full_transcript":
                    "Galileo was an American robotic space program. It studied Jupiter."
            }
        }
    })
}

fn json_response(value: Value) -> ProviderApiResponse {
    json_response_with_headers(value, &[])
}

fn json_response_with_headers(value: Value, extra: &[(&str, &str)]) -> ProviderApiResponse {
    let mut headers = Headers::from([("content-type".to_string(), "application/json".to_string())]);
    for (name, value) in extra {
        headers.insert((*name).to_string(), (*value).to_string());
    }
    ProviderApiResponse::text(200, "OK", serde_json::to_string(&value).expect("json"))
        .with_headers(headers)
}

/// Transport that serves the full upload/initiate/poll fixture chain.
///
/// `result_headers` are attached to the poll (result) response so that response
/// header propagation can be asserted.
fn fixture_transport(
    result_headers: &'static [(&'static str, &'static str)],
) -> (Arc<Mutex<Vec<ProviderApiRequest>>>, GladiaTransport) {
    let upload = upload_fixture();
    let initiate = initiate_fixture();
    let result = result_fixture();
    capture_transport(move |request| match request.url.as_str() {
        "https://api.gladia.io/v2/upload" => json_response(upload.clone()),
        "https://api.gladia.io/v2/pre-recorded" => json_response(initiate.clone()),
        "https://api.gladia.io/v2/pre-recorded/job-123" => {
            json_response_with_headers(result.clone(), result_headers)
        }
        other => panic!("unexpected request url {other}"),
    })
}

fn request_json_body(request: &ProviderApiRequest) -> Value {
    match request.body.as_ref().expect("request body") {
        ProviderApiRequestBody::Text { content } => {
            serde_json::from_str(content).expect("json body")
        }
        other => panic!("expected text JSON body, got {other:?}"),
    }
}

fn run_default_generate(
    result_headers: &'static [(&'static str, &'static str)],
) -> (
    Arc<Mutex<Vec<ProviderApiRequest>>>,
    TranscriptionModelResult,
) {
    let (requests, transport) = fixture_transport(result_headers);
    let provider = create_gladia(GladiaProviderSettings::new().with_api_key("test-api-key"))
        .with_transport(transport)
        .with_current_date(|| OffsetDateTime::UNIX_EPOCH);
    let result = poll_ready(provider.transcription().do_generate(
        TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/wav"),
    ));
    (requests, result)
}

/// gladia-0001 `gladia-error.test.ts:6` it should parse Gladia resource exhausted error.
///
/// Upstream parses the Gladia error envelope and surfaces the inner message. The
/// port routes that error through `doGenerate`, so we assert the exhausted-quota
/// message is decoded from the error body and surfaced as the error metadata.
#[test]
fn gladia_0001_it_should_parse_gladia_resource_exhausted_error() {
    let (requests, transport) = capture_transport(|request| match request.url.as_str() {
        "https://api.gladia.io/v2/upload" => ProviderApiResponse::text(
            429,
            "Too Many Requests",
            r#"{"error":{"message":"Resource has been exhausted (e.g. check quota).","code":429}}"#,
        ),
        other => panic!("unexpected request url {other}"),
    });
    let provider = create_gladia(GladiaProviderSettings::new().with_api_key("test-api-key"))
        .with_transport(transport);
    let result = poll_ready(provider.transcription().do_generate(
        TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/wav"),
    ));

    assert_eq!(requests.lock().expect("requests lock").len(), 1);
    assert_eq!(
        result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("gladia"))
            .and_then(|metadata| metadata.get("errorMessage"))
            .and_then(Value::as_str),
        Some("Resource has been exhausted (e.g. check quota).")
    );
}

/// gladia-0002 `gladia-transcription-model.test.ts:55` it should pass audio_url to pre-recorded endpoint.
#[test]
fn gladia_0002_it_should_pass_audio_url_to_pre_recorded_endpoint() {
    let (requests, _) = run_default_generate(&[]);
    let requests = requests.lock().expect("requests lock");

    assert_eq!(requests[1].url, "https://api.gladia.io/v2/pre-recorded");
    let body = request_json_body(&requests[1]);
    assert_eq!(
        body.get("audio_url").and_then(Value::as_str),
        Some("https://api.gladia.io/file/audio-123")
    );
}

/// gladia-0003 `gladia-transcription-model.test.ts:66` it should pass headers.
#[test]
fn gladia_0003_it_should_pass_headers() {
    let (requests, transport) = fixture_transport(&[]);
    let provider = create_gladia(
        GladiaProviderSettings::new()
            .with_api_key("test-api-key")
            .with_header("Custom-Provider-Header", "provider-header-value"),
    )
    .with_transport(transport);
    let _ = poll_ready(
        provider.transcription().do_generate(
            TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/wav")
                .with_header("Custom-Request-Header", "request-header-value"),
        ),
    );
    let requests = requests.lock().expect("requests lock");

    // pre-recorded (JSON) request carries the merged provider + request headers.
    assert_eq!(
        requests[1].headers.get("x-gladia-key"),
        Some(&"test-api-key".to_string())
    );
    assert_eq!(
        requests[1].headers.get("content-type"),
        Some(&"application/json".to_string())
    );
    assert_eq!(
        requests[1].headers.get("custom-provider-header"),
        Some(&"provider-header-value".to_string())
    );
    assert_eq!(
        requests[1].headers.get("custom-request-header"),
        Some(&"request-header-value".to_string())
    );
    assert!(
        requests[0]
            .headers
            .get("user-agent")
            .is_some_and(|value| value.contains("ai-sdk/gladia/"))
    );
}

/// gladia-0004 `gladia-transcription-model.test.ts:93` it should extract the transcription text.
#[test]
fn gladia_0004_it_should_extract_the_transcription_text() {
    let (_, result) = run_default_generate(&[]);
    assert_eq!(
        result.text,
        "Galileo was an American robotic space program. It studied Jupiter."
    );
}

/// gladia-0005 `gladia-transcription-model.test.ts:104` it should generate full response.
#[test]
fn gladia_0005_it_should_generate_full_response() {
    use ai_sdk_rust::TranscriptionModelSegment;

    let (_, result) = run_default_generate(&[]);

    assert_eq!(
        result.text,
        "Galileo was an American robotic space program. It studied Jupiter."
    );
    assert_eq!(result.language.as_deref(), Some("en"));
    assert_eq!(result.duration_in_seconds, Some(36.74));
    assert_eq!(
        result.segments,
        vec![
            TranscriptionModelSegment::new(
                "Galileo was an American robotic space program.",
                0.14,
                5.341
            ),
            TranscriptionModelSegment::new("It studied Jupiter.", 5.662, 8.099),
        ]
    );
    assert_eq!(result.response.timestamp, OffsetDateTime::UNIX_EPOCH);
    assert_eq!(result.response.model_id, "default");
}

/// gladia-0006 `gladia-transcription-model.test.ts:132` it should include response headers.
#[test]
fn gladia_0006_it_should_include_response_headers() {
    let (_, result) = run_default_generate(&[
        ("x-request-id", "test-request-id"),
        ("x-ratelimit-remaining", "123"),
    ]);
    let headers = result
        .response
        .headers
        .as_ref()
        .expect("response headers present");

    assert_eq!(
        headers.get("content-type"),
        Some(&"application/json".to_string())
    );
    assert_eq!(
        headers.get("x-request-id"),
        Some(&"test-request-id".to_string())
    );
    assert_eq!(
        headers.get("x-ratelimit-remaining"),
        Some(&"123".to_string())
    );
}

/// gladia-0007 `gladia-transcription-model.test.ts:163` it should include timestamp and modelId.
#[test]
fn gladia_0007_it_should_include_timestamp_and_model_id() {
    let (_, result) = run_default_generate(&[]);
    assert_eq!(result.response.timestamp, OffsetDateTime::UNIX_EPOCH);
    assert_eq!(result.response.model_id, "default");
}
