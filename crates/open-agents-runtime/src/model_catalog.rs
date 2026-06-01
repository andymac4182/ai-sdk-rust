//! Open Agents model catalog, variant, access, and route helpers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Upstream web app fallback model for new sessions.
pub const DEFAULT_MODEL_ID: &str = "anthropic/claude-haiku-4.5";

/// Repository default model used when a selected model is unavailable.
pub const APP_DEFAULT_MODEL_ID: &str = "openai/gpt-5.4";

/// Prefix for user-created and built-in Open Agents model variants.
pub const MODEL_VARIANT_ID_PREFIX: &str = "variant:";

/// Prefix for built-in Open Agents model variants.
pub const BUILT_IN_VARIANT_ID_PREFIX: &str = "variant:builtin:";

const TOKENS_PER_MILLION: f64 = 1_000_000.0;
const RESTRICTED_MODEL_PREFIXES: &[&str] = &["anthropic/claude-opus-"];

/// Gateway model row returned by the Open Agents model route.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayAvailableModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "modelType", default, skip_serializing_if = "Option::is_none")]
    pub model_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
}

/// Per-token cost tier from models.dev.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AvailableModelCostTier {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
}

/// Model pricing metadata from models.dev.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AvailableModelCost {
    #[serde(flatten)]
    pub base: AvailableModelCostTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_over_200k: Option<AvailableModelCostTier>,
}

/// Available language model with optional context and pricing metadata.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "modelType", default, skip_serializing_if = "Option::is_none")]
    pub model_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<AvailableModelCost>,
}

impl From<GatewayAvailableModel> for AvailableModel {
    fn from(model: GatewayAvailableModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            description: model.description,
            model_type: model.model_type,
            context_window: model.context_window,
            cost: None,
        }
    }
}

/// Usage values used for Open Agents cost estimation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

/// User or built-in model variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVariant {
    pub id: String,
    pub name: String,
    pub base_model_id: String,
    #[serde(default)]
    pub provider_options: BTreeMap<String, Value>,
}

impl ModelVariant {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        base_model_id: impl Into<String>,
        provider_options: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            base_model_id: base_model_id.into(),
            provider_options,
        }
    }
}

/// Provider-keyed model options passed to the AI SDK provider layer.
pub type ProviderOptionsByProvider = BTreeMap<String, BTreeMap<String, Value>>;

/// Result of resolving a selected model id against the variant list.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedModelSelection {
    pub resolved_model_id: String,
    pub provider_options_by_provider: Option<ProviderOptionsByProvider>,
    pub is_missing_variant: bool,
}

/// UI option row used by model pickers and route tests.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
    pub short_label: String,
    pub description: Option<String>,
    pub is_variant: bool,
    pub context_window: Option<u64>,
    pub cost: Option<AvailableModelCost>,
    pub provider: String,
}

/// Provider group used by model picker rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelGroup {
    pub provider: String,
    pub label: String,
    pub options: Vec<ModelOption>,
}

/// Minimal session data needed by model-access rules.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelAccessSession {
    pub auth_provider: Option<String>,
    pub email: Option<String>,
}

/// User preferences shape sanitized by model-access rules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserModelPreferences {
    pub default_model_id: Option<String>,
    pub default_subagent_model_id: Option<String>,
    pub model_variants: Vec<ModelVariant>,
    pub enabled_model_ids: Vec<String>,
}

/// Error JSON returned by a failed Gateway model request.
#[derive(Clone, Debug, PartialEq)]
pub struct GatewayModelsError {
    pub value: Value,
}

impl GatewayModelsError {
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

/// Returns the display name for an available model.
pub fn get_model_display_name(model: &AvailableModel) -> &str {
    model.name.as_deref().unwrap_or(&model.id)
}

/// Estimates model cost using the upstream 200k-token pricing threshold.
pub fn estimate_model_usage_cost(
    usage: ModelUsage,
    cost: Option<&AvailableModelCost>,
) -> Option<f64> {
    let cost = cost?;
    let cost_tier = resolve_cost_tier(usage, cost);
    let input_price = cost_tier.input?;
    let output_price = cost_tier.output?;
    let cached_input_tokens = usage.cached_input_tokens.min(usage.input_tokens);
    let uncached_input_tokens = usage.input_tokens.saturating_sub(cached_input_tokens);
    let cache_read_price = cost_tier.cache_read.unwrap_or(input_price);

    Some(
        (uncached_input_tokens as f64 * input_price) / TOKENS_PER_MILLION
            + (cached_input_tokens as f64 * cache_read_price) / TOKENS_PER_MILLION
            + (usage.output_tokens as f64 * output_price) / TOKENS_PER_MILLION,
    )
}

fn resolve_cost_tier(usage: ModelUsage, cost: &AvailableModelCost) -> AvailableModelCostTier {
    if usage.input_tokens > 200_000
        && cost.context_over_200k.as_ref().is_some_and(|tier| {
            tier.input.is_some() || tier.output.is_some() || tier.cache_read.is_some()
        })
    {
        let over = cost.context_over_200k.as_ref().expect("checked");
        return AvailableModelCostTier {
            input: over.input.or(cost.base.input),
            output: over.output.or(cost.base.output),
            cache_read: over.cache_read.or(cost.base.cache_read),
        };
    }

    cost.base.clone()
}

/// Returns true for OpenAI GPT pro models disabled by Open Agents.
pub fn is_model_disabled(model_id: &str) -> bool {
    model_id.starts_with("openai/gpt-") && model_id.ends_with("-pro")
}

/// Filters disabled models from an available model list.
pub fn filter_disabled_models<T: Clone>(models: &[T], id: impl Fn(&T) -> &str) -> Vec<T> {
    models
        .iter()
        .filter(|model| !is_model_disabled(id(model)))
        .cloned()
        .collect()
}

/// Resolves disabled selections to the app default.
pub fn resolve_available_model_id(model_id: &str) -> String {
    if is_model_disabled(model_id) {
        APP_DEFAULT_MODEL_ID.to_string()
    } else {
        model_id.to_string()
    }
}

/// Converts flat variant options into provider-keyed options.
pub fn to_provider_options_by_provider(
    base_model_id: &str,
    provider_options: BTreeMap<String, Value>,
) -> Option<ProviderOptionsByProvider> {
    let provider = provider_from_model_id(base_model_id);
    if provider.is_empty() {
        return None;
    }

    let mut provider_options = provider_options;
    if provider == "openai" {
        provider_options.insert("store".to_string(), Value::Bool(false));
    }

    if provider_options.is_empty() {
        return None;
    }

    Some(BTreeMap::from([(provider, provider_options)]))
}

/// Returns true for built-in variant ids.
pub fn is_built_in_variant(variant_id: &str) -> bool {
    variant_id.starts_with(BUILT_IN_VARIANT_ID_PREFIX)
}

/// Built-in Open Agents variants.
pub fn built_in_variants() -> Vec<ModelVariant> {
    vec![
        ModelVariant::new(
            "variant:builtin:gpt-5.4-xhigh",
            "GPT-5.4 (XHigh)",
            "openai/gpt-5.4",
            BTreeMap::from([
                ("reasoningEffort".to_string(), json!("xhigh")),
                ("reasoningSummary".to_string(), json!("auto")),
            ]),
        ),
        ModelVariant::new(
            "variant:builtin:claude-opus-4.6-high",
            "Claude Opus 4.6 (High)",
            "anthropic/claude-opus-4.6",
            BTreeMap::from([("effort".to_string(), json!("high"))]),
        ),
    ]
}

/// Combines built-in variants with user variants.
pub fn get_all_variants(user_variants: &[ModelVariant]) -> Vec<ModelVariant> {
    built_in_variants()
        .into_iter()
        .chain(user_variants.iter().cloned())
        .collect()
}

/// Resolves a selected model id to a base model plus provider options.
pub fn resolve_model_selection(
    selected_model_id: &str,
    variants: &[ModelVariant],
) -> ResolvedModelSelection {
    if !selected_model_id.starts_with(MODEL_VARIANT_ID_PREFIX) {
        return ResolvedModelSelection {
            resolved_model_id: selected_model_id.to_string(),
            provider_options_by_provider: None,
            is_missing_variant: false,
        };
    }

    let Some(variant) = variants
        .iter()
        .find(|variant| variant.id == selected_model_id)
    else {
        return ResolvedModelSelection {
            resolved_model_id: selected_model_id.to_string(),
            provider_options_by_provider: None,
            is_missing_variant: true,
        };
    };

    ResolvedModelSelection {
        resolved_model_id: variant.base_model_id.clone(),
        provider_options_by_provider: to_provider_options_by_provider(
            &variant.base_model_id,
            variant.provider_options.clone(),
        ),
        is_missing_variant: false,
    }
}

/// Builds base and variant model option rows.
pub fn build_model_options(
    models: &[AvailableModel],
    variants: &[ModelVariant],
) -> Vec<ModelOption> {
    let base_models_by_id = models
        .iter()
        .map(|model| (model.id.clone(), model))
        .collect::<BTreeMap<_, _>>();
    let base_model_options = models.iter().map(to_base_model_option);
    let variant_options = variants.iter().map(|variant| {
        to_variant_option(
            variant,
            base_models_by_id.get(&variant.base_model_id).copied(),
        )
    });

    base_model_options.chain(variant_options).collect()
}

/// Groups model options by provider with Anthropic and OpenAI pinned first.
pub fn group_by_provider(options: &[ModelOption]) -> Vec<ModelGroup> {
    let mut groups: BTreeMap<String, Vec<ModelOption>> = BTreeMap::new();
    let mut providers = Vec::new();

    for option in options {
        if !groups.contains_key(&option.provider) {
            providers.push(option.provider.clone());
        }
        groups
            .entry(option.provider.clone())
            .or_default()
            .push(option.clone());
    }

    providers.sort_by_key(|provider| provider_sort_key(provider));
    providers
        .into_iter()
        .map(|provider| ModelGroup {
            label: provider.clone(),
            options: groups.remove(&provider).unwrap_or_default(),
            provider,
        })
        .collect()
}

/// Appends a missing variant option when a saved variant no longer exists.
pub fn with_missing_model_option(
    model_options: &[ModelOption],
    model_id: Option<&str>,
) -> Vec<ModelOption> {
    let Some(model_id) = model_id.filter(|id| !id.is_empty()) else {
        return model_options.to_vec();
    };
    if model_options.iter().any(|option| option.id == model_id)
        || !model_id.starts_with(MODEL_VARIANT_ID_PREFIX)
    {
        return model_options.to_vec();
    }

    let label = format!(
        "{} (missing)",
        model_id.trim_start_matches(MODEL_VARIANT_ID_PREFIX)
    );
    let mut next = model_options.to_vec();
    next.push(ModelOption {
        id: model_id.to_string(),
        label: label.clone(),
        short_label: label,
        description: Some("Variant no longer exists".to_string()),
        is_variant: true,
        context_window: None,
        cost: None,
        provider: "unknown".to_string(),
    });
    next
}

/// Returns the default option id for a model picker.
pub fn get_default_model_option_id(model_options: &[ModelOption]) -> String {
    if model_options
        .iter()
        .any(|option| option.id == APP_DEFAULT_MODEL_ID)
    {
        APP_DEFAULT_MODEL_ID.to_string()
    } else {
        model_options
            .first()
            .map(|option| option.id.clone())
            .unwrap_or_else(|| APP_DEFAULT_MODEL_ID.to_string())
    }
}

/// Returns true when a model is unavailable to the current managed-trial session.
pub fn is_restricted_model_id_for_session(
    model_id: &str,
    session: Option<&ModelAccessSession>,
    request_url: &str,
) -> bool {
    is_managed_template_trial_user(session, request_url)
        && RESTRICTED_MODEL_PREFIXES
            .iter()
            .any(|prefix| model_id.starts_with(prefix))
}

/// Filters restricted base models for the current session.
pub fn filter_models_for_session<T: Clone>(
    models: &[T],
    session: Option<&ModelAccessSession>,
    request_url: &str,
    id: impl Fn(&T) -> &str,
) -> Vec<T> {
    if !is_managed_template_trial_user(session, request_url) {
        return models.to_vec();
    }
    models
        .iter()
        .filter(|model| {
            !RESTRICTED_MODEL_PREFIXES
                .iter()
                .any(|prefix| id(model).starts_with(prefix))
        })
        .cloned()
        .collect()
}

/// Filters restricted model variants for the current session.
pub fn filter_model_variants_for_session(
    variants: &[ModelVariant],
    session: Option<&ModelAccessSession>,
    request_url: &str,
) -> Vec<ModelVariant> {
    if !is_managed_template_trial_user(session, request_url) {
        return variants.to_vec();
    }
    variants
        .iter()
        .filter(|variant| {
            !RESTRICTED_MODEL_PREFIXES
                .iter()
                .any(|prefix| variant.base_model_id.starts_with(prefix))
        })
        .cloned()
        .collect()
}

/// Sanitizes a selected model id for the current session.
pub fn sanitize_selected_model_id_for_session(
    model_id: Option<&str>,
    model_variants: &[ModelVariant],
    session: Option<&ModelAccessSession>,
    request_url: &str,
) -> Option<String> {
    let model_id = model_id?;
    if !is_managed_template_trial_user(session, request_url) {
        return Some(model_id.to_string());
    }
    if RESTRICTED_MODEL_PREFIXES
        .iter()
        .any(|prefix| model_id.starts_with(prefix))
    {
        return Some(APP_DEFAULT_MODEL_ID.to_string());
    }
    if model_id.starts_with(MODEL_VARIANT_ID_PREFIX)
        && !filter_model_variants_for_session(model_variants, session, request_url)
            .iter()
            .any(|variant| variant.id == model_id)
    {
        return Some(APP_DEFAULT_MODEL_ID.to_string());
    }
    Some(model_id.to_string())
}

/// Sanitizes model preferences for managed-trial access.
pub fn sanitize_user_preferences_for_session(
    preferences: &UserModelPreferences,
    session: Option<&ModelAccessSession>,
    request_url: &str,
) -> UserModelPreferences {
    if !is_managed_template_trial_user(session, request_url) {
        return preferences.clone();
    }

    let filtered_model_variants =
        filter_model_variants_for_session(&preferences.model_variants, session, request_url);
    let available_model_variants = filter_model_variants_for_session(
        &get_all_variants(&filtered_model_variants),
        session,
        request_url,
    );

    UserModelPreferences {
        default_model_id: Some(
            sanitize_selected_model_id_for_session(
                preferences.default_model_id.as_deref(),
                &available_model_variants,
                session,
                request_url,
            )
            .unwrap_or_else(|| APP_DEFAULT_MODEL_ID.to_string()),
        ),
        default_subagent_model_id: Some(
            sanitize_selected_model_id_for_session(
                preferences.default_subagent_model_id.as_deref(),
                &available_model_variants,
                session,
                request_url,
            )
            .unwrap_or_else(|| APP_DEFAULT_MODEL_ID.to_string()),
        ),
        model_variants: filtered_model_variants,
        enabled_model_ids: preferences
            .enabled_model_ids
            .iter()
            .filter(|model_id| !is_restricted_model_id_for_session(model_id, session, request_url))
            .cloned()
            .collect(),
    }
}

/// Builds `/api/models` output from deterministic Gateway/models.dev inputs.
pub fn available_models_route_models(
    gateway_result: Result<Vec<GatewayAvailableModel>, GatewayModelsError>,
    models_dev_data: &Value,
    session: Option<&ModelAccessSession>,
    request_url: &str,
) -> Result<Vec<AvailableModel>, String> {
    let gateway_models = match gateway_result {
        Ok(models) => models,
        Err(error) => get_models_from_gateway_error(&error.value)
            .ok_or_else(|| "Failed to fetch available models".to_string())?,
    };
    let metadata = models_dev_metadata_map(models_dev_data);
    let language_models = gateway_models
        .into_iter()
        .filter(|model| model.model_type.as_deref() == Some("language"))
        .filter(|model| !is_model_disabled(&model.id))
        .map(AvailableModel::from)
        .map(|model| add_models_dev_metadata(model, &metadata))
        .collect::<Vec<_>>();

    Ok(filter_models_for_session(
        &language_models,
        session,
        request_url,
        |model| &model.id,
    ))
}

/// Recovers valid model rows from Gateway validation-error payloads.
pub fn get_models_from_gateway_error(error: &Value) -> Option<Vec<GatewayAvailableModel>> {
    let models = error
        .pointer("/response/models")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(parse_gateway_error_model)
        .collect::<Vec<_>>();

    if models.is_empty() {
        None
    } else {
        Some(models)
    }
}

fn parse_gateway_error_model(value: &Value) -> Option<GatewayAvailableModel> {
    Some(GatewayAvailableModel {
        id: value.get("id")?.as_str()?.to_string(),
        name: Some(value.get("name")?.as_str()?.to_string()),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        model_type: value
            .get("modelType")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        context_window: value.get("context_window").and_then(Value::as_u64),
    })
}

fn to_base_model_option(model: &AvailableModel) -> ModelOption {
    let label = get_model_display_name(model).to_string();
    let provider = provider_from_model_id(&model.id);
    ModelOption {
        id: model.id.clone(),
        short_label: strip_provider_prefix(&label, &provider),
        label,
        description: model.description.clone(),
        is_variant: false,
        context_window: model.context_window,
        cost: model.cost.clone(),
        provider,
    }
}

fn to_variant_option(variant: &ModelVariant, base_model: Option<&AvailableModel>) -> ModelOption {
    let base_label = base_model
        .map(get_model_display_name)
        .unwrap_or(&variant.base_model_id);
    let provider = provider_from_model_id(&variant.base_model_id);
    ModelOption {
        id: variant.id.clone(),
        label: variant.name.clone(),
        short_label: strip_provider_prefix(&variant.name, &provider),
        description: Some(format!("Variant of {base_label}")),
        is_variant: true,
        context_window: base_model.and_then(|model| model.context_window),
        cost: base_model.and_then(|model| model.cost.clone()),
        provider,
    }
}

fn provider_sort_key(provider: &str) -> (u8, String) {
    match provider {
        "anthropic" => (0, String::new()),
        "openai" => (1, String::new()),
        other => (2, other.to_string()),
    }
}

fn provider_from_model_id(model_id: &str) -> String {
    model_id
        .split_once('/')
        .map(|(provider, _)| provider)
        .unwrap_or(model_id)
        .to_string()
}

fn strip_provider_prefix(label: &str, provider: &str) -> String {
    let prefixes: &[&str] = match provider {
        "anthropic" => &["Claude"],
        "google" => &["Gemini"],
        "xai" => &["Grok"],
        "mistral" => &["Mistral"],
        "deepseek" => &["DeepSeek"],
        "meta" => &["Meta"],
        _ => &[],
    };

    for prefix in prefixes {
        if let Some(stripped) = label.strip_prefix(&format!("{prefix} ")) {
            return stripped.to_string();
        }
    }

    label.to_string()
}

fn is_managed_template_trial_user(session: Option<&ModelAccessSession>, request_url: &str) -> bool {
    let Some(session) = session else {
        return false;
    };
    session.auth_provider.as_deref() == Some("vercel")
        && is_managed_template_deployment(request_url)
        && !has_allowed_managed_template_email(session.email.as_deref())
}

fn is_managed_template_deployment(request_url: &str) -> bool {
    matches!(
        normalize_host(request_url).as_deref(),
        Some("open-agents.dev" | "www.open-agents.dev")
    )
}

fn has_allowed_managed_template_email(email: Option<&str>) -> bool {
    email
        .map(str::trim)
        .and_then(|email| email.rsplit_once('@').map(|(_, domain)| domain.to_string()))
        .is_some_and(|domain| domain.eq_ignore_ascii_case("vercel.com"))
}

fn normalize_host(value: &str) -> Option<String> {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(&trimmed);
    let host_port = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme);
    if host_port.starts_with('[') {
        return host_port.split(']').next().map(|host| format!("{host}]"));
    }
    Some(host_port.split(':').next().unwrap_or(host_port).to_string())
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ModelsDevMetadata {
    context_window: Option<u64>,
    cost: Option<AvailableModelCost>,
}

fn models_dev_metadata_map(data: &Value) -> BTreeMap<String, ModelsDevMetadata> {
    let mut metadata = BTreeMap::new();
    let Some(providers) = data.as_object() else {
        return metadata;
    };

    for (provider_key, provider_value) in providers {
        let Some(models) = provider_value.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (model_key, model_value) in models {
            let Some(model) = model_value.as_object() else {
                continue;
            };
            let raw_id = model.get("id").and_then(Value::as_str).unwrap_or(model_key);
            let model_id = if raw_id.contains('/') {
                raw_id.to_string()
            } else {
                format!("{provider_key}/{raw_id}")
            };
            let context_window = model
                .get("limit")
                .and_then(|limit| limit.get("context"))
                .and_then(Value::as_u64)
                .filter(|value| *value > 0);
            let cost = model.get("cost").and_then(models_dev_cost);

            if context_window.is_some() || cost.is_some() {
                metadata.insert(
                    model_id,
                    ModelsDevMetadata {
                        context_window,
                        cost,
                    },
                );
            }
        }
    }

    metadata
}

fn models_dev_cost(value: &Value) -> Option<AvailableModelCost> {
    let base = models_dev_cost_tier(value);
    let context_over_200k = value
        .get("context_over_200k")
        .and_then(models_dev_cost_tier);
    if base.is_none() && context_over_200k.is_none() {
        return None;
    }
    Some(AvailableModelCost {
        base: base.unwrap_or_default(),
        context_over_200k,
    })
}

fn models_dev_cost_tier(value: &Value) -> Option<AvailableModelCostTier> {
    let input = value.get("input").and_then(Value::as_f64);
    let output = value.get("output").and_then(Value::as_f64);
    let cache_read = value.get("cache_read").and_then(Value::as_f64);
    if input.is_none() && output.is_none() && cache_read.is_none() {
        return None;
    }
    Some(AvailableModelCostTier {
        input,
        output,
        cache_read,
    })
}

fn add_models_dev_metadata(
    mut model: AvailableModel,
    metadata_map: &BTreeMap<String, ModelsDevMetadata>,
) -> AvailableModel {
    let Some(metadata) = metadata_map.get(&model.id) else {
        return model;
    };
    if let Some(context_window) = metadata.context_window.filter(|value| *value > 0) {
        model.context_window = Some(context_window);
    }
    if let Some(cost) = &metadata.cost {
        model.cost = Some(cost.clone());
    }
    model
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> BTreeMap<String, Value> {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect()
    }

    fn available_model(id: &str, name: Option<&str>) -> AvailableModel {
        AvailableModel {
            id: id.to_string(),
            name: name.map(ToString::to_string),
            model_type: Some("language".to_string()),
            ..AvailableModel::default()
        }
    }

    fn managed_trial_session() -> ModelAccessSession {
        ModelAccessSession {
            auth_provider: Some("vercel".to_string()),
            email: Some("alice@example.com".to_string()),
        }
    }

    fn vercel_session() -> ModelAccessSession {
        ModelAccessSession {
            auth_provider: Some("vercel".to_string()),
            email: Some("dev@vercel.com".to_string()),
        }
    }

    fn opus_variant() -> ModelVariant {
        ModelVariant::new(
            "variant:user-opus",
            "User Opus",
            "anthropic/claude-opus-4.6",
            object([("effort", json!("high"))]),
        )
    }

    #[test]
    fn model_usage_cost_uses_base_and_context_over_200k_pricing() {
        let cost = AvailableModelCost {
            base: AvailableModelCostTier {
                input: Some(2.5),
                output: Some(15.0),
                cache_read: Some(0.25),
            },
            context_over_200k: Some(AvailableModelCostTier {
                input: Some(5.0),
                output: Some(22.5),
                cache_read: Some(0.5),
            }),
        };

        let base = estimate_model_usage_cost(
            ModelUsage {
                input_tokens: 100_000,
                cached_input_tokens: 80_000,
                output_tokens: 1_000,
            },
            Some(&cost),
        )
        .expect("base cost");
        assert!((base - 0.085).abs() < 0.000_001);

        let over_200k = estimate_model_usage_cost(
            ModelUsage {
                input_tokens: 250_000,
                cached_input_tokens: 200_000,
                output_tokens: 1_000,
            },
            Some(&cost),
        )
        .expect("context-over cost");
        assert!((over_200k - 0.3725).abs() < 0.000_001);
    }

    #[test]
    fn model_availability_disables_openai_gpt_pro_models() {
        assert!(is_model_disabled("openai/gpt-5.4-pro"));
        assert!(is_model_disabled("openai/gpt-5.5-pro"));
        assert!(!is_model_disabled("openai/gpt-5.5-pro-preview"));
        assert!(!is_model_disabled("openai/gpt-5.5"));
        assert!(!is_model_disabled("openai/o1-pro"));

        let models = vec![
            available_model("openai/gpt-5.5", Some("GPT 5.5")),
            available_model("openai/gpt-5.5-pro", Some("GPT 5.5 Pro")),
            available_model("anthropic/claude-opus-4.6", Some("Claude Opus 4.6")),
        ];
        let filtered = filter_disabled_models(&models, |model| &model.id);
        assert_eq!(
            filtered
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["openai/gpt-5.5", "anthropic/claude-opus-4.6"]
        );
        assert_eq!(
            resolve_available_model_id("openai/gpt-5.5-pro"),
            APP_DEFAULT_MODEL_ID
        );
        assert_eq!(
            resolve_available_model_id("openai/gpt-5.5"),
            "openai/gpt-5.5"
        );
    }

    #[test]
    fn model_variants_provider_options_builtins_and_resolution() {
        assert_eq!(
            to_provider_options_by_provider(
                "openai/gpt-5",
                object([
                    ("reasoningEffort", json!("medium")),
                    ("reasoningSummary", json!("detailed")),
                ]),
            ),
            Some(BTreeMap::from([(
                "openai".to_string(),
                object([
                    ("reasoningEffort", json!("medium")),
                    ("reasoningSummary", json!("detailed")),
                    ("store", json!(false)),
                ])
            )]))
        );
        assert_eq!(
            to_provider_options_by_provider("openai/gpt-5", BTreeMap::new())
                .expect("openai defaults")["openai"]["store"],
            json!(false)
        );
        assert_eq!(
            to_provider_options_by_provider("anthropic/claude-opus-4.6", BTreeMap::new()),
            None
        );
        assert_eq!(
            to_provider_options_by_provider(
                "openai/gpt-5",
                object([("reasoningEffort", json!("medium")), ("store", json!(true)),]),
            )
            .expect("openai options")["openai"]["store"],
            json!(false)
        );
        assert!(is_built_in_variant("variant:builtin:gpt-5.4-xhigh"));
        assert!(!is_built_in_variant("variant:openai-medium"));

        let builtins = built_in_variants();
        assert_eq!(builtins.len(), 2);
        assert!(
            builtins
                .iter()
                .all(|variant| is_built_in_variant(&variant.id))
        );

        let user_variant = ModelVariant::new(
            "variant:openai-medium",
            "OpenAI Medium Reasoning",
            "openai/gpt-5",
            object([("reasoningEffort", json!("medium"))]),
        );
        let all = get_all_variants(std::slice::from_ref(&user_variant));
        assert_eq!(all[0], builtins[0]);
        assert_eq!(all[1], builtins[1]);
        assert_eq!(all[2], user_variant);

        assert_eq!(
            resolve_model_selection("openai/gpt-5", &[]),
            ResolvedModelSelection {
                resolved_model_id: "openai/gpt-5".to_string(),
                provider_options_by_provider: None,
                is_missing_variant: false,
            }
        );
        assert_eq!(
            resolve_model_selection("variant:openai-medium", &[all[2].clone()]),
            ResolvedModelSelection {
                resolved_model_id: "openai/gpt-5".to_string(),
                provider_options_by_provider: Some(BTreeMap::from([(
                    "openai".to_string(),
                    object([
                        ("reasoningEffort", json!("medium")),
                        ("store", json!(false)),
                    ])
                )])),
                is_missing_variant: false,
            }
        );
        assert_eq!(
            resolve_model_selection("variant:builtin:gpt-5.4-xhigh", &builtins),
            ResolvedModelSelection {
                resolved_model_id: "openai/gpt-5.4".to_string(),
                provider_options_by_provider: Some(BTreeMap::from([(
                    "openai".to_string(),
                    object([
                        ("reasoningEffort", json!("xhigh")),
                        ("reasoningSummary", json!("auto")),
                        ("store", json!(false)),
                    ])
                )])),
                is_missing_variant: false,
            }
        );
        assert!(resolve_model_selection("variant:missing", &[]).is_missing_variant);
    }

    #[test]
    fn model_options_build_group_missing_and_default_selection() {
        let models = vec![AvailableModel {
            id: "openai/gpt-5".to_string(),
            name: Some("GPT-5".to_string()),
            description: Some("Base model".to_string()),
            model_type: Some("language".to_string()),
            context_window: Some(400_000),
            cost: None,
        }];
        let variants = vec![ModelVariant::new(
            "variant:gpt-5-medium",
            "GPT-5 Medium Reasoning",
            "openai/gpt-5",
            object([("reasoningEffort", json!("medium"))]),
        )];
        let options = build_model_options(&models, &variants);
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].label, "GPT-5");
        assert_eq!(options[1].description.as_deref(), Some("Variant of GPT-5"));

        let anthropic = build_model_options(
            &[available_model(
                "anthropic/claude-opus-4.6",
                Some("Claude Opus 4.6"),
            )],
            &[],
        );
        assert_eq!(anthropic[0].short_label, "Opus 4.6");

        let grouped = group_by_provider(&[
            ModelOption {
                id: "google/gemini-2.5".to_string(),
                label: "Gemini 2.5".to_string(),
                short_label: "2.5".to_string(),
                description: None,
                is_variant: false,
                context_window: None,
                cost: None,
                provider: "google".to_string(),
            },
            ModelOption {
                id: "openai/gpt-5".to_string(),
                label: "GPT-5".to_string(),
                short_label: "GPT-5".to_string(),
                description: None,
                is_variant: false,
                context_window: None,
                cost: None,
                provider: "openai".to_string(),
            },
            ModelOption {
                id: "variant:opus-custom".to_string(),
                label: "Opus Custom".to_string(),
                short_label: "Opus Custom".to_string(),
                description: None,
                is_variant: true,
                context_window: None,
                cost: None,
                provider: "anthropic".to_string(),
            },
            anthropic[0].clone(),
        ]);
        assert_eq!(
            grouped
                .iter()
                .map(|group| group.provider.as_str())
                .collect::<Vec<_>>(),
            vec!["anthropic", "openai", "google"]
        );
        assert_eq!(grouped[0].options[0].id, "variant:opus-custom");

        let missing = with_missing_model_option(&[], Some("variant:removed"));
        assert_eq!(missing[0].label, "removed (missing)");
        let original = vec![options[0].clone()];
        assert_eq!(
            with_missing_model_option(&original, Some("openai/unknown-model")),
            original
        );
        assert_eq!(
            with_missing_model_option(&options, Some("variant:gpt-5-medium")),
            options
        );

        let default_options = vec![
            ModelOption {
                id: APP_DEFAULT_MODEL_ID.to_string(),
                provider: "anthropic".to_string(),
                ..options[0].clone()
            },
            options[0].clone(),
        ];
        assert_eq!(
            get_default_model_option_id(&default_options),
            APP_DEFAULT_MODEL_ID
        );
        assert_eq!(get_default_model_option_id(&options), "openai/gpt-5");
    }

    #[test]
    fn model_access_filters_and_sanitizes_managed_trial_preferences() {
        let request_url = "https://open-agents.dev/api/test";
        let managed = managed_trial_session();
        let base_models = vec![
            available_model("anthropic/claude-opus-4.6", None),
            available_model("anthropic/claude-haiku-4.5", None),
        ];
        assert_eq!(
            filter_models_for_session(&base_models, Some(&managed), request_url, |model| &model.id)
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["anthropic/claude-haiku-4.5"]
        );

        let variants = vec![
            opus_variant(),
            ModelVariant::new(
                "variant:user-gpt",
                "User GPT",
                "openai/gpt-5",
                BTreeMap::new(),
            ),
        ];
        assert_eq!(
            filter_model_variants_for_session(&variants, Some(&managed), request_url)
                .iter()
                .map(|variant| variant.id.as_str())
                .collect::<Vec<_>>(),
            vec!["variant:user-gpt"]
        );
        assert_eq!(
            sanitize_selected_model_id_for_session(
                Some("variant:builtin:claude-opus-4.6-high"),
                &[opus_variant()],
                Some(&managed),
                request_url,
            ),
            Some(APP_DEFAULT_MODEL_ID.to_string())
        );

        let preferences = UserModelPreferences {
            default_model_id: Some("anthropic/claude-opus-4.6".to_string()),
            default_subagent_model_id: Some("variant:builtin:claude-opus-4.6-high".to_string()),
            model_variants: vec![opus_variant()],
            enabled_model_ids: vec![
                "anthropic/claude-opus-4.6".to_string(),
                "openai/gpt-5".to_string(),
            ],
        };
        let sanitized =
            sanitize_user_preferences_for_session(&preferences, Some(&managed), request_url);
        assert_eq!(
            sanitized.default_model_id.as_deref(),
            Some(APP_DEFAULT_MODEL_ID)
        );
        assert_eq!(
            sanitized.default_subagent_model_id.as_deref(),
            Some(APP_DEFAULT_MODEL_ID)
        );
        assert!(sanitized.model_variants.is_empty());
        assert_eq!(sanitized.enabled_model_ids, vec!["openai/gpt-5"]);

        assert_eq!(
            sanitize_user_preferences_for_session(
                &preferences,
                Some(&vercel_session()),
                request_url
            ),
            preferences
        );
    }

    #[test]
    fn models_route_enriches_filters_trials_and_recovers_gateway_errors() {
        let route_models = available_models_route_models(
            Ok(vec![
                GatewayAvailableModel {
                    id: "openai/gpt-5.3-codex".to_string(),
                    model_type: Some("language".to_string()),
                    context_window: Some(200_000),
                    ..GatewayAvailableModel::default()
                },
                GatewayAvailableModel {
                    id: "anthropic/claude-opus-4.6".to_string(),
                    model_type: Some("language".to_string()),
                    context_window: Some(200_000),
                    ..GatewayAvailableModel::default()
                },
                GatewayAvailableModel {
                    id: "openai/gpt-4o-mini".to_string(),
                    model_type: Some("language".to_string()),
                    context_window: Some(128_000),
                    ..GatewayAvailableModel::default()
                },
                GatewayAvailableModel {
                    id: "openai/image-gen".to_string(),
                    model_type: Some("image".to_string()),
                    context_window: Some(200_000),
                    ..GatewayAvailableModel::default()
                },
            ]),
            &json!({
                "openai": { "models": { "gpt-5.3-codex": { "limit": { "context": 400_000 } } } },
                "anthropic": { "models": { "claude-opus-4.6": { "limit": { "context": 1_000_000 } } } }
            }),
            None,
            "http://localhost/api/models",
        )
        .expect("route models");
        let context_by_id = route_models
            .iter()
            .map(|model| (model.id.as_str(), model.context_window))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            context_by_id.get("openai/gpt-5.3-codex"),
            Some(&Some(400_000))
        );
        assert_eq!(
            context_by_id.get("anthropic/claude-opus-4.6"),
            Some(&Some(1_000_000))
        );
        assert_eq!(
            context_by_id.get("openai/gpt-4o-mini"),
            Some(&Some(128_000))
        );
        assert!(!context_by_id.contains_key("openai/image-gen"));

        let managed = managed_trial_session();
        let trial_models = available_models_route_models(
            Ok(vec![
                GatewayAvailableModel {
                    id: "anthropic/claude-opus-4.6".to_string(),
                    model_type: Some("language".to_string()),
                    ..GatewayAvailableModel::default()
                },
                GatewayAvailableModel {
                    id: "anthropic/claude-haiku-4.5".to_string(),
                    model_type: Some("language".to_string()),
                    ..GatewayAvailableModel::default()
                },
            ]),
            &json!({}),
            Some(&managed),
            "https://open-agents.dev/api/models",
        )
        .expect("trial models");
        assert_eq!(trial_models[0].id, "anthropic/claude-haiku-4.5");

        let related = available_models_route_models(
            Ok(vec![GatewayAvailableModel {
                id: "openai/gpt-5.3-codex-2026-02-15".to_string(),
                model_type: Some("language".to_string()),
                context_window: Some(200_000),
                ..GatewayAvailableModel::default()
            }]),
            &json!({
                "openai": { "models": {
                    "gpt-5": { "limit": { "context": 272_000 } },
                    "gpt-5.3-codex": { "limit": { "context": 400_000 } }
                } }
            }),
            None,
            "http://localhost/api/models",
        )
        .expect("related models");
        assert_eq!(related[0].context_window, Some(200_000));

        let cost_models = available_models_route_models(
            Ok(vec![GatewayAvailableModel {
                id: "openai/gpt-5.3-codex".to_string(),
                model_type: Some("language".to_string()),
                context_window: Some(200_000),
                ..GatewayAvailableModel::default()
            }]),
            &json!({
                "invalidProvider": "bad",
                "openai": { "models": {
                    "gpt-5.3-codex": {
                        "limit": { "context": "400_000" },
                        "cost": {
                            "input": 1.25,
                            "output": 10.0,
                            "context_over_200k": { "input": 2.5 }
                        }
                    },
                    "broken": {
                        "limit": { "context": "not-a-number" },
                        "cost": { "input": "expensive" }
                    }
                } }
            }),
            None,
            "http://localhost/api/models",
        )
        .expect("cost models");
        assert_eq!(cost_models[0].context_window, Some(200_000));
        assert_eq!(cost_models[0].cost.as_ref().unwrap().base.input, Some(1.25));
        assert_eq!(
            cost_models[0]
                .cost
                .as_ref()
                .unwrap()
                .context_over_200k
                .as_ref()
                .unwrap()
                .input,
            Some(2.5)
        );

        let recovered = available_models_route_models(
            Err(GatewayModelsError::new(json!({
                "response": { "models": [
                    {
                        "id": "openai/gpt-5.4",
                        "name": "GPT 5.4",
                        "description": "Latest GPT model",
                        "modelType": "language"
                    },
                    { "id": "openai/gpt-5.4-broken", "modelType": "language" },
                    {
                        "id": "cohere/rerank-v3.5",
                        "name": "Cohere Rerank 3.5",
                        "description": "Reranking model",
                        "modelType": "reranking"
                    }
                ] }
            }))),
            &json!({}),
            None,
            "http://localhost/api/models",
        )
        .expect("recovered models");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].id, "openai/gpt-5.4");
        assert_eq!(recovered[0].name.as_deref(), Some("GPT 5.4"));
    }

    #[test]
    fn provider_option_defaults_cover_future_gpt_anthropic_and_attribution_cases() {
        assert!(
            crate::get_open_agent_provider_options_for_model("openai/gpt-5.9", None)["openai"]
                .contains_key("reasoningSummary")
        );
        assert!(
            !crate::get_open_agent_provider_options_for_model("openai/gpt-4o", None)["openai"]
                .contains_key("reasoningSummary")
        );
        assert_eq!(
            crate::get_open_agent_provider_options_for_model("anthropic/claude-opus-4.7", None)["anthropic"]
                ["thinking"]["type"],
            json!("adaptive")
        );
    }

    #[test]
    fn model_catalog_builtin_constants_match_open_agents_defaults() {
        let ids = built_in_variants()
            .iter()
            .map(|variant| variant.id.clone())
            .collect::<BTreeSet<_>>();
        assert!(ids.contains("variant:builtin:gpt-5.4-xhigh"));
        assert!(ids.contains("variant:builtin:claude-opus-4.6-high"));
        assert_eq!(DEFAULT_MODEL_ID, "anthropic/claude-haiku-4.5");
        assert_eq!(APP_DEFAULT_MODEL_ID, "openai/gpt-5.4");
    }
}
