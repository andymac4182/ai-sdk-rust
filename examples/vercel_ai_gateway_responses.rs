use std::env;
use std::fs;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use ai_sdk_rust::{
    GenerateTextOptions, Prompt, VercelAiGatewayOpenAICompatibleProvider, generate_text,
};

fn main() {
    let api_key = gateway_api_key().expect(
        "set AI_GATEWAY_API_KEY, AI_SDK_RUST_AI_GATEWAY_API_KEY, or VERCEL_OIDC_TOKEN in the environment or .env.local",
    );
    let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL")
        .or_else(|_| env::var("AI_GATEWAY_OPENAI_RESPONSES_MODEL"))
        .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RESPONSES_MODEL"))
        .or_else(|_| env::var("AI_GATEWAY_RESPONSES_MODEL"))
        .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
    let model = VercelAiGatewayOpenAICompatibleProvider::new()
        .with_api_key(api_key)
        .responses(model_id);

    let result = poll_ready(generate_text(
        GenerateTextOptions::from_prompt(
            &model,
            Prompt::from_prompt("Reply with one short sentence about the OpenAI Responses API."),
        )
        .expect("prompt should standardize")
        .with_max_output_tokens(48)
        .with_temperature(0.2),
    ));

    println!("{}", result.text);
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
