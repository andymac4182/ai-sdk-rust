use std::env;
use std::error::Error;
use std::time::{Duration, Instant};

use ai_sdk_otel::LocalOtlpTraceReceiver;
use serde_json::Value as JsonValue;

fn main() -> Result<(), Box<dyn Error>> {
    let timeout = receiver_timeout();
    let expected_requests = env::var("AI_SDK_RUST_OTEL_RECEIVER_REQUESTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    let receiver = LocalOtlpTraceReceiver::start()?;
    println!("OTLP receiver listening at {}", receiver.endpoint());
    println!(
        "export OTEL_EXPORTER_OTLP_TRACES_ENDPOINT={}",
        receiver.endpoint()
    );
    println!("export OTEL_EXPORTER_OTLP_PROTOCOL=http/json");
    match timeout {
        Some(timeout) => println!(
            "waiting up to {} seconds for {} OTLP trace export(s)",
            timeout.as_secs(),
            request_count_label(expected_requests),
        ),
        None => println!(
            "waiting indefinitely for {} OTLP trace export(s); stop with Ctrl-C",
            request_count_label(expected_requests),
        ),
    }

    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let mut printed_requests = 0;
    loop {
        let requests = receiver.received_requests();
        for request in requests.iter().skip(printed_requests) {
            println!(
                "received request {}: {} {}",
                printed_requests + 1,
                request.method,
                request.path
            );
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
            printed_requests += 1;
        }
        if expected_requests > 0 && printed_requests >= expected_requests {
            return Ok(());
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            if printed_requests == 0 {
                println!("no OTLP trace export received before timeout");
            } else {
                println!(
                    "received {} OTLP trace export(s) before timeout",
                    printed_requests
                );
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn receiver_timeout() -> Option<Duration> {
    env::var("AI_SDK_RUST_OTEL_RECEIVER_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| {
            if seconds == 0 {
                None
            } else {
                Some(Duration::from_secs(seconds))
            }
        })
        .unwrap_or_else(|| Some(Duration::from_secs(30)))
}

fn request_count_label(expected_requests: usize) -> String {
    if expected_requests == 0 {
        "unlimited".to_string()
    } else {
        expected_requests.to_string()
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
