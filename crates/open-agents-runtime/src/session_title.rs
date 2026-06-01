//! Open Agents session-title route helpers.

use serde_json::{Value, json};

/// Response-like result for deterministic `/api/generate-title` parity tests.
#[derive(Clone, Debug, PartialEq)]
pub struct GenerateTitleRouteOutput {
    pub status: u16,
    pub body: Value,
}

/// Handles the portable auth, JSON, validation, and generation branches.
pub fn handle_generate_title_request(
    authenticated: bool,
    body: Result<Value, ()>,
    generate_title: impl FnOnce(&str) -> Option<String>,
) -> GenerateTitleRouteOutput {
    if !authenticated {
        return response(401, json!({ "error": "Not authenticated" }));
    }

    let Ok(body) = body else {
        return response(400, json!({ "error": "Invalid JSON body" }));
    };

    let Some(message) = body.get("message").and_then(Value::as_str) else {
        return response(400, json!({ "error": "Missing required field: message" }));
    };
    let Some(trimmed) = normalize_session_title_message(message) else {
        return response(400, json!({ "error": "Missing required field: message" }));
    };

    match generate_title(&trimmed).and_then(|title| parse_generated_session_title(&title)) {
        Some(title) => response(200, json!({ "title": title })),
        None => response(500, json!({ "error": "Failed to generate title" })),
    }
}

/// Trims and bounds the user message that is sent to the title model.
pub fn normalize_session_title_message(message: &str) -> Option<String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(2000).collect())
}

/// Builds the model prompt used to name a coding session.
pub fn build_session_title_prompt(message: &str) -> Option<String> {
    let message = normalize_session_title_message(message)?;
    Some(format!(
        "You are a developer tool that names coding sessions. Generate a concise title (max 5 words) for a coding session based on the user's first message below. The title should help the user quickly identify what this session is about at a glance. Do NOT use quotes or punctuation around the title. Respond with ONLY the title, nothing else.\n\nUser message:\n{message}"
    ))
}

/// Extracts the first non-empty generated title line and caps it at 60 chars.
pub fn parse_generated_session_title(text: &str) -> Option<String> {
    let title = text.lines().next()?.trim();
    if title.is_empty() {
        return None;
    }
    Some(title.chars().take(60).collect())
}

fn response(status: u16, body: Value) -> GenerateTitleRouteOutput {
    GenerateTitleRouteOutput { status, body }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_title_route_matches_auth_json_validation_and_success_cases() {
        let unauthenticated =
            handle_generate_title_request(false, Ok(json!({ "message": "hello" })), |_| {
                Some("unused".to_string())
            });
        assert_eq!(unauthenticated.status, 401);
        assert_eq!(unauthenticated.body["error"], json!("Not authenticated"));

        let invalid_json =
            handle_generate_title_request(true, Err(()), |_| Some("unused".to_string()));
        assert_eq!(invalid_json.status, 400);
        assert_eq!(invalid_json.body["error"], json!("Invalid JSON body"));

        let missing =
            handle_generate_title_request(true, Ok(json!({})), |_| Some("unused".to_string()));
        assert_eq!(missing.status, 400);
        assert_eq!(
            missing.body["error"],
            json!("Missing required field: message")
        );

        let blank = handle_generate_title_request(true, Ok(json!({ "message": "   " })), |_| {
            Some("unused".to_string())
        });
        assert_eq!(blank.status, 400);
        assert_eq!(
            blank.body["error"],
            json!("Missing required field: message")
        );

        let mut captured_message = String::new();
        let success = handle_generate_title_request(
            true,
            Ok(json!({ "message": "  hello world  " })),
            |message| {
                captured_message = message.to_string();
                Some("  Fix API Validation\nIgnore this line".to_string())
            },
        );
        assert_eq!(success.status, 200);
        assert_eq!(success.body["title"], json!("Fix API Validation"));
        assert_eq!(captured_message, "hello world");
    }

    #[test]
    fn generate_title_prompt_truncates_input_and_failure_maps_to_500() {
        let long_message = format!("{}tail", "a".repeat(2100));
        let prompt = build_session_title_prompt(&long_message).expect("prompt");
        assert!(prompt.contains("User message:\n"));
        assert!(!prompt.contains("tail"));
        assert_eq!(
            normalize_session_title_message(&long_message)
                .expect("message")
                .chars()
                .count(),
            2000
        );

        let empty_generation =
            handle_generate_title_request(true, Ok(json!({ "message": "hello" })), |_| {
                Some("\n".to_string())
            });
        assert_eq!(empty_generation.status, 500);
        assert_eq!(
            empty_generation.body["error"],
            json!("Failed to generate title")
        );

        let failed_generation =
            handle_generate_title_request(true, Ok(json!({ "message": "hello" })), |_| None);
        assert_eq!(failed_generation.status, 500);
        assert_eq!(
            parse_generated_session_title(&"x".repeat(80))
                .expect("title")
                .chars()
                .count(),
            60
        );
    }
}
