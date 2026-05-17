use std::future::{Future, Pending, Ready, ready};
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::provider::{ProviderOptions, SpecificationVersion};

/// Original embedding operation passed to middleware wrappers.
pub type EmbeddingModelDoEmbed<'a> = Box<
    dyn FnOnce() -> Pin<Box<dyn Future<Output = EmbeddingModelResult> + Send + 'a>> + Send + 'a,
>;

/// Options passed to embedding middleware hooks that only inspect the model.
#[derive(Debug)]
pub struct EmbeddingModelMiddlewareModelOptions<'a, M: EmbeddingModel> {
    /// The embedding model being wrapped.
    pub model: &'a M,
}

impl<'a, M: EmbeddingModel> EmbeddingModelMiddlewareModelOptions<'a, M> {
    /// Creates model-only middleware hook options.
    pub fn new(model: &'a M) -> Self {
        Self { model }
    }
}

/// Options passed to embedding middleware parameter transforms.
#[derive(Debug)]
pub struct EmbeddingModelTransformParamsOptions<'a, M: EmbeddingModel> {
    /// Original embedding call options.
    pub params: EmbeddingModelCallOptions,

    /// The embedding model being wrapped.
    pub model: &'a M,
}

impl<'a, M: EmbeddingModel> EmbeddingModelTransformParamsOptions<'a, M> {
    /// Creates transform-params middleware hook options.
    pub fn new(params: EmbeddingModelCallOptions, model: &'a M) -> Self {
        Self { params, model }
    }
}

/// Options passed to embedding middleware operation wrappers.
pub struct EmbeddingModelWrapEmbedOptions<'a, M: EmbeddingModel> {
    /// Original embedding operation.
    pub do_embed: EmbeddingModelDoEmbed<'a>,

    /// Embedding call options, transformed if a transform hook ran first.
    pub params: EmbeddingModelCallOptions,

    /// The embedding model being wrapped.
    pub model: &'a M,
}

impl<'a, M: EmbeddingModel> EmbeddingModelWrapEmbedOptions<'a, M> {
    /// Creates wrap-embed middleware hook options.
    pub fn new(
        do_embed: EmbeddingModelDoEmbed<'a>,
        params: EmbeddingModelCallOptions,
        model: &'a M,
    ) -> Self {
        Self {
            do_embed,
            params,
            model,
        }
    }
}

/// Middleware for provider-v4 embedding models.
///
/// Upstream `EmbeddingModelV4Middleware` exposes optional hooks for overriding
/// identity/capability values, transforming call options, and wrapping
/// `doEmbed`. This Rust trait represents optional hooks as methods that return
/// `None` when the middleware does not handle that step.
pub trait EmbeddingModelMiddleware<M: EmbeddingModel> {
    /// Future returned by [`EmbeddingModelMiddleware::override_max_embeddings_per_call`].
    type OverrideMaxEmbeddingsPerCallFuture<'a>: Future<Output = Option<usize>> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`EmbeddingModelMiddleware::override_supports_parallel_calls`].
    type OverrideSupportsParallelCallsFuture<'a>: Future<Output = bool> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`EmbeddingModelMiddleware::transform_params`].
    type TransformParamsFuture<'a>: Future<Output = EmbeddingModelCallOptions> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`EmbeddingModelMiddleware::wrap_embed`].
    type WrapEmbedFuture<'a>: Future<Output = EmbeddingModelResult> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Returns the middleware interface version.
    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    /// Optionally overrides the provider identifier reported by the model.
    fn override_provider(
        &self,
        options: EmbeddingModelMiddlewareModelOptions<'_, M>,
    ) -> Option<String> {
        let _ = options;
        None
    }

    /// Optionally overrides the provider-specific model id.
    fn override_model_id(
        &self,
        options: EmbeddingModelMiddlewareModelOptions<'_, M>,
    ) -> Option<String> {
        let _ = options;
        None
    }

    /// Optionally overrides the model's maximum embeddings per call capability.
    fn override_max_embeddings_per_call<'a>(
        &'a self,
        options: EmbeddingModelMiddlewareModelOptions<'a, M>,
    ) -> Option<Self::OverrideMaxEmbeddingsPerCallFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally overrides whether the model supports parallel embedding calls.
    fn override_supports_parallel_calls<'a>(
        &'a self,
        options: EmbeddingModelMiddlewareModelOptions<'a, M>,
    ) -> Option<Self::OverrideSupportsParallelCallsFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally transforms call options before invoking the model.
    fn transform_params<'a>(
        &'a self,
        options: EmbeddingModelTransformParamsOptions<'a, M>,
    ) -> Option<Self::TransformParamsFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally wraps the model's embedding operation.
    fn wrap_embed<'a>(
        &'a self,
        options: EmbeddingModelWrapEmbedOptions<'a, M>,
    ) -> Option<Self::WrapEmbedFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }
}

/// Default provider call settings applied by [`DefaultEmbeddingSettingsMiddleware`].
///
/// Upstream `defaultEmbeddingSettingsMiddleware` accepts a partial provider-v4
/// embedding call options object with `headers` and `providerOptions`. Rust
/// keeps that same boundary as an explicit settings record and treats `None`
/// as "no default".
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingModelDefaultSettings {
    /// Default HTTP headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Default provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl EmbeddingModelDefaultSettings {
    /// Creates an empty default-settings record.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a default HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the default provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// Embedding model middleware that applies default call settings.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultEmbeddingSettingsMiddleware {
    /// Settings applied when callers leave the corresponding call option unset.
    pub settings: EmbeddingModelDefaultSettings,
}

impl DefaultEmbeddingSettingsMiddleware {
    /// Creates default-settings middleware from a settings record.
    pub fn new(settings: EmbeddingModelDefaultSettings) -> Self {
        Self { settings }
    }

    fn apply_settings(&self, mut params: EmbeddingModelCallOptions) -> EmbeddingModelCallOptions {
        params.headers = merge_headers(self.settings.headers.as_ref(), params.headers);
        params.provider_options = merge_provider_options_deep(
            self.settings.provider_options.as_ref(),
            params.provider_options,
        );

        params
    }
}

/// Creates embedding model middleware that applies default call settings.
pub fn default_embedding_settings_middleware(
    settings: EmbeddingModelDefaultSettings,
) -> DefaultEmbeddingSettingsMiddleware {
    DefaultEmbeddingSettingsMiddleware::new(settings)
}

impl<M: EmbeddingModel> EmbeddingModelMiddleware<M> for DefaultEmbeddingSettingsMiddleware {
    type OverrideMaxEmbeddingsPerCallFuture<'a>
        = Pending<Option<usize>>
    where
        Self: 'a,
        M: 'a;

    type OverrideSupportsParallelCallsFuture<'a>
        = Pending<bool>
    where
        Self: 'a,
        M: 'a;

    type TransformParamsFuture<'a>
        = Ready<EmbeddingModelCallOptions>
    where
        Self: 'a,
        M: 'a;

    type WrapEmbedFuture<'a>
        = Pending<EmbeddingModelResult>
    where
        Self: 'a,
        M: 'a;

    fn transform_params<'a>(
        &'a self,
        options: EmbeddingModelTransformParamsOptions<'a, M>,
    ) -> Option<Self::TransformParamsFuture<'a>>
    where
        M: 'a,
    {
        Some(ready(self.apply_settings(options.params)))
    }
}

fn merge_headers(
    default_headers: Option<&Headers>,
    params_headers: Option<Headers>,
) -> Option<Headers> {
    if default_headers.is_none() && params_headers.is_none() {
        return None;
    }

    let mut headers = default_headers.cloned().unwrap_or_default();

    if let Some(params_headers) = params_headers {
        headers.extend(params_headers);
    }

    Some(headers)
}

fn merge_provider_options_deep(
    default_provider_options: Option<&ProviderOptions>,
    params_provider_options: Option<ProviderOptions>,
) -> Option<ProviderOptions> {
    if default_provider_options.is_none() && params_provider_options.is_none() {
        return None;
    }

    let mut provider_options = default_provider_options.cloned().unwrap_or_default();

    if let Some(params_provider_options) = params_provider_options {
        for (provider, params_options) in params_provider_options {
            match provider_options.get_mut(&provider) {
                Some(default_options) => {
                    *default_options = merge_json_objects(default_options, &params_options);
                }
                None => {
                    provider_options.insert(provider, params_options);
                }
            }
        }
    }

    Some(provider_options)
}

fn merge_json_objects(default_object: &JsonObject, params_object: &JsonObject) -> JsonObject {
    let mut merged = default_object.clone();

    for (key, params_value) in params_object {
        if matches!(key.as_str(), "__proto__" | "constructor" | "prototype") {
            continue;
        }

        let merged_value = match (merged.get(key), params_value) {
            (Some(JsonValue::Object(default_nested)), JsonValue::Object(params_nested)) => {
                JsonValue::Object(merge_json_objects(default_nested, params_nested))
            }
            _ => params_value.clone(),
        };

        merged.insert(key.clone(), merged_value);
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::{
        EmbeddingModelDefaultSettings, EmbeddingModelMiddleware,
        EmbeddingModelMiddlewareModelOptions, EmbeddingModelTransformParamsOptions,
        EmbeddingModelWrapEmbedOptions, default_embedding_settings_middleware,
    };
    use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult};
    use crate::provider::SpecificationVersion;
    use crate::warning::Warning;
    use serde_json::json;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    struct StaticEmbeddingModel;

    impl EmbeddingModel for StaticEmbeddingModel {
        type MaxEmbeddingsPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;

        type SupportsParallelCallsFuture<'a>
            = Ready<bool>
        where
            Self: 'a;

        type EmbedFuture<'a>
            = Ready<EmbeddingModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "base-provider"
        }

        fn model_id(&self) -> &str {
            "embed-base"
        }

        fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
            ready(Some(4))
        }

        fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
            ready(true)
        }

        fn do_embed(&self, _options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
            ready(EmbeddingModelResult::new(vec![vec![1.0, 2.0]]))
        }
    }

    struct StaticEmbeddingMiddleware;

    impl EmbeddingModelMiddleware<StaticEmbeddingModel> for StaticEmbeddingMiddleware {
        type OverrideMaxEmbeddingsPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            StaticEmbeddingModel: 'a;

        type OverrideSupportsParallelCallsFuture<'a>
            = Ready<bool>
        where
            Self: 'a,
            StaticEmbeddingModel: 'a;

        type TransformParamsFuture<'a>
            = Ready<EmbeddingModelCallOptions>
        where
            Self: 'a,
            StaticEmbeddingModel: 'a;

        type WrapEmbedFuture<'a>
            = Pin<Box<dyn Future<Output = EmbeddingModelResult> + Send + 'a>>
        where
            Self: 'a,
            StaticEmbeddingModel: 'a;

        fn override_provider(
            &self,
            options: EmbeddingModelMiddlewareModelOptions<'_, StaticEmbeddingModel>,
        ) -> Option<String> {
            Some(format!("{}-middleware", options.model.provider()))
        }

        fn override_model_id(
            &self,
            options: EmbeddingModelMiddlewareModelOptions<'_, StaticEmbeddingModel>,
        ) -> Option<String> {
            Some(format!("{}-wrapped", options.model.model_id()))
        }

        fn override_max_embeddings_per_call<'a>(
            &'a self,
            options: EmbeddingModelMiddlewareModelOptions<'a, StaticEmbeddingModel>,
        ) -> Option<Self::OverrideMaxEmbeddingsPerCallFuture<'a>>
        where
            Self: 'a,
            StaticEmbeddingModel: 'a,
        {
            assert_eq!(options.model.model_id(), "embed-base");
            Some(ready(Some(8)))
        }

        fn override_supports_parallel_calls<'a>(
            &'a self,
            options: EmbeddingModelMiddlewareModelOptions<'a, StaticEmbeddingModel>,
        ) -> Option<Self::OverrideSupportsParallelCallsFuture<'a>>
        where
            Self: 'a,
            StaticEmbeddingModel: 'a,
        {
            assert_eq!(options.model.provider(), "base-provider");
            Some(ready(false))
        }

        fn transform_params<'a>(
            &'a self,
            mut options: EmbeddingModelTransformParamsOptions<'a, StaticEmbeddingModel>,
        ) -> Option<Self::TransformParamsFuture<'a>>
        where
            Self: 'a,
            StaticEmbeddingModel: 'a,
        {
            assert_eq!(options.model.provider(), "base-provider");
            options
                .params
                .values
                .push("added-by-middleware".to_string());
            Some(ready(options.params))
        }

        fn wrap_embed<'a>(
            &'a self,
            options: EmbeddingModelWrapEmbedOptions<'a, StaticEmbeddingModel>,
        ) -> Option<Self::WrapEmbedFuture<'a>>
        where
            Self: 'a,
            StaticEmbeddingModel: 'a,
        {
            assert_eq!(options.params.values, ["input"]);

            Some(Box::pin(async move {
                let mut result = (options.do_embed)().await;
                result.warnings.push(Warning::Other {
                    message: "wrapped".to_string(),
                });
                result
            }))
        }
    }

    fn poll_ready<T>(mut future: Ready<T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("std::future::Ready never returns pending"),
        }
    }

    fn poll_boxed<T>(mut future: Pin<Box<dyn Future<Output = T> + Send + '_>>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test future only awaits ready futures"),
        }
    }

    #[test]
    fn embedding_model_middleware_exposes_upstream_v4_hooks() {
        let model = StaticEmbeddingModel;
        let middleware = StaticEmbeddingMiddleware;

        assert_eq!(middleware.specification_version(), SpecificationVersion::V4);
        assert_eq!(
            middleware.override_provider(EmbeddingModelMiddlewareModelOptions::new(&model)),
            Some("base-provider-middleware".to_string())
        );
        assert_eq!(
            middleware.override_model_id(EmbeddingModelMiddlewareModelOptions::new(&model)),
            Some("embed-base-wrapped".to_string())
        );

        let max_embeddings = middleware
            .override_max_embeddings_per_call(EmbeddingModelMiddlewareModelOptions::new(&model))
            .expect("max embeddings hook is implemented");
        assert_eq!(poll_ready(max_embeddings), Some(8));

        let supports_parallel = middleware
            .override_supports_parallel_calls(EmbeddingModelMiddlewareModelOptions::new(&model))
            .expect("parallel-call hook is implemented");
        assert!(!poll_ready(supports_parallel));

        let transformed = middleware
            .transform_params(EmbeddingModelTransformParamsOptions::new(
                EmbeddingModelCallOptions::new(vec!["input".to_string()]),
                &model,
            ))
            .expect("transform hook is implemented");
        assert_eq!(
            poll_ready(transformed).values,
            ["input".to_string(), "added-by-middleware".to_string()]
        );

        let wrapped = middleware
            .wrap_embed(EmbeddingModelWrapEmbedOptions::new(
                Box::new(|| Box::pin(ready(EmbeddingModelResult::new(vec![vec![3.0, 4.0]])))),
                EmbeddingModelCallOptions::new(vec!["input".to_string()]),
                &model,
            ))
            .expect("wrap hook is implemented");
        let result = poll_boxed(wrapped);

        assert_eq!(result.embeddings, vec![vec![3.0, 4.0]]);
        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "wrapped".to_string(),
            }]
        );
    }

    #[test]
    fn embedding_model_middleware_hooks_are_optional_by_default() {
        struct NoopMiddleware;

        impl EmbeddingModelMiddleware<StaticEmbeddingModel> for NoopMiddleware {
            type OverrideMaxEmbeddingsPerCallFuture<'a>
                = Ready<Option<usize>>
            where
                Self: 'a,
                StaticEmbeddingModel: 'a;

            type OverrideSupportsParallelCallsFuture<'a>
                = Ready<bool>
            where
                Self: 'a,
                StaticEmbeddingModel: 'a;

            type TransformParamsFuture<'a>
                = Ready<EmbeddingModelCallOptions>
            where
                Self: 'a,
                StaticEmbeddingModel: 'a;

            type WrapEmbedFuture<'a>
                = Ready<EmbeddingModelResult>
            where
                Self: 'a,
                StaticEmbeddingModel: 'a;
        }

        let model = StaticEmbeddingModel;
        let middleware = NoopMiddleware;

        assert_eq!(
            middleware.override_provider(EmbeddingModelMiddlewareModelOptions::new(&model)),
            None
        );
        assert_eq!(
            middleware.override_model_id(EmbeddingModelMiddlewareModelOptions::new(&model)),
            None
        );
        assert!(
            middleware
                .override_max_embeddings_per_call(EmbeddingModelMiddlewareModelOptions::new(&model))
                .is_none()
        );
        assert!(
            middleware
                .override_supports_parallel_calls(EmbeddingModelMiddlewareModelOptions::new(&model))
                .is_none()
        );
        assert!(
            middleware
                .transform_params(EmbeddingModelTransformParamsOptions::new(
                    EmbeddingModelCallOptions::new(vec!["input".to_string()]),
                    &model,
                ))
                .is_none()
        );
        assert!(
            middleware
                .wrap_embed(EmbeddingModelWrapEmbedOptions::new(
                    Box::new(|| Box::pin(ready(EmbeddingModelResult::new(Vec::new())))),
                    EmbeddingModelCallOptions::new(vec!["input".to_string()]),
                    &model,
                ))
                .is_none()
        );
    }

    #[test]
    fn embedding_model_default_settings_serialize_as_upstream_partial_call_options() {
        let provider_options = serde_json::from_value(json!({
            "google": {
                "outputDimensionality": 512
            }
        }))
        .expect("provider options deserialize");
        let settings = EmbeddingModelDefaultSettings::new()
            .with_header("X-Default-Header", "default")
            .with_provider_options(provider_options);

        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "headers": {
                    "X-Default-Header": "default"
                },
                "providerOptions": {
                    "google": {
                        "outputDimensionality": 512
                    }
                }
            })
        );
    }

    #[test]
    fn default_embedding_settings_middleware_applies_headers_without_overriding_params() {
        let model = StaticEmbeddingModel;
        let middleware = default_embedding_settings_middleware(
            EmbeddingModelDefaultSettings::new()
                .with_header("X-Default-Header", "default")
                .with_header("X-Custom-Header", "default-custom"),
        );

        let transformed = middleware
            .transform_params(EmbeddingModelTransformParamsOptions::new(
                EmbeddingModelCallOptions::new(vec!["hello world".to_string()])
                    .with_header("X-Custom-Header", "caller-custom"),
                &model,
            ))
            .expect("default settings implements transform params");
        let params = poll_ready(transformed);

        assert_eq!(params.values, ["hello world"]);
        assert_eq!(
            params.headers,
            Some(
                serde_json::from_value(json!({
                    "X-Default-Header": "default",
                    "X-Custom-Header": "caller-custom"
                }))
                .expect("headers deserialize")
            )
        );
    }

    #[test]
    fn default_embedding_settings_middleware_deep_merges_provider_options() {
        let model = StaticEmbeddingModel;
        let default_provider_options = serde_json::from_value(json!({
            "google": {
                "outputDimensionality": 512,
                "nested": {
                    "default": true,
                    "caller": false
                }
            },
            "openai": {
                "dimensions": 256
            }
        }))
        .expect("default provider options deserialize");
        let caller_provider_options = serde_json::from_value(json!({
            "google": {
                "taskType": "SEMANTIC_SIMILARITY",
                "nested": {
                    "caller": true
                }
            }
        }))
        .expect("caller provider options deserialize");
        let middleware = default_embedding_settings_middleware(
            EmbeddingModelDefaultSettings::new().with_provider_options(default_provider_options),
        );

        let transformed = middleware
            .transform_params(EmbeddingModelTransformParamsOptions::new(
                EmbeddingModelCallOptions::new(vec!["hello world".to_string()])
                    .with_provider_options(caller_provider_options),
                &model,
            ))
            .expect("default settings implements transform params");
        let params = poll_ready(transformed);

        assert_eq!(
            params.provider_options,
            Some(
                serde_json::from_value(json!({
                    "google": {
                        "outputDimensionality": 512,
                        "taskType": "SEMANTIC_SIMILARITY",
                        "nested": {
                            "default": true,
                            "caller": true
                        }
                    },
                    "openai": {
                        "dimensions": 256
                    }
                }))
                .expect("provider options deserialize")
            )
        );
    }

    #[test]
    fn default_embedding_settings_middleware_preserves_none_when_no_defaults_or_params() {
        let model = StaticEmbeddingModel;
        let middleware =
            default_embedding_settings_middleware(EmbeddingModelDefaultSettings::new());

        let transformed = middleware
            .transform_params(EmbeddingModelTransformParamsOptions::new(
                EmbeddingModelCallOptions::new(vec!["hello world".to_string()]),
                &model,
            ))
            .expect("default settings implements transform params");
        let params = poll_ready(transformed);

        assert_eq!(params.headers, None);
        assert_eq!(params.provider_options, None);
    }
}
