use std::env;
use std::error::Error;
use std::time::{Duration, Instant};

use ai_sdk_otel::LocalOtlpTraceReceiver;
use serde_json::Value as JsonValue;

fn main() -> Result<(), Box<dyn Error>> {
    let timeout = env::var("AI_SDK_RUST_OTEL_RECEIVER_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(30));
    let receiver = LocalOtlpTraceReceiver::start()?;
    println!("OTLP receiver listening at {}", receiver.endpoint());
    println!(
        "Set OTEL_EXPORTER_OTLP_TRACES_ENDPOINT={} and send one trace export within {} seconds.",
        receiver.endpoint(),
        timeout.as_secs()
    );

    let deadline = Instant::now() + timeout;
    loop {
        let requests = receiver.wait_for_requests(1, Duration::from_millis(250));
        if let Some(request) = requests.first() {
            println!("received {} {}", request.method, request.path);
            match request.body_json() {
                Some(body) => {
                    let span_names = otlp_span_names(&body);
                    if span_names.is_empty() {
                        println!("received OTLP JSON with no spans");
                    } else {
                        println!("received spans: {}", span_names.join(", "));
                    }
                }
                None => println!("received non-JSON OTLP body"),
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            println!("no OTLP trace export received before timeout");
            return Ok(());
        }
    }
}

fn otlp_span_names(body: &JsonValue) -> Vec<String> {
    let mut names = Vec::new();
    let Some(resource_spans) = body.get("resourceSpans").and_then(JsonValue::as_array) else {
        return names;
    };
    for resource_span in resource_spans {
        let Some(scope_spans) = resource_span
            .get("scopeSpans")
            .and_then(JsonValue::as_array)
        else {
            continue;
        };
        for scope_span in scope_spans {
            let Some(spans) = scope_span.get("spans").and_then(JsonValue::as_array) else {
                continue;
            };
            names.extend(
                spans
                    .iter()
                    .filter_map(|span| span.get("name").and_then(JsonValue::as_str))
                    .map(str::to_string),
            );
        }
    }
    names
}
