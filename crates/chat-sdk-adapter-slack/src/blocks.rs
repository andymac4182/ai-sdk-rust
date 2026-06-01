//! Slack Block Kit subpath.
//!
//! 1:1 with upstream `packages/adapter-slack/src/blocks/index.ts`.
//! Card-to-Block-Kit rendering lives in [`crate::cards`]; this module
//! owns the `blocks` subpath aliases plus the input-request helpers
//! introduced by the current upstream drift.

use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
use serde_json::{Value, json};

pub use crate::cards::{
    SlackBlock, SlackBlockKitOptions, card_to_block_kit, card_to_block_kit as card_to_slack_blocks,
    card_to_block_kit_with_options, card_to_fallback_text_slack as card_to_fallback_text,
    card_to_fallback_text_slack as card_to_slack_fallback_text,
};

pub const SLACK_INPUT_ACTION_PREFIX: &str = "input:";
pub const SLACK_FREEFORM_ACTION_PREFIX: &str = "input-freeform:";
pub const SLACK_FREEFORM_CALLBACK_ID: &str = "input-freeform-submit";
pub const SLACK_FREEFORM_BLOCK_ID: &str = "input-freeform-block";
pub const SLACK_FREEFORM_ACTION_ID: &str = "input-freeform-text";

const ACTIONS_ELEMENTS_LIMIT: usize = 25;
const BUTTON_TEXT_LIMIT: usize = 75;
const BUTTON_VALUE_LIMIT: usize = 2000;
const OPTION_TEXT_LIMIT: usize = 75;
const OPTION_VALUE_LIMIT: usize = 150;
const OPTIONS_LIMIT: usize = 100;
const RADIO_OPTIONS_LIMIT: usize = 10;
const SECTION_TEXT_LIMIT: usize = 3000;
const TITLE_LIMIT: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlackInputDisplay {
    Buttons,
    Radio,
    Select,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackInputOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub style: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackInputRequest {
    pub allow_freeform: bool,
    pub display: SlackInputDisplay,
    pub options: Vec<SlackInputOption>,
    pub prompt: String,
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackInputAction {
    pub action_id: String,
    pub selected_option_value: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackInputResponse {
    pub option_id: Option<String>,
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SlackFreeformViewOptions {
    pub metadata: Value,
    pub prompt: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackFreeformValue {
    pub action_id: String,
    pub block_id: String,
    pub value: Option<String>,
}

pub fn convert_slack_emoji_placeholders(text: &str) -> String {
    convert_emoji_placeholders(text, PlaceholderPlatform::Slack, None)
}

pub fn input_request_to_slack_blocks(request: &SlackInputRequest) -> Vec<SlackBlock> {
    let prompt = json!({
        "type": "section",
        "text": {
            "type": "mrkdwn",
            "text": truncate(&request.prompt, SECTION_TEXT_LIMIT),
        },
    });
    let extras = if request.allow_freeform {
        vec![freeform_button(&request.request_id)]
    } else {
        Vec::new()
    };

    let elements = if request.options.is_empty() {
        vec![freeform_button(&request.request_id)]
    } else if request.display == SlackInputDisplay::Radio {
        let mut elements = vec![radio_element(request)];
        elements.extend(extras);
        elements
    } else if request.display == SlackInputDisplay::Select {
        let mut elements = vec![select_element(request)];
        elements.extend(extras);
        elements
    } else {
        let limit = if request.allow_freeform {
            ACTIONS_ELEMENTS_LIMIT - 1
        } else {
            ACTIONS_ELEMENTS_LIMIT
        };
        let mut elements: Vec<Value> = request
            .options
            .iter()
            .take(limit)
            .enumerate()
            .map(|(index, option)| button_element(&request.request_id, option, index))
            .collect();
        elements.extend(extras);
        elements
    };

    vec![prompt, json!({ "type": "actions", "elements": elements })]
}

pub fn parse_slack_input_response(action: &SlackInputAction) -> Option<SlackInputResponse> {
    let id = action.action_id.strip_prefix(SLACK_INPUT_ACTION_PREFIX)?;
    if let Some(selected) = &action.selected_option_value {
        return (!id.is_empty()).then(|| SlackInputResponse {
            option_id: Some(selected.clone()),
            request_id: id.to_string(),
        });
    }

    let (request_id, _) = id.rsplit_once(":button:")?;
    let value = action.value.clone()?;
    (!request_id.is_empty()).then(|| SlackInputResponse {
        option_id: Some(value),
        request_id: request_id.to_string(),
    })
}

pub fn build_slack_freeform_view(options: &SlackFreeformViewOptions) -> Value {
    let title = truncate(
        options
            .title
            .as_deref()
            .or(options.prompt.as_deref())
            .unwrap_or("Your answer"),
        TITLE_LIMIT,
    );
    let mut blocks = Vec::new();
    if let Some(prompt) = &options.prompt {
        blocks.push(json!({
            "type": "section",
            "text": { "type": "mrkdwn", "text": truncate(prompt, SECTION_TEXT_LIMIT) },
        }));
    }
    blocks.push(json!({
        "type": "input",
        "block_id": SLACK_FREEFORM_BLOCK_ID,
        "element": {
            "type": "plain_text_input",
            "action_id": SLACK_FREEFORM_ACTION_ID,
            "multiline": true,
        },
        "label": { "type": "plain_text", "text": "Answer" },
    }));

    json!({
        "type": "modal",
        "callback_id": SLACK_FREEFORM_CALLBACK_ID,
        "title": { "type": "plain_text", "text": title },
        "close": { "type": "plain_text", "text": "Cancel" },
        "submit": { "type": "plain_text", "text": "Submit" },
        "private_metadata": metadata_string(&options.metadata),
        "blocks": blocks,
    })
}

pub fn parse_slack_freeform_value(values: &[SlackFreeformValue]) -> Option<String> {
    values
        .iter()
        .find(|value| {
            value.block_id == SLACK_FREEFORM_BLOCK_ID && value.action_id == SLACK_FREEFORM_ACTION_ID
        })
        .and_then(|value| value.value.clone())
}

pub fn answered_slack_input_blocks(
    answer: &str,
    prompt_block: Option<Value>,
    user_id: Option<&str>,
) -> Vec<SlackBlock> {
    let mut blocks = Vec::new();
    if let Some(prompt) = prompt_block.filter(Value::is_object) {
        blocks.push(prompt);
    }
    blocks.push(json!({
        "type": "section",
        "text": { "type": "mrkdwn", "text": format!(":white_check_mark: *{answer}*") },
    }));
    if let Some(user_id) = user_id {
        blocks.push(json!({
            "type": "context",
            "elements": [{ "type": "mrkdwn", "text": format!("Answered by <@{user_id}>") }],
        }));
    }
    blocks
}

fn freeform_button(request_id: &str) -> Value {
    json!({
        "type": "button",
        "action_id": format!("{SLACK_FREEFORM_ACTION_PREFIX}{request_id}"),
        "style": "primary",
        "text": { "type": "plain_text", "text": "Type your answer" },
        "value": request_id,
    })
}

fn button_element(request_id: &str, option: &SlackInputOption, index: usize) -> Value {
    let mut button = serde_json::Map::new();
    button.insert("type".to_string(), json!("button"));
    button.insert(
        "action_id".to_string(),
        json!(format!(
            "{SLACK_INPUT_ACTION_PREFIX}{request_id}:button:{index}"
        )),
    );
    button.insert(
        "text".to_string(),
        json!({ "type": "plain_text", "text": truncate(&option.label, BUTTON_TEXT_LIMIT) }),
    );
    button.insert(
        "value".to_string(),
        json!(truncate(&option.id, BUTTON_VALUE_LIMIT)),
    );
    if matches!(option.style.as_deref(), Some("primary" | "danger")) {
        button.insert("style".to_string(), json!(option.style.as_deref().unwrap()));
    }
    Value::Object(button)
}

fn select_element(request: &SlackInputRequest) -> Value {
    json!({
        "type": "static_select",
        "action_id": format!("{SLACK_INPUT_ACTION_PREFIX}{}", request.request_id),
        "placeholder": { "type": "plain_text", "text": "Choose an option" },
        "options": request.options.iter().take(OPTIONS_LIMIT).map(input_option).collect::<Vec<_>>(),
    })
}

fn radio_element(request: &SlackInputRequest) -> Value {
    let options: Vec<Value> = request.options.iter().map(input_option).collect();
    if options.len() > RADIO_OPTIONS_LIMIT {
        return select_element(request);
    }
    json!({
        "type": "radio_buttons",
        "action_id": format!("{SLACK_INPUT_ACTION_PREFIX}{}", request.request_id),
        "options": options,
    })
}

fn input_option(option: &SlackInputOption) -> Value {
    json!({
        "text": { "type": "plain_text", "text": truncate(&option.label, OPTION_TEXT_LIMIT) },
        "value": truncate(&option.id, OPTION_VALUE_LIMIT),
    })
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn metadata_string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::cards::{
        ActionsChild, ActionsElement, ActionsKind, ButtonElement, ButtonKind, CardChild,
        CardElement, CardKind, FieldElement, FieldKind, FieldsElement, FieldsKind, ImageElement,
        ImageKind, LinkElement, LinkKind, TableElement, TableKind, TextElement, TextKind,
        TextStyle,
    };
    use chat_sdk_chat::modals::{
        RadioSelectElement, RadioSelectKind, SelectElement, SelectKind, SelectOptionElement,
    };

    fn card(children: Vec<CardChild>) -> CardElement {
        CardElement {
            title: None,
            subtitle: None,
            image_url: None,
            children,
            kind: CardKind::Card,
        }
    }

    fn text(content: &str) -> CardChild {
        CardChild::Text(TextElement {
            content: content.to_string(),
            style: None,
            kind: TextKind::Text,
        })
    }

    fn option(id: &str, label: &str) -> SlackInputOption {
        SlackInputOption {
            id: id.to_string(),
            label: label.to_string(),
            description: None,
            style: None,
        }
    }

    fn select_option(label: &str, value: &str) -> SelectOptionElement {
        SelectOptionElement {
            label: label.to_string(),
            value: value.to_string(),
            description: None,
        }
    }

    #[test]
    fn blocks_converts_card_headers_and_context() {
        let mut card = card(vec![]);
        card.title = Some("Order".to_string());
        card.subtitle = Some("Status changed".to_string());
        card.image_url = Some("https://example.com/image.png".to_string());

        assert_eq!(
            card_to_slack_blocks(&card),
            vec![
                json!({ "type": "header", "text": { "type": "plain_text", "text": "Order", "emoji": true } }),
                json!({ "type": "context", "elements": [{ "type": "mrkdwn", "text": "Status changed" }] }),
                json!({ "type": "image", "image_url": "https://example.com/image.png", "alt_text": "Order" }),
            ]
        );
    }

    #[test]
    fn blocks_truncates_header_text_to_slacks_header_block_limit() {
        let mut card = card(vec![]);
        card.title = Some("a".repeat(200));
        assert_eq!(
            card_to_slack_blocks(&card)[0]["text"]["text"],
            "a".repeat(150)
        );
    }

    #[test]
    fn blocks_truncates_image_urls_to_slacks_image_block_limit() {
        let long_url = format!("https://example.com/{}", "a".repeat(4000));
        let mut top = card(vec![]);
        top.title = Some("{{emoji:frame}}".to_string());
        top.image_url = Some(long_url.clone());
        let top_blocks = card_to_slack_blocks(&top);
        assert_eq!(top_blocks[0]["text"]["text"], ":frame:");
        assert_eq!(
            top_blocks[1]["image_url"],
            format!("https://example.com/{}", "a".repeat(2980))
        );

        let child = card(vec![CardChild::Image(ImageElement {
            alt: None,
            kind: ImageKind::Image,
            url: long_url,
        })]);
        assert_eq!(
            card_to_slack_blocks(&child)[0]["image_url"],
            format!("https://example.com/{}", "a".repeat(2980))
        );
    }

    #[test]
    fn blocks_converts_text_and_links() {
        let blocks = card_to_slack_blocks(&card(vec![
            text("plain"),
            CardChild::Text(TextElement {
                content: "bold".to_string(),
                style: Some(TextStyle::Bold),
                kind: TextKind::Text,
            }),
            CardChild::Text(TextElement {
                content: "muted".to_string(),
                style: Some(TextStyle::Muted),
                kind: TextKind::Text,
            }),
            CardChild::Link(LinkElement {
                label: "Docs".to_string(),
                kind: LinkKind::Link,
                url: "https://example.com".to_string(),
            }),
        ]));

        assert_eq!(
            blocks[0],
            json!({ "type": "section", "text": { "type": "mrkdwn", "text": "plain" } })
        );
        assert_eq!(blocks[1]["text"]["text"], "*bold*");
        assert_eq!(blocks[2]["type"], "context");
        assert_eq!(blocks[3]["text"]["text"], "<https://example.com|Docs>");
    }

    #[test]
    fn blocks_converts_actions() {
        let blocks = card_to_slack_blocks(&card(vec![CardChild::Actions(ActionsElement {
            children: vec![
                ActionsChild::Button(ButtonElement {
                    action_type: None,
                    callback_url: None,
                    disabled: None,
                    id: "approve".to_string(),
                    label: "Approve".to_string(),
                    style: Some(chat_sdk_chat::cards::ButtonStyle::Primary),
                    kind: ButtonKind::Button,
                    value: None,
                }),
                ActionsChild::Select(SelectElement {
                    id: "status".to_string(),
                    initial_option: None,
                    label: "Status".to_string(),
                    optional: None,
                    options: vec![
                        select_option("Open", "open"),
                        select_option("Closed", "closed"),
                    ],
                    placeholder: Some("Choose".to_string()),
                    kind: SelectKind::Select,
                }),
                ActionsChild::RadioSelect(RadioSelectElement {
                    id: "plan".to_string(),
                    initial_option: None,
                    label: "Plan".to_string(),
                    optional: None,
                    options: vec![SelectOptionElement {
                        label: "Pro".to_string(),
                        value: "pro".to_string(),
                        description: Some("For teams".to_string()),
                    }],
                    kind: RadioSelectKind::RadioSelect,
                }),
            ],
            kind: ActionsKind::Actions,
        })]));

        let elements = blocks[0]["elements"].as_array().unwrap();
        assert_eq!(elements[0]["type"], "button");
        assert_eq!(elements[0]["style"], "primary");
        assert_eq!(elements[1]["type"], "static_select");
        assert_eq!(elements[1]["placeholder"]["text"], "Choose");
        assert_eq!(elements[2]["type"], "radio_buttons");
        assert_eq!(elements[2]["options"][0]["description"]["type"], "mrkdwn");
    }

    #[test]
    fn blocks_limits_action_elements_and_select_options_to_slack_limits() {
        let action_children = (0..30)
            .map(|index| {
                ActionsChild::Button(ButtonElement {
                    action_type: None,
                    callback_url: None,
                    disabled: None,
                    id: format!("b{index}"),
                    label: format!("Button {index}"),
                    style: None,
                    kind: ButtonKind::Button,
                    value: None,
                })
            })
            .collect();
        let many_options = (0..120)
            .map(|index| select_option(&format!("Option {index}"), &format!("value-{index}")))
            .collect();

        let blocks = card_to_slack_blocks(&card(vec![
            CardChild::Actions(ActionsElement {
                children: action_children,
                kind: ActionsKind::Actions,
            }),
            CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::Select(SelectElement {
                    id: "select".to_string(),
                    initial_option: None,
                    label: "Select".to_string(),
                    optional: None,
                    options: many_options,
                    placeholder: None,
                    kind: SelectKind::Select,
                })],
                kind: ActionsKind::Actions,
            }),
        ]));

        assert_eq!(blocks[0]["elements"].as_array().unwrap().len(), 25);
        assert_eq!(
            blocks[1]["elements"][0]["options"]
                .as_array()
                .unwrap()
                .len(),
            100
        );
    }

    #[test]
    fn blocks_truncates_option_values_to_slacks_option_object_limit() {
        let blocks = card_to_slack_blocks(&card(vec![CardChild::Actions(ActionsElement {
            children: vec![ActionsChild::Select(SelectElement {
                id: "select".to_string(),
                initial_option: None,
                label: "Select".to_string(),
                optional: None,
                options: vec![select_option("Option", &"v".repeat(200))],
                placeholder: None,
                kind: SelectKind::Select,
            })],
            kind: ActionsKind::Actions,
        })]));
        assert_eq!(
            blocks[0]["elements"][0]["options"][0]["value"],
            "v".repeat(150)
        );
    }

    #[test]
    fn blocks_matches_truncated_initial_options_for_select_elements() {
        let long_value = "v".repeat(200);
        let blocks = card_to_slack_blocks(&card(vec![CardChild::Actions(ActionsElement {
            children: vec![
                ActionsChild::Select(SelectElement {
                    id: "select".to_string(),
                    initial_option: Some(long_value.clone()),
                    label: "Select".to_string(),
                    optional: None,
                    options: vec![select_option("Option", &long_value)],
                    placeholder: None,
                    kind: SelectKind::Select,
                }),
                ActionsChild::RadioSelect(RadioSelectElement {
                    id: "radio".to_string(),
                    initial_option: Some(long_value.clone()),
                    label: "Radio".to_string(),
                    optional: None,
                    options: vec![select_option("Option", &long_value)],
                    kind: RadioSelectKind::RadioSelect,
                }),
            ],
            kind: ActionsKind::Actions,
        })]));
        assert_eq!(
            blocks[0]["elements"][0]["initial_option"]["value"],
            "v".repeat(150)
        );
        assert_eq!(
            blocks[0]["elements"][1]["initial_option"]["value"],
            "v".repeat(150)
        );
    }

    #[test]
    fn blocks_omits_initial_options_when_no_initial_value_is_provided() {
        let blocks = card_to_slack_blocks(&card(vec![CardChild::Actions(ActionsElement {
            children: vec![ActionsChild::Select(SelectElement {
                id: "select".to_string(),
                initial_option: None,
                label: "Select".to_string(),
                optional: None,
                options: vec![select_option("Option", "")],
                placeholder: None,
                kind: SelectKind::Select,
            })],
            kind: ActionsKind::Actions,
        })]));
        assert!(blocks[0]["elements"][0].get("initial_option").is_none());
    }

    #[test]
    fn blocks_converts_fields_and_tables() {
        let blocks = card_to_slack_blocks(&card(vec![
            CardChild::Fields(FieldsElement {
                children: vec![
                    FieldElement {
                        label: "Name".to_string(),
                        kind: FieldKind::Field,
                        value: "Ada".to_string(),
                    },
                    FieldElement {
                        label: "Role".to_string(),
                        kind: FieldKind::Field,
                        value: "Engineer".to_string(),
                    },
                ],
                kind: FieldsKind::Fields,
            }),
            CardChild::Table(TableElement {
                align: Some(vec![
                    chat_sdk_chat::cards::TableAlignment::Left,
                    chat_sdk_chat::cards::TableAlignment::Right,
                ]),
                headers: vec!["Name".to_string(), "Score".to_string()],
                rows: vec![vec!["Ada".to_string(), "10".to_string()]],
                kind: TableKind::Table,
            }),
        ]));

        assert_eq!(blocks[0]["fields"][0]["text"], "*Name*\nAda");
        assert_eq!(
            blocks[1]["column_settings"],
            json!([{ "align": "left" }, { "align": "right" }])
        );
        assert_eq!(blocks[1]["rows"][1][0]["text"], "Ada");
    }

    #[test]
    fn blocks_falls_back_to_ascii_tables_after_one_native_table() {
        let table = CardChild::Table(TableElement {
            align: None,
            headers: vec!["A".to_string(), "B".to_string()],
            rows: vec![vec!["1".to_string(), "2".to_string()]],
            kind: TableKind::Table,
        });
        let blocks = card_to_slack_blocks(&card(vec![table.clone(), table]));
        assert_eq!(
            blocks[1],
            json!({ "type": "section", "text": { "type": "mrkdwn", "text": "```\nA | B\n1 | 2\n```" } })
        );
    }

    #[test]
    fn blocks_generates_slack_fallback_text() {
        let result = card_to_slack_fallback_text(&card(vec![
            text("Hello"),
            CardChild::Fields(FieldsElement {
                children: vec![FieldElement {
                    label: "Status".to_string(),
                    kind: FieldKind::Field,
                    value: "Ready".to_string(),
                }],
                kind: FieldsKind::Fields,
            }),
        ]));
        assert!(result.contains("Hello"));
        assert!(result.contains("Status: Ready"));
    }

    #[test]
    fn blocks_subpath_keeps_compatibility_aliases() {
        let input = card(vec![text("hello")]);
        assert_eq!(card_to_block_kit(&input), card_to_slack_blocks(&input));
        assert_eq!(
            card_to_fallback_text(&input),
            card_to_slack_fallback_text(&input)
        );
    }

    #[test]
    fn blocks_supports_custom_emoji_conversion() {
        let input = card(vec![text("{{emoji:thumbs_up}}")]);
        assert_eq!(
            card_to_slack_blocks(&input)[0]["text"]["text"],
            ":thumbs_up:"
        );
        let convert = |_text: &str| ":+1:".to_string();
        assert_eq!(
            card_to_block_kit_with_options(
                &input,
                SlackBlockKitOptions {
                    convert_emoji: Some(&convert),
                    max_blocks: None,
                },
            )[0]["text"]["text"],
            ":+1:"
        );
        assert_eq!(
            convert_slack_emoji_placeholders("hi {{emoji:wave}}"),
            "hi :wave:"
        );
    }

    #[test]
    fn input_request_renders_input_requests_as_slack_buttons() {
        let request = SlackInputRequest {
            allow_freeform: false,
            display: SlackInputDisplay::Buttons,
            options: vec![
                SlackInputOption {
                    style: Some("primary".to_string()),
                    ..option("approve", "Approve")
                },
                SlackInputOption {
                    style: Some("danger".to_string()),
                    ..option("deny", "Deny")
                },
            ],
            prompt: "Approve deploy?".to_string(),
            request_id: "req-1".to_string(),
        };
        assert_eq!(
            input_request_to_slack_blocks(&request)[1]["elements"][0]["action_id"],
            "input:req-1:button:0"
        );
        assert_eq!(
            input_request_to_slack_blocks(&request)[1]["elements"][1]["style"],
            "danger"
        );
    }

    #[test]
    fn input_request_renders_input_requests_as_selects() {
        let request = SlackInputRequest {
            allow_freeform: false,
            display: SlackInputDisplay::Select,
            options: vec![option("one", "One")],
            prompt: "Pick one".to_string(),
            request_id: "req-1".to_string(),
        };
        assert_eq!(
            input_request_to_slack_blocks(&request)[1],
            json!({
                "type": "actions",
                "elements": [{
                    "type": "static_select",
                    "action_id": "input:req-1",
                    "placeholder": { "type": "plain_text", "text": "Choose an option" },
                    "options": [{ "text": { "type": "plain_text", "text": "One" }, "value": "one" }],
                }],
            })
        );
    }

    #[test]
    fn input_request_renders_input_requests_as_radios() {
        let request = SlackInputRequest {
            allow_freeform: false,
            display: SlackInputDisplay::Radio,
            options: vec![option("one", "One")],
            prompt: "Pick one".to_string(),
            request_id: "req-1".to_string(),
        };
        assert_eq!(
            input_request_to_slack_blocks(&request)[1]["elements"][0]["type"],
            "radio_buttons"
        );
    }

    #[test]
    fn input_request_renders_freeform_alongside_options_when_allowed() {
        let request = SlackInputRequest {
            allow_freeform: true,
            display: SlackInputDisplay::Buttons,
            options: vec![option("approve", "Approve")],
            prompt: "Approve deploy?".to_string(),
            request_id: "req-1".to_string(),
        };
        let elements = input_request_to_slack_blocks(&request)[1]["elements"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(elements[1]["action_id"], "input-freeform:req-1");
        assert_eq!(elements[1]["value"], "req-1");
    }

    #[test]
    fn input_request_renders_and_reads_freeform_input_modals() {
        let view = build_slack_freeform_view(&SlackFreeformViewOptions {
            metadata: json!({ "requestId": "req-1" }),
            prompt: Some("Tell me why".to_string()),
            title: None,
        });
        assert_eq!(view["callback_id"], "input-freeform-submit");
        assert_eq!(view["private_metadata"], r#"{"requestId":"req-1"}"#);
        assert_eq!(
            parse_slack_freeform_value(&[SlackFreeformValue {
                action_id: "input-freeform-text".to_string(),
                block_id: "input-freeform-block".to_string(),
                value: Some("because".to_string()),
            }])
            .as_deref(),
            Some("because")
        );
    }

    #[test]
    fn input_request_parses_input_actions_and_answered_blocks() {
        assert_eq!(
            parse_slack_input_response(&SlackInputAction {
                action_id: "input:req-1:button:0".to_string(),
                selected_option_value: None,
                value: Some("approve".to_string()),
            }),
            Some(SlackInputResponse {
                option_id: Some("approve".to_string()),
                request_id: "req-1".to_string(),
            })
        );
        assert_eq!(
            parse_slack_input_response(&SlackInputAction {
                action_id: "input:req-2".to_string(),
                selected_option_value: Some("later".to_string()),
                value: None,
            }),
            Some(SlackInputResponse {
                option_id: Some("later".to_string()),
                request_id: "req-2".to_string(),
            })
        );
        assert_eq!(
            answered_slack_input_blocks("Approve", None, Some("U123")),
            vec![
                json!({ "type": "section", "text": { "type": "mrkdwn", "text": ":white_check_mark: *Approve*" } }),
                json!({ "type": "context", "elements": [{ "type": "mrkdwn", "text": "Answered by <@U123>" }] }),
            ]
        );
    }

    #[test]
    fn blocks_import_boundary_does_not_import_runtime_packages() {
        let source = include_str!("blocks.rs");
        let forbidden_runtime = concat!("use ", "chat_sdk_adapter_shared::runtime");
        let forbidden_http = concat!("req", "west");
        assert!(!source.contains(forbidden_runtime));
        assert!(!source.contains(forbidden_http));
    }
}
