use std::env;
use std::fs;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use ai_sdk_rust::VercelAiGatewayOpenAICompatibleProvider;

fn main() {
    let limit = env::var("AI_GATEWAY_MODEL_LIST_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);
    let provider = match gateway_api_key() {
        Some(api_key) => VercelAiGatewayOpenAICompatibleProvider::new().with_api_key(api_key),
        None => VercelAiGatewayOpenAICompatibleProvider::new(),
    };
    let models = poll_ready(provider.list_models()).expect("Gateway model list request failed");
    let first_model_id = models.data.first().map(|model| model.id.clone());

    for model in models.data.iter().take(limit) {
        let model_type = model.model_type.as_deref().unwrap_or("unknown");
        let tags = if model.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", model.tags.join(","))
        };
        println!("{} ({}){}", model.id, model_type, tags);
    }

    if let Some(model_id) = first_model_id {
        let model = poll_ready(provider.retrieve_model(&model_id))
            .expect("Gateway model retrieval request failed");
        println!(
            "retrieved {} ({})",
            model.id,
            model.model_type.as_deref().unwrap_or("unknown")
        );
    }
}

fn gateway_api_key() -> Option<String> {
    non_empty_env_setting("AI_GATEWAY_API_KEY")
        .or_else(|| non_empty_env_setting("AI_SDK_RUST_AI_GATEWAY_API_KEY"))
        .or_else(|| non_empty_env_setting("VERCEL_OIDC_TOKEN"))
        .or_else(|| dotenv_setting("AI_GATEWAY_API_KEY"))
        .or_else(|| dotenv_setting("AI_SDK_RUST_AI_GATEWAY_API_KEY"))
        .or_else(|| dotenv_setting("VERCEL_OIDC_TOKEN"))
}

fn non_empty_env_setting(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn dotenv_setting(name: &str) -> Option<String> {
    let content = fs::read_to_string(".env.local").ok()?;

    content.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }

        let (key, value) = line.split_once('=')?;
        if key.trim() == name {
            let value = unquote_dotenv_value(value.trim());
            if value.is_empty() { None } else { Some(value) }
        } else {
            None
        }
    })
}

fn unquote_dotenv_value(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return value[1..value.len() - 1].to_string();
        }
    }

    value.to_string()
}

fn poll_ready<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = Box::pin(future);

    match Pin::new(&mut future).poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("default provider transport completes synchronously"),
    }
}
