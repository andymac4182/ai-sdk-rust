use std::marker::PhantomData;

use serde::de::DeserializeOwned;

use crate::generate_text::NoObjectGeneratedError;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    FinishReason, LanguageModelResponse, LanguageModelResponseFormat, LanguageModelUsage,
};
use crate::provider::TypeValidationError;
use crate::provider_utils::{FlexibleSchema, safe_parse_json, safe_validate_types};
use crate::util::parse_partial_json;

/// Context passed to complete-output parsers.
#[derive(Clone, Debug)]
pub struct OutputParseContext<'a> {
    /// Response metadata from the model call.
    pub response: &'a LanguageModelResponse,

    /// Usage metadata from the model call.
    pub usage: &'a LanguageModelUsage,

    /// Unified finish reason from the model call.
    pub finish_reason: &'a FinishReason,
}

impl<'a> OutputParseContext<'a> {
    /// Creates parser context from response, usage, and finish reason.
    pub fn new(
        response: &'a LanguageModelResponse,
        usage: &'a LanguageModelUsage,
        finish_reason: &'a FinishReason,
    ) -> Self {
        Self {
            response,
            usage,
            finish_reason,
        }
    }
}

/// Output specification for plain text generation.
#[derive(Clone, Debug, Default)]
pub struct TextOutput {
    name: &'static str,
}

impl TextOutput {
    /// Returns the output mode name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the response format for text output.
    pub fn response_format(&self) -> LanguageModelResponseFormat {
        LanguageModelResponseFormat::text()
    }

    /// Parses the complete text output.
    pub fn parse_complete_output(&self, text: Option<&str>) -> Option<String> {
        text.map(str::to_owned)
    }

    /// Parses a partial text output.
    pub fn parse_partial_output(&self, text: Option<&str>) -> Option<String> {
        text.map(str::to_owned)
    }
}

/// Creates a plain text output specification.
pub fn text() -> TextOutput {
    TextOutput { name: "text" }
}

/// Output specification for typed object generation.
#[derive(Clone, Debug)]
pub struct ObjectOutput<OBJECT> {
    schema: FlexibleSchema<OBJECT>,
    name: Option<String>,
    description: Option<String>,
}

impl<OBJECT> ObjectOutput<OBJECT>
where
    OBJECT: DeserializeOwned + Clone + 'static,
{
    /// Returns the output mode name.
    pub fn name(&self) -> &'static str {
        "object"
    }

    /// Sets the output name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the output description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Returns the response format for object output.
    pub fn response_format(&self) -> LanguageModelResponseFormat {
        let schema = self.schema.clone().into_schema();
        let mut response_format =
            LanguageModelResponseFormat::json().with_schema(schema.json_schema().clone());

        if let Some(name) = &self.name {
            response_format = response_format.with_name(name.clone());
        }

        if let Some(description) = &self.description {
            response_format = response_format.with_description(description.clone());
        }

        response_format
    }

    /// Parses the complete object output.
    #[allow(clippy::result_large_err)]
    #[allow(clippy::result_large_err)]
    #[allow(clippy::result_large_err)]
    pub fn parse_complete_output(
        &self,
        text: &str,
        context: OutputParseContext<'_>,
    ) -> Result<OBJECT, NoObjectGeneratedError> {
        let parse_result = safe_parse_json(text);
        let parsed_value = match parse_result {
            crate::provider_utils::ParseJsonResult::Success { value, .. } => value,
            crate::provider_utils::ParseJsonResult::Failure { error, .. } => {
                return Err(NoObjectGeneratedError::with_message(
                    "No object generated: could not parse the response.",
                    context.response.clone(),
                    context.usage.clone(),
                    context.finish_reason.clone(),
                )
                .with_text(text)
                .with_cause(error));
            }
        };

        match safe_validate_types(parsed_value, self.schema.clone(), None) {
            crate::provider_utils::ValidateTypesResult::Success { value, .. } => Ok(value),
            crate::provider_utils::ValidateTypesResult::Failure { error, .. } => {
                Err(NoObjectGeneratedError::with_message(
                    "No object generated: response did not match schema.",
                    context.response.clone(),
                    context.usage.clone(),
                    context.finish_reason.clone(),
                )
                .with_text(text)
                .with_cause(error))
            }
        }
    }

    /// Parses a partial object output.
    pub fn parse_partial_output(&self, text: Option<&str>) -> Option<JsonValue> {
        let result = parse_partial_json(text);

        match result.state() {
            crate::util::ParsePartialJsonState::FailedParse
            | crate::util::ParsePartialJsonState::UndefinedInput => None,
            crate::util::ParsePartialJsonState::SuccessfulParse
            | crate::util::ParsePartialJsonState::RepairedParse => result.value().cloned(),
        }
    }
}

/// Creates an object output specification.
pub fn object<OBJECT>(schema: impl Into<FlexibleSchema<OBJECT>>) -> ObjectOutput<OBJECT>
where
    OBJECT: DeserializeOwned + Clone + 'static,
{
    ObjectOutput {
        schema: schema.into(),
        name: None,
        description: None,
    }
}

/// Output specification for array generation.
#[derive(Clone, Debug)]
pub struct ArrayOutput<ELEMENT> {
    element_schema: FlexibleSchema<ELEMENT>,
    name: Option<String>,
    description: Option<String>,
}

impl<ELEMENT> ArrayOutput<ELEMENT>
where
    ELEMENT: DeserializeOwned + Clone + 'static,
{
    /// Returns the output mode name.
    pub fn name(&self) -> &'static str {
        "array"
    }

    /// Sets the output name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the output description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Returns the response format for array output.
    pub fn response_format(&self) -> LanguageModelResponseFormat {
        let element_schema = self.element_schema.clone().into_schema();
        let mut item_schema = element_schema.json_schema().clone();
        item_schema.remove("$schema");

        let schema = JsonObject::from_iter([
            (
                "$schema".to_string(),
                JsonValue::String("http://json-schema.org/draft-07/schema#".to_string()),
            ),
            ("type".to_string(), JsonValue::String("object".to_string())),
            (
                "properties".to_string(),
                JsonValue::Object(JsonObject::from_iter([(
                    "elements".to_string(),
                    JsonValue::Object(JsonObject::from_iter([
                        ("type".to_string(), JsonValue::String("array".to_string())),
                        ("items".to_string(), JsonValue::Object(item_schema)),
                    ])),
                )])),
            ),
            (
                "required".to_string(),
                JsonValue::Array(vec![JsonValue::String("elements".to_string())]),
            ),
            ("additionalProperties".to_string(), JsonValue::Bool(false)),
        ]);

        let mut response_format = LanguageModelResponseFormat::json().with_schema(schema);

        if let Some(name) = &self.name {
            response_format = response_format.with_name(name.clone());
        }

        if let Some(description) = &self.description {
            response_format = response_format.with_description(description.clone());
        }

        response_format
    }

    /// Parses the complete array output.
    #[allow(clippy::result_large_err)]
    pub fn parse_complete_output(
        &self,
        text: &str,
        context: OutputParseContext<'_>,
    ) -> Result<Vec<ELEMENT>, NoObjectGeneratedError> {
        let parse_result = safe_parse_json(text);
        let outer_value = match parse_result {
            crate::provider_utils::ParseJsonResult::Success { value, .. } => value,
            crate::provider_utils::ParseJsonResult::Failure { error, .. } => {
                return Err(NoObjectGeneratedError::with_message(
                    "No object generated: could not parse the response.",
                    context.response.clone(),
                    context.usage.clone(),
                    context.finish_reason.clone(),
                )
                .with_text(text)
                .with_cause(error));
            }
        };

        let Some(elements) = outer_value
            .as_object()
            .and_then(|object| object.get("elements"))
        else {
            return Err(NoObjectGeneratedError::with_message(
                "No object generated: response did not match schema.",
                context.response.clone(),
                context.usage.clone(),
                context.finish_reason.clone(),
            )
            .with_text(text)
            .with_cause(TypeValidationError::with_cause_message(
                outer_value,
                "response must be an object with an elements array",
                None,
            )));
        };

        let Some(elements) = elements.as_array() else {
            return Err(NoObjectGeneratedError::with_message(
                "No object generated: response did not match schema.",
                context.response.clone(),
                context.usage.clone(),
                context.finish_reason.clone(),
            )
            .with_text(text)
            .with_cause(TypeValidationError::with_cause_message(
                outer_value,
                "response must be an object with an elements array",
                None,
            )));
        };

        let mut parsed_elements = Vec::with_capacity(elements.len());
        for element in elements {
            match safe_validate_types(element.clone(), self.element_schema.clone(), None) {
                crate::provider_utils::ValidateTypesResult::Success { value, .. } => {
                    parsed_elements.push(value);
                }
                crate::provider_utils::ValidateTypesResult::Failure { error, .. } => {
                    return Err(NoObjectGeneratedError::with_message(
                        "No object generated: response did not match schema.",
                        context.response.clone(),
                        context.usage.clone(),
                        context.finish_reason.clone(),
                    )
                    .with_text(text)
                    .with_cause(error));
                }
            }
        }

        Ok(parsed_elements)
    }

    /// Parses a partial array output.
    pub fn parse_partial_output(&self, text: Option<&str>) -> Option<Vec<ELEMENT>> {
        let result = parse_partial_json(text);
        let (value, state) = result.into_parts();
        let outer_value = match state {
            crate::util::ParsePartialJsonState::FailedParse
            | crate::util::ParsePartialJsonState::UndefinedInput => return None,
            crate::util::ParsePartialJsonState::SuccessfulParse
            | crate::util::ParsePartialJsonState::RepairedParse => value?,
        };

        let elements = outer_value
            .as_object()
            .and_then(|object| object.get("elements"))?;

        let elements = elements.as_array()?;

        let raw_elements: &[JsonValue] =
            if matches!(state, crate::util::ParsePartialJsonState::RepairedParse)
                && !elements.is_empty()
            {
                &elements[..elements.len() - 1]
            } else {
                elements
            };

        let mut parsed_elements = Vec::new();
        for element in raw_elements {
            if let crate::provider_utils::ValidateTypesResult::Success { value, .. } =
                safe_validate_types(element.clone(), self.element_schema.clone(), None)
            {
                parsed_elements.push(value);
            }
        }

        Some(parsed_elements)
    }

    /// Creates a transform that emits newly available elements on each update.
    pub fn create_element_stream_transform(&self) -> ArrayOutputElementStreamTransform<ELEMENT> {
        ArrayOutputElementStreamTransform::new()
    }
}

/// Creates an array output specification.
pub fn array<ELEMENT>(element: impl Into<FlexibleSchema<ELEMENT>>) -> ArrayOutput<ELEMENT>
where
    ELEMENT: DeserializeOwned + Clone + 'static,
{
    ArrayOutput {
        element_schema: element.into(),
        name: None,
        description: None,
    }
}

/// Stateful element transformer for array outputs.
#[derive(Clone, Debug, Default)]
pub struct ArrayOutputElementStreamTransform<ELEMENT> {
    published_elements: usize,
    _marker: PhantomData<ELEMENT>,
}

impl<ELEMENT> ArrayOutputElementStreamTransform<ELEMENT> {
    /// Creates a new element transformer.
    pub fn new() -> Self {
        Self {
            published_elements: 0,
            _marker: PhantomData,
        }
    }

    /// Emits the elements that have not been published yet.
    pub fn transform(&mut self, partial_output: &[ELEMENT]) -> Vec<ELEMENT>
    where
        ELEMENT: Clone,
    {
        let mut output = Vec::new();
        while self.published_elements < partial_output.len() {
            output.push(partial_output[self.published_elements].clone());
            self.published_elements += 1;
        }
        output
    }
}

/// Output specification for choice generation.
#[derive(Clone, Debug)]
pub struct ChoiceOutput {
    options: Vec<String>,
    name: Option<String>,
    description: Option<String>,
}

impl ChoiceOutput {
    /// Returns the output mode name.
    pub fn name(&self) -> &'static str {
        "choice"
    }

    /// Sets the output name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the output description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Returns the response format for choice output.
    pub fn response_format(&self) -> LanguageModelResponseFormat {
        let mut response_format =
            LanguageModelResponseFormat::json().with_schema(JsonObject::from_iter([
                (
                    "$schema".to_string(),
                    JsonValue::String("http://json-schema.org/draft-07/schema#".to_string()),
                ),
                ("type".to_string(), JsonValue::String("object".to_string())),
                (
                    "properties".to_string(),
                    JsonValue::Object(JsonObject::from_iter([(
                        "result".to_string(),
                        JsonValue::Object(JsonObject::from_iter([
                            ("type".to_string(), JsonValue::String("string".to_string())),
                            (
                                "enum".to_string(),
                                JsonValue::Array(
                                    self.options
                                        .iter()
                                        .cloned()
                                        .map(JsonValue::String)
                                        .collect(),
                                ),
                            ),
                        ])),
                    )])),
                ),
                (
                    "required".to_string(),
                    JsonValue::Array(vec![JsonValue::String("result".to_string())]),
                ),
                ("additionalProperties".to_string(), JsonValue::Bool(false)),
            ]));

        if let Some(name) = &self.name {
            response_format = response_format.with_name(name.clone());
        }

        if let Some(description) = &self.description {
            response_format = response_format.with_description(description.clone());
        }

        response_format
    }

    /// Parses the complete choice output.
    #[allow(clippy::result_large_err)]
    pub fn parse_complete_output(
        &self,
        text: &str,
        context: OutputParseContext<'_>,
    ) -> Result<String, NoObjectGeneratedError> {
        let parse_result = safe_parse_json(text);
        let outer_value = match parse_result {
            crate::provider_utils::ParseJsonResult::Success { value, .. } => value,
            crate::provider_utils::ParseJsonResult::Failure { error, .. } => {
                return Err(NoObjectGeneratedError::with_message(
                    "No object generated: could not parse the response.",
                    context.response.clone(),
                    context.usage.clone(),
                    context.finish_reason.clone(),
                )
                .with_text(text)
                .with_cause(error));
            }
        };

        let Some(result) = outer_value
            .as_object()
            .and_then(|object| object.get("result"))
        else {
            return Err(NoObjectGeneratedError::with_message(
                "No object generated: response did not match schema.",
                context.response.clone(),
                context.usage.clone(),
                context.finish_reason.clone(),
            )
            .with_text(text)
            .with_cause(TypeValidationError::with_cause_message(
                outer_value,
                "response must be an object that contains a choice value.",
                None,
            )));
        };

        let Some(result) = result.as_str() else {
            return Err(NoObjectGeneratedError::with_message(
                "No object generated: response did not match schema.",
                context.response.clone(),
                context.usage.clone(),
                context.finish_reason.clone(),
            )
            .with_text(text)
            .with_cause(TypeValidationError::with_cause_message(
                outer_value,
                "response must be an object that contains a choice value.",
                None,
            )));
        };

        if self.options.iter().any(|option| option == result) {
            Ok(result.to_string())
        } else {
            Err(NoObjectGeneratedError::with_message(
                "No object generated: response did not match schema.",
                context.response.clone(),
                context.usage.clone(),
                context.finish_reason.clone(),
            )
            .with_text(text)
            .with_cause(TypeValidationError::with_cause_message(
                outer_value,
                "response must be an object that contains a choice value.",
                None,
            )))
        }
    }

    /// Parses a partial choice output.
    pub fn parse_partial_output(&self, text: Option<&str>) -> Option<String> {
        let result = parse_partial_json(text);
        let (value, state) = result.into_parts();
        let outer_value = match state {
            crate::util::ParsePartialJsonState::FailedParse
            | crate::util::ParsePartialJsonState::UndefinedInput => return None,
            crate::util::ParsePartialJsonState::SuccessfulParse
            | crate::util::ParsePartialJsonState::RepairedParse => value?,
        };

        let result = outer_value
            .as_object()
            .and_then(|object| object.get("result"))?;

        let result = result.as_str()?;

        let potential_matches: Vec<&String> = self
            .options
            .iter()
            .filter(|option| option.starts_with(result))
            .collect();

        match state {
            crate::util::ParsePartialJsonState::SuccessfulParse => {
                if potential_matches
                    .iter()
                    .any(|option| option.as_str() == result)
                {
                    Some(result.to_string())
                } else {
                    None
                }
            }
            crate::util::ParsePartialJsonState::RepairedParse => {
                (potential_matches.len() == 1).then(|| potential_matches[0].to_string())
            }
            crate::util::ParsePartialJsonState::UndefinedInput
            | crate::util::ParsePartialJsonState::FailedParse => None,
        }
    }
}

/// Creates a choice output specification.
pub fn choice(options: Vec<String>) -> ChoiceOutput {
    ChoiceOutput {
        options,
        name: None,
        description: None,
    }
}

/// Output specification for unstructured JSON generation.
#[derive(Clone, Debug, Default)]
pub struct JsonOutput {
    name: Option<String>,
    description: Option<String>,
}

impl JsonOutput {
    /// Returns the output mode name.
    pub fn name(&self) -> &'static str {
        "json"
    }

    /// Sets the output name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the output description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Returns the response format for JSON output.
    pub fn response_format(&self) -> LanguageModelResponseFormat {
        let mut response_format = LanguageModelResponseFormat::json();

        if let Some(name) = &self.name {
            response_format = response_format.with_name(name.clone());
        }

        if let Some(description) = &self.description {
            response_format = response_format.with_description(description.clone());
        }

        response_format
    }

    /// Parses the complete JSON output.
    #[allow(clippy::result_large_err)]
    pub fn parse_complete_output(
        &self,
        text: &str,
        context: OutputParseContext<'_>,
    ) -> Result<JsonValue, NoObjectGeneratedError> {
        let parse_result = safe_parse_json(text);
        match parse_result {
            crate::provider_utils::ParseJsonResult::Success { value, .. } => Ok(value),
            crate::provider_utils::ParseJsonResult::Failure { error, .. } => {
                Err(NoObjectGeneratedError::with_message(
                    "No object generated: could not parse the response.",
                    context.response.clone(),
                    context.usage.clone(),
                    context.finish_reason.clone(),
                )
                .with_text(text)
                .with_cause(error))
            }
        }
    }

    /// Parses a partial JSON output.
    pub fn parse_partial_output(&self, text: Option<&str>) -> Option<JsonValue> {
        let result = parse_partial_json(text);

        match result.state() {
            crate::util::ParsePartialJsonState::FailedParse
            | crate::util::ParsePartialJsonState::UndefinedInput => None,
            crate::util::ParsePartialJsonState::SuccessfulParse
            | crate::util::ParsePartialJsonState::RepairedParse => result.value().cloned(),
        }
    }
}

/// Creates a JSON output specification.
pub fn json() -> JsonOutput {
    JsonOutput::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde::Deserialize;
    use serde_json::json;
    use std::iter::FromIterator;

    use crate::language_model::{
        InputTokenUsage, LanguageModelResponse, LanguageModelUsage, OutputTokenUsage,
    };
    use crate::provider_utils::{Schema, ValidationResult};

    fn test_context() -> (LanguageModelResponse, LanguageModelUsage, FinishReason) {
        (
            LanguageModelResponse::new()
                .with_id("123")
                .with_model_id("456"),
            LanguageModelUsage {
                input_tokens: InputTokenUsage {
                    total: Some(1),
                    no_cache: Some(1),
                    cache_read: None,
                    cache_write: None,
                },
                output_tokens: OutputTokenUsage {
                    total: Some(2),
                    text: Some(2),
                    reasoning: None,
                },
                raw: None,
            },
            FinishReason::Length,
        )
    }

    fn object_schema() -> FlexibleSchema<TypedObject> {
        let schema = Schema::<TypedObject>::new(JsonObject::from_iter([
            (
                "$schema".to_string(),
                JsonValue::String("http://json-schema.org/draft-07/schema#".to_string()),
            ),
            ("type".to_string(), JsonValue::String("object".to_string())),
            (
                "properties".to_string(),
                JsonValue::Object(JsonObject::from_iter([(
                    "content".to_string(),
                    JsonValue::Object(JsonObject::from_iter([(
                        "type".to_string(),
                        JsonValue::String("string".to_string()),
                    )])),
                )])),
            ),
            (
                "required".to_string(),
                JsonValue::Array(vec![JsonValue::String("content".to_string())]),
            ),
            ("additionalProperties".to_string(), JsonValue::Bool(false)),
        ]))
        .with_validator(|value| {
            match serde_json::from_value::<TypedObject>(value.clone()) {
                Ok(value) => ValidationResult::success(value),
                Err(error) => ValidationResult::failure(error.to_string()),
            }
        });

        schema.into()
    }

    #[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
    struct TypedObject {
        content: String,
    }

    #[test]
    fn text_output_returns_text_as_is_and_handles_missing_input() {
        let output = text();
        assert_eq!(output.name(), "text");
        assert_eq!(
            output.response_format(),
            LanguageModelResponseFormat::text()
        );
        assert_eq!(
            output.parse_complete_output(Some("some output")),
            Some("some output".to_string())
        );
        assert_eq!(output.parse_complete_output(Some("")), Some(String::new()));
        assert_eq!(output.parse_complete_output(None), None);
        assert_eq!(
            output.parse_partial_output(Some("partial text")),
            Some("partial text".to_string())
        );
        assert_eq!(output.parse_partial_output(Some("")), Some(String::new()));
    }

    #[test]
    fn object_output_returns_response_format_and_parses_complete_and_partial_output() {
        let (response, usage, finish_reason) = test_context();
        let context = OutputParseContext::new(&response, &usage, &finish_reason);
        let output = object::<TypedObject>(object_schema())
            .with_name("test-name")
            .with_description("test description");

        assert_eq!(
            output.response_format(),
            LanguageModelResponseFormat::json()
                .with_schema(object_schema().as_schema().json_schema().clone())
                .with_name("test-name")
                .with_description("test description")
        );

        let parsed = output
            .parse_complete_output(r#"{ "content": "test" }"#, context.clone())
            .expect("parsed object");
        assert_eq!(
            parsed,
            TypedObject {
                content: "test".to_string()
            }
        );

        let invalid = output
            .parse_complete_output(r#"{ broken json"#, context.clone())
            .expect_err("invalid parse should fail");
        assert_eq!(
            invalid.message(),
            "No object generated: could not parse the response."
        );
        assert_eq!(invalid.response(), &response);
        assert_eq!(invalid.usage(), &usage);
        assert_eq!(invalid.finish_reason(), &finish_reason);

        let validation_error = output
            .parse_complete_output(r#"{ "content": 123 }"#, context.clone())
            .expect_err("invalid schema should fail");
        assert_eq!(
            validation_error.message(),
            "No object generated: response did not match schema."
        );
        assert_eq!(validation_error.response(), &response);
        assert_eq!(validation_error.usage(), &usage);
        assert_eq!(validation_error.finish_reason(), &finish_reason);

        assert_eq!(
            output.parse_partial_output(Some(r#"{ "content": "partial" }"#)),
            Some(json!({ "content": "partial" }))
        );
        assert_eq!(
            output.parse_partial_output(Some(r#"{ "content": "partial", "count": 42"#)),
            Some(json!({ "content": "partial", "count": 42 }))
        );
        assert_eq!(output.parse_partial_output(Some("")), None);
    }

    #[test]
    fn array_output_returns_response_format_and_partial_elements() {
        let (response, usage, finish_reason) = test_context();
        let context = OutputParseContext::new(&response, &usage, &finish_reason);
        let element_schema = object_schema();

        let output = array::<TypedObject>(element_schema)
            .with_name("test-array-name")
            .with_description("test array description");

        let expected_schema = JsonObject::from_iter([
            (
                "$schema".to_string(),
                JsonValue::String("http://json-schema.org/draft-07/schema#".to_string()),
            ),
            ("type".to_string(), JsonValue::String("object".to_string())),
            (
                "properties".to_string(),
                JsonValue::Object(JsonObject::from_iter([(
                    "elements".to_string(),
                    JsonValue::Object(JsonObject::from_iter([
                        ("type".to_string(), JsonValue::String("array".to_string())),
                        (
                            "items".to_string(),
                            JsonValue::Object(JsonObject::from_iter([
                                ("type".to_string(), JsonValue::String("object".to_string())),
                                (
                                    "properties".to_string(),
                                    JsonValue::Object(JsonObject::from_iter([(
                                        "content".to_string(),
                                        JsonValue::Object(JsonObject::from_iter([(
                                            "type".to_string(),
                                            JsonValue::String("string".to_string()),
                                        )])),
                                    )])),
                                ),
                                (
                                    "required".to_string(),
                                    JsonValue::Array(vec![JsonValue::String(
                                        "content".to_string(),
                                    )]),
                                ),
                                ("additionalProperties".to_string(), JsonValue::Bool(false)),
                            ])),
                        ),
                    ])),
                )])),
            ),
            (
                "required".to_string(),
                JsonValue::Array(vec![JsonValue::String("elements".to_string())]),
            ),
            ("additionalProperties".to_string(), JsonValue::Bool(false)),
        ]);

        assert_eq!(
            output.response_format(),
            LanguageModelResponseFormat::json()
                .with_schema(expected_schema)
                .with_name("test-array-name")
                .with_description("test array description")
        );

        let parsed = output
            .parse_complete_output(
                r#"{ "elements": [{ "content": "test" }] }"#,
                context.clone(),
            )
            .expect("parsed array");
        assert_eq!(
            parsed,
            vec![TypedObject {
                content: "test".to_string()
            }]
        );

        let parse_error = output
            .parse_complete_output(r#"{ broken json"#, context.clone())
            .expect_err("invalid json should fail");
        assert_eq!(
            parse_error.message(),
            "No object generated: could not parse the response."
        );
        assert_eq!(parse_error.response(), &response);
        assert_eq!(parse_error.usage(), &usage);
        assert_eq!(parse_error.finish_reason(), &finish_reason);

        let validation_error = output
            .parse_complete_output(r#"{ "elements": [{ "content": 123 }] }"#, context.clone())
            .expect_err("invalid element should fail");
        assert_eq!(
            validation_error.message(),
            "No object generated: response did not match schema."
        );
        assert_eq!(validation_error.response(), &response);
        assert_eq!(validation_error.usage(), &usage);
        assert_eq!(validation_error.finish_reason(), &finish_reason);

        let repaired = output
            .parse_partial_output(Some(
                r#"{ "elements": [{ "content": "a" }, { "content": "b" }"#,
            ))
            .expect("repaired partial array");
        assert_eq!(
            repaired,
            vec![TypedObject {
                content: "a".to_string()
            }]
        );

        let successful = output
            .parse_partial_output(Some(
                r#"{ "elements": [{ "content": "a" }, { "content": "b" }] }"#,
            ))
            .expect("successful partial array");
        assert_eq!(
            successful,
            vec![
                TypedObject {
                    content: "a".to_string()
                },
                TypedObject {
                    content: "b".to_string()
                }
            ]
        );

        assert_eq!(
            output.parse_partial_output(Some(r#"{ "elements": [] }"#)),
            Some(vec![])
        );
        assert_eq!(
            output.parse_partial_output(Some(r#"{ not valid json"#)),
            None
        );
        assert_eq!(
            output.parse_partial_output(Some(r#"{ "foo": [1,2,3] }"#)),
            None
        );
        assert_eq!(
            output.parse_partial_output(Some(r#"{ "elements": "not-an-array" }"#)),
            None
        );
        assert_eq!(output.parse_partial_output(None), None);
    }

    #[test]
    fn choice_output_handles_complete_and_partial_choice_values() {
        let (response, usage, finish_reason) = test_context();
        let context = OutputParseContext::new(&response, &usage, &finish_reason);
        let output = choice(vec![
            "aaa".to_string(),
            "aab".to_string(),
            "ccc".to_string(),
        ])
        .with_name("test-choice-name")
        .with_description("test choice description");

        let expected_schema = JsonObject::from_iter([
            (
                "$schema".to_string(),
                JsonValue::String("http://json-schema.org/draft-07/schema#".to_string()),
            ),
            ("type".to_string(), JsonValue::String("object".to_string())),
            (
                "properties".to_string(),
                JsonValue::Object(JsonObject::from_iter([(
                    "result".to_string(),
                    JsonValue::Object(JsonObject::from_iter([
                        ("type".to_string(), JsonValue::String("string".to_string())),
                        (
                            "enum".to_string(),
                            JsonValue::Array(vec![
                                JsonValue::String("aaa".to_string()),
                                JsonValue::String("aab".to_string()),
                                JsonValue::String("ccc".to_string()),
                            ]),
                        ),
                    ])),
                )])),
            ),
            (
                "required".to_string(),
                JsonValue::Array(vec![JsonValue::String("result".to_string())]),
            ),
            ("additionalProperties".to_string(), JsonValue::Bool(false)),
        ]);

        assert_eq!(
            output.response_format(),
            LanguageModelResponseFormat::json()
                .with_schema(expected_schema)
                .with_name("test-choice-name")
                .with_description("test choice description")
        );

        assert_eq!(
            output
                .parse_complete_output(r#"{ "result": "aaa" }"#, context.clone())
                .expect("choice output"),
            "aaa".to_string()
        );
        assert!(
            output
                .parse_complete_output(r#"{ broken json"#, context.clone())
                .is_err()
        );
        assert!(
            output
                .parse_complete_output(r#"{}"#, context.clone())
                .is_err()
        );
        assert!(
            output
                .parse_complete_output(r#"{ "result": "d" }"#, context.clone())
                .is_err()
        );
        assert!(
            output
                .parse_complete_output(r#"{ "result": 5 }"#, context.clone())
                .is_err()
        );
        assert!(output.parse_complete_output(r#""a""#, context).is_err());

        assert_eq!(
            output.parse_partial_output(Some(r#"{ "result": "aaa" }"#)),
            Some("aaa".to_string())
        );
        assert_eq!(output.parse_partial_output(Some(r#"{ broken json"#)), None);
        assert_eq!(output.parse_partial_output(Some(r#"{}"#)), None);
        assert_eq!(
            output.parse_partial_output(Some(r#"{ "result": "d" }"#)),
            None
        );
        assert_eq!(
            output.parse_partial_output(Some(r#"{ "result": 5 }"#)),
            None
        );
        assert_eq!(output.parse_partial_output(Some(r#""a""#)), None);
        assert_eq!(
            output.parse_partial_output(Some(r#"{ "result": "aab" }"#)),
            Some("aab".to_string())
        );
        assert_eq!(
            output.parse_partial_output(Some(r#"{ "result": "c"#)),
            Some("ccc".to_string())
        );
        assert_eq!(
            output.parse_partial_output(Some(r#"{ "result": "x" }"#)),
            None
        );
        assert_eq!(output.parse_partial_output(Some(r#"{ "result": "a"#)), None);
        assert_eq!(output.parse_partial_output(Some("null")), None);
        assert_eq!(output.parse_partial_output(None), None);
    }

    #[test]
    fn json_output_handles_complete_and_partial_json() {
        let (response, usage, finish_reason) = test_context();
        let context = OutputParseContext::new(&response, &usage, &finish_reason);
        let output = json()
            .with_name("test-json-name")
            .with_description("test json description");

        assert_eq!(
            output.response_format(),
            LanguageModelResponseFormat::json()
                .with_name("test-json-name")
                .with_description("test json description")
        );

        assert_eq!(
            output
                .parse_complete_output(r#"{"a": 1, "b": [2,3]}"#, context.clone())
                .expect("parsed json"),
            json!({ "a": 1, "b": [2, 3] })
        );
        assert!(
            output
                .parse_complete_output(r#"{ a: 1 }"#, context.clone())
                .is_err()
        );
        assert!(output.parse_complete_output(r#"foo"#, context).is_err());

        assert_eq!(
            output.parse_partial_output(Some(r#"{ "foo": 1, "bar": [2, 3] }"#)),
            Some(json!({ "foo": 1, "bar": [2, 3] }))
        );
        let repaired = output.parse_partial_output(Some(r#"{ "foo": 123"#));
        if let Some(value) = repaired {
            assert_eq!(value, json!({ "foo": 123 }));
        }
        assert_eq!(output.parse_partial_output(Some("invalid!")), None);
        assert_eq!(output.parse_partial_output(Some("")), None);
        assert_eq!(output.parse_partial_output(Some("undefined")), None);
        assert_eq!(output.parse_partial_output(Some("null")), Some(json!(null)));
        assert_eq!(output.parse_partial_output(None), None);
    }

    // ---------------------------------------------------------------------
    // stream-text array/choice output streaming parity
    //
    // The upstream `streamText` cases drive `partialOutputStream`,
    // `elementStream`, `output`, and `text` entirely from the cumulative text
    // deltas fed through `Output.array(...)`/`Output.choice(...)`. These tests
    // replay the exact upstream delta sequences through the array/choice
    // partial/complete parsers (the same logic `streamText` uses) and assert
    // the resulting snapshots, deduplicating repeated partial values the way
    // the streamText output transform does.
    // ---------------------------------------------------------------------

    #[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
    struct ContentElement {
        content: String,
    }

    fn content_element_schema() -> FlexibleSchema<ContentElement> {
        let schema = Schema::<ContentElement>::new(JsonObject::from_iter([
            ("type".to_string(), JsonValue::String("object".to_string())),
            (
                "properties".to_string(),
                JsonValue::Object(JsonObject::from_iter([(
                    "content".to_string(),
                    JsonValue::Object(JsonObject::from_iter([(
                        "type".to_string(),
                        JsonValue::String("string".to_string()),
                    )])),
                )])),
            ),
            (
                "required".to_string(),
                JsonValue::Array(vec![JsonValue::String("content".to_string())]),
            ),
        ]))
        .with_validator(|value| {
            match serde_json::from_value::<ContentElement>(value.clone()) {
                Ok(value) => ValidationResult::success(value),
                Err(error) => ValidationResult::failure(error.to_string()),
            }
        });
        schema.into()
    }

    /// Accumulates `deltas` into the running text and returns the cumulative
    /// snapshots a consumer of that text would observe after each delta.
    fn cumulative_texts(deltas: &[&str]) -> Vec<String> {
        let mut text = String::new();
        let mut texts = Vec::new();
        for delta in deltas {
            text.push_str(delta);
            texts.push(text.clone());
        }
        texts
    }

    /// Replays cumulative text snapshots through an array output, emitting the
    /// deduplicated `partialOutputStream` the streamText transform produces.
    fn array_partial_output_stream(
        output: &ArrayOutput<ContentElement>,
        deltas: &[&str],
    ) -> Vec<Vec<ContentElement>> {
        let mut stream = Vec::new();
        for text in cumulative_texts(deltas) {
            if let Some(partial) = output.parse_partial_output(Some(&text)) {
                if stream.last() != Some(&partial) {
                    stream.push(partial);
                }
            }
        }
        stream
    }

    /// Replays cumulative text snapshots through a choice output, emitting the
    /// deduplicated `partialOutputStream` the streamText transform produces.
    fn choice_partial_output_stream(output: &ChoiceOutput, deltas: &[&str]) -> Vec<String> {
        let mut stream = Vec::new();
        for text in cumulative_texts(deltas) {
            if let Some(partial) = output.parse_partial_output(Some(&text)) {
                if stream.last() != Some(&partial) {
                    stream.push(partial);
                }
            }
        }
        stream
    }

    /// Three-element array streamed delta-by-delta, used by the array-output
    /// streaming cases. Mirrors the upstream `describe('array with 3 elements')`
    /// model stream.
    const ARRAY_THREE_ELEMENTS_DELTAS: &[&str] = &[
        "{\"elements\":[",
        "{",
        "\"content\":",
        "\"element 1\"",
        "},",
        "{ ",
        "\"content\": ",
        "\"element 2\"",
        "},",
        "{",
        "\"content\":",
        "\"element 3\"",
        "}",
        "]",
        "}",
    ];

    fn array_three_elements_text() -> String {
        cumulative_texts(ARRAY_THREE_ELEMENTS_DELTAS)
            .last()
            .cloned()
            .unwrap_or_default()
    }

    /// Maps packages/ai stream-text.test.ts:19978
    /// `should stream only complete objects in partialObjectStream` (array with
    /// 3 elements) — only complete elements appear in each partial snapshot; the
    /// in-progress trailing element is withheld until it parses cleanly.
    #[test]
    fn stream_text_array_output_three_elements_partial_output_stream() {
        let output = array::<ContentElement>(content_element_schema());
        let stream = array_partial_output_stream(&output, ARRAY_THREE_ELEMENTS_DELTAS);
        assert_eq!(
            stream,
            vec![
                vec![],
                vec![ContentElement {
                    content: "element 1".to_string()
                }],
                vec![
                    ContentElement {
                        content: "element 1".to_string()
                    },
                    ContentElement {
                        content: "element 2".to_string()
                    },
                ],
                vec![
                    ContentElement {
                        content: "element 1".to_string()
                    },
                    ContentElement {
                        content: "element 2".to_string()
                    },
                    ContentElement {
                        content: "element 3".to_string()
                    },
                ],
            ]
        );
    }

    /// Maps packages/ai stream-text.test.ts:20011
    /// `should stream individual elements in elementStream` (array with 3
    /// elements) — the element transform publishes each newly completed element
    /// exactly once, in order.
    #[test]
    fn stream_text_array_output_three_elements_element_stream() {
        let output = array::<ContentElement>(content_element_schema());
        let mut transform = output.create_element_stream_transform();
        let mut elements = Vec::new();
        for partial in array_partial_output_stream(&output, ARRAY_THREE_ELEMENTS_DELTAS) {
            elements.extend(transform.transform(&partial));
        }
        assert_eq!(
            elements,
            vec![
                ContentElement {
                    content: "element 1".to_string()
                },
                ContentElement {
                    content: "element 2".to_string()
                },
                ContentElement {
                    content: "element 3".to_string()
                },
            ]
        );
    }

    /// Maps packages/ai stream-text.test.ts:20028
    /// `should resolve output promise with the correct content` (array with 3
    /// elements) — the completed text parses to all three validated elements.
    #[test]
    fn stream_text_array_output_three_elements_resolves_output() {
        let (response, usage, finish_reason) = test_context();
        let context = OutputParseContext::new(&response, &usage, &finish_reason);
        let output = array::<ContentElement>(content_element_schema());
        let parsed = output
            .parse_complete_output(&array_three_elements_text(), context)
            .expect("array output resolves");
        assert_eq!(
            parsed,
            vec![
                ContentElement {
                    content: "element 1".to_string()
                },
                ContentElement {
                    content: "element 2".to_string()
                },
                ContentElement {
                    content: "element 3".to_string()
                },
            ]
        );
    }

    /// Maps packages/ai stream-text.test.ts:20036
    /// `should resolve text promise with the correct text` (array with 3
    /// elements) — the raw text promise is the concatenated, unmodified deltas.
    #[test]
    fn stream_text_array_output_three_elements_resolves_text() {
        assert_eq!(
            array_three_elements_text(),
            "{\"elements\":[{\"content\":\"element 1\"},{ \"content\": \"element 2\"},{\"content\":\"element 3\"}]}"
        );
    }

    /// Two-element array delivered in a single text delta, used by the
    /// "array with 2 elements streamed in 1 chunk" cases.
    const ARRAY_TWO_ELEMENTS_SINGLE_CHUNK: &[&str] =
        &[r#"{"elements":[{"content":"element 1"},{"content":"element 2"}]}"#];

    /// Maps packages/ai stream-text.test.ts:20079
    /// `should stream only complete objects in partialObjectStream` (array with
    /// 2 elements in 1 chunk) — a single complete chunk yields a single partial
    /// snapshot containing both elements.
    #[test]
    fn stream_text_array_output_single_chunk_partial_output_stream() {
        let output = array::<ContentElement>(content_element_schema());
        let stream = array_partial_output_stream(&output, ARRAY_TWO_ELEMENTS_SINGLE_CHUNK);
        assert_eq!(
            stream,
            vec![vec![
                ContentElement {
                    content: "element 1".to_string()
                },
                ContentElement {
                    content: "element 2".to_string()
                },
            ]]
        );
    }

    /// Maps packages/ai stream-text.test.ts:20095
    /// `should stream individual elements in elementStream` (array with 2
    /// elements in 1 chunk) — both elements are published from the single
    /// partial snapshot.
    #[test]
    fn stream_text_array_output_single_chunk_element_stream() {
        let output = array::<ContentElement>(content_element_schema());
        let mut transform = output.create_element_stream_transform();
        let mut elements = Vec::new();
        for partial in array_partial_output_stream(&output, ARRAY_TWO_ELEMENTS_SINGLE_CHUNK) {
            elements.extend(transform.transform(&partial));
        }
        assert_eq!(
            elements,
            vec![
                ContentElement {
                    content: "element 1".to_string()
                },
                ContentElement {
                    content: "element 2".to_string()
                },
            ]
        );
    }

    /// Maps packages/ai stream-text.test.ts:20109
    /// `should resolve output promise with the correct content` (array with 2
    /// elements in 1 chunk) — both elements resolve from the completed text.
    #[test]
    fn stream_text_array_output_single_chunk_resolves_output() {
        let (response, usage, finish_reason) = test_context();
        let context = OutputParseContext::new(&response, &usage, &finish_reason);
        let output = array::<ContentElement>(content_element_schema());
        let parsed = output
            .parse_complete_output(ARRAY_TWO_ELEMENTS_SINGLE_CHUNK[0], context)
            .expect("array output resolves");
        assert_eq!(
            parsed,
            vec![
                ContentElement {
                    content: "element 1".to_string()
                },
                ContentElement {
                    content: "element 2".to_string()
                },
            ]
        );
    }

    /// Maps packages/ai stream-text.test.ts:20116
    /// `should resolve text promise with the correct text` (array with 2
    /// elements in 1 chunk) — the text promise is the single unmodified chunk.
    #[test]
    fn stream_text_array_output_single_chunk_resolves_text() {
        assert_eq!(
            ARRAY_TWO_ELEMENTS_SINGLE_CHUNK[0],
            r#"{"elements":[{"content":"element 1"},{"content":"element 2"}]}"#
        );
    }

    /// Deltas for `{ "result": "sunny" }`, used by the choice-output cases.
    const CHOICE_SUNNY_DELTAS: &[&str] = &["{ ", "\"result\": ", "\"su", "nny", "\"", " }"];

    fn weather_choice() -> ChoiceOutput {
        choice(vec![
            "sunny".to_string(),
            "rainy".to_string(),
            "snowy".to_string(),
        ])
    }

    /// Maps packages/ai stream-text.test.ts:20125
    /// `should stream an choice value` — the choice partial stream emits the
    /// single matched option once the repaired result uniquely matches.
    #[test]
    fn stream_text_choice_output_streams_choice_value() {
        let output = weather_choice();
        assert_eq!(
            choice_partial_output_stream(&output, CHOICE_SUNNY_DELTAS),
            vec!["sunny".to_string()]
        );
    }

    /// Maps packages/ai stream-text.test.ts:20160
    /// `should resolve text promise with the correct text` (choice output) — the
    /// text promise is the raw concatenated JSON object text.
    #[test]
    fn stream_text_choice_output_resolves_text() {
        assert_eq!(
            cumulative_texts(CHOICE_SUNNY_DELTAS)
                .last()
                .cloned()
                .unwrap(),
            "{ \"result\": \"sunny\" }"
        );
    }

    /// Maps packages/ai stream-text.test.ts:20190
    /// `should resolve output promise with the correct content` (choice output)
    /// — the completed text resolves to the matched option value.
    #[test]
    fn stream_text_choice_output_resolves_output() {
        let (response, usage, finish_reason) = test_context();
        let context = OutputParseContext::new(&response, &usage, &finish_reason);
        let output = weather_choice();
        let text = cumulative_texts(CHOICE_SUNNY_DELTAS)
            .last()
            .cloned()
            .unwrap();
        assert_eq!(
            output
                .parse_complete_output(&text, context)
                .expect("choice resolves"),
            "sunny".to_string()
        );
    }

    /// Maps packages/ai stream-text.test.ts:20220
    /// `should not stream incorrect values` — a result that matches no option is
    /// never emitted on the choice partial stream.
    #[test]
    fn stream_text_choice_output_does_not_stream_incorrect_values() {
        let output = weather_choice();
        let deltas: &[&str] = &["{ ", "\"result\": ", "\"foo", "bar", "\"", " }"];
        assert!(choice_partial_output_stream(&output, deltas).is_empty());
    }

    /// Maps packages/ai stream-text.test.ts:20254
    /// `should handle ambiguous values` — while several options share a prefix
    /// the partial value is withheld, then the exact match is emitted once the
    /// result resolves uniquely.
    #[test]
    fn stream_text_choice_output_handles_ambiguous_values() {
        let output = choice(vec!["foobar".to_string(), "foobar2".to_string()]);
        // No text-end: the final state is a repaired (unterminated) object.
        let deltas: &[&str] = &["{ ", "\"result\": ", "\"foo", "bar", "\"", " }"];
        assert_eq!(
            choice_partial_output_stream(&output, deltas),
            vec!["foobar".to_string()]
        );
    }

    /// Maps packages/ai stream-text.test.ts:20288
    /// `should handle non-ambiguous values` — a result that is a prefix of only
    /// one option resolves to that option on the partial stream.
    #[test]
    fn stream_text_choice_output_handles_non_ambiguous_values() {
        let output = choice(vec!["foobar".to_string(), "barfoo".to_string()]);
        let deltas: &[&str] = &["{ ", "\"result\": ", "\"foo", "bar", "\"", " }"];
        assert_eq!(
            choice_partial_output_stream(&output, deltas),
            vec!["foobar".to_string()]
        );
    }
}
