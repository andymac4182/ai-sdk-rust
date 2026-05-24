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

/// Embedding model wrapper that applies one middleware around a provider-v4 model.
///
/// Upstream `wrapEmbeddingModel` accepts one or more middlewares. This Rust
/// wrapper models the same behavior for a single middleware without allocating a
/// middleware collection; callers can wrap the returned model again to compose
/// additional middleware.
#[derive(Clone, Debug)]
pub struct WrappedEmbeddingModel<M, W> {
    model: M,
    middleware: W,
    provider_id: String,
    model_id: String,
}

impl<M, W> WrappedEmbeddingModel<M, W>
where
    M: EmbeddingModel,
    W: EmbeddingModelMiddleware<M>,
{
    /// Creates an embedding model wrapper using middleware-provided identity
    /// overrides when present.
    pub fn new(model: M, middleware: W) -> Self {
        let provider_id = middleware
            .override_provider(EmbeddingModelMiddlewareModelOptions::new(&model))
            .unwrap_or_else(|| model.provider().to_string());
        let model_id = middleware
            .override_model_id(EmbeddingModelMiddlewareModelOptions::new(&model))
            .unwrap_or_else(|| model.model_id().to_string());

        Self {
            model,
            middleware,
            provider_id,
            model_id,
        }
    }

    /// Sets an explicit provider id, taking precedence over middleware identity
    /// overrides and the wrapped model's provider id.
    pub fn with_provider_id(mut self, provider_id: impl Into<String>) -> Self {
        self.provider_id = provider_id.into();
        self
    }

    /// Sets an explicit model id, taking precedence over middleware identity
    /// overrides and the wrapped model's model id.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Returns the wrapped base embedding model.
    pub fn model(&self) -> &M {
        &self.model
    }

    /// Returns the middleware applied by this wrapper.
    pub fn middleware(&self) -> &W {
        &self.middleware
    }

    /// Consumes the wrapper into the base model and middleware.
    pub fn into_parts(self) -> (M, W) {
        (self.model, self.middleware)
    }
}

/// Wraps an embedding model with middleware.
pub fn wrap_embedding_model<M, W>(model: M, middleware: W) -> WrappedEmbeddingModel<M, W>
where
    M: EmbeddingModel,
    W: EmbeddingModelMiddleware<M>,
{
    WrappedEmbeddingModel::new(model, middleware)
}

impl<M, W> EmbeddingModel for WrappedEmbeddingModel<M, W>
where
    M: EmbeddingModel + Sync,
    W: EmbeddingModelMiddleware<M> + Sync,
{
    type MaxEmbeddingsPerCallFuture<'a>
        = Pin<Box<dyn Future<Output = Option<usize>> + Send + 'a>>
    where
        Self: 'a;

    type SupportsParallelCallsFuture<'a>
        = Pin<Box<dyn Future<Output = bool> + Send + 'a>>
    where
        Self: 'a;

    type EmbedFuture<'a>
        = Pin<Box<dyn Future<Output = EmbeddingModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.provider_id
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
        Box::pin(async move {
            if let Some(max_embeddings_per_call) = self.middleware.override_max_embeddings_per_call(
                EmbeddingModelMiddlewareModelOptions::new(&self.model),
            ) {
                max_embeddings_per_call.await
            } else {
                self.model.max_embeddings_per_call().await
            }
        })
    }

    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
        Box::pin(async move {
            if let Some(supports_parallel_calls) = self.middleware.override_supports_parallel_calls(
                EmbeddingModelMiddlewareModelOptions::new(&self.model),
            ) {
                supports_parallel_calls.await
            } else {
                self.model.supports_parallel_calls().await
            }
        })
    }

    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
        Box::pin(async move {
            let params = if let Some(transform_params) =
                self.middleware
                    .transform_params(EmbeddingModelTransformParamsOptions::new(
                        options.clone(),
                        &self.model,
                    )) {
                transform_params.await
            } else {
                options
            };

            let do_embed_params = params.clone();
            let fallback_params = params.clone();
            let model = &self.model;
            let do_embed: EmbeddingModelDoEmbed<'_> =
                Box::new(move || Box::pin(model.do_embed(do_embed_params)));

            if let Some(wrap_embed) =
                self.middleware
                    .wrap_embed(EmbeddingModelWrapEmbedOptions::new(
                        do_embed,
                        params,
                        &self.model,
                    ))
            {
                wrap_embed.await
            } else {
                self.model.do_embed(fallback_params).await
            }
        })
    }
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
        wrap_embedding_model,
    };
    use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult};
    use crate::provider::SpecificationVersion;
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::BTreeMap;
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

    struct ParamEchoEmbeddingModel;

    impl EmbeddingModel for ParamEchoEmbeddingModel {
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
            "echo-provider"
        }

        fn model_id(&self) -> &str {
            "echo-embedding"
        }

        fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
            ready(Some(2))
        }

        fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
            ready(true)
        }

        fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
            ready(
                EmbeddingModelResult::new(vec![vec![options.values.len() as f64]]).with_warning(
                    Warning::Other {
                        message: options.values.join("|"),
                    },
                ),
            )
        }
    }

    struct TransformAndWrapEmbeddingMiddleware;

    impl EmbeddingModelMiddleware<ParamEchoEmbeddingModel> for TransformAndWrapEmbeddingMiddleware {
        type OverrideMaxEmbeddingsPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            ParamEchoEmbeddingModel: 'a;

        type OverrideSupportsParallelCallsFuture<'a>
            = Ready<bool>
        where
            Self: 'a,
            ParamEchoEmbeddingModel: 'a;

        type TransformParamsFuture<'a>
            = Ready<EmbeddingModelCallOptions>
        where
            Self: 'a,
            ParamEchoEmbeddingModel: 'a;

        type WrapEmbedFuture<'a>
            = Pin<Box<dyn Future<Output = EmbeddingModelResult> + Send + 'a>>
        where
            Self: 'a,
            ParamEchoEmbeddingModel: 'a;

        fn transform_params<'a>(
            &'a self,
            mut options: EmbeddingModelTransformParamsOptions<'a, ParamEchoEmbeddingModel>,
        ) -> Option<Self::TransformParamsFuture<'a>>
        where
            Self: 'a,
            ParamEchoEmbeddingModel: 'a,
        {
            options.params.values.push("transformed".to_string());
            Some(ready(options.params))
        }

        fn wrap_embed<'a>(
            &'a self,
            options: EmbeddingModelWrapEmbedOptions<'a, ParamEchoEmbeddingModel>,
        ) -> Option<Self::WrapEmbedFuture<'a>>
        where
            Self: 'a,
            ParamEchoEmbeddingModel: 'a,
        {
            assert_eq!(options.params.values, ["input", "transformed"]);

            Some(Box::pin(async move {
                let mut result = (options.do_embed)().await;
                result.warnings.push(Warning::Other {
                    message: format!("wrapped {}", options.model.model_id()),
                });
                result
            }))
        }
    }

    #[derive(Clone, Copy)]
    struct NoopEmbeddingMiddleware;

    impl<M: EmbeddingModel> EmbeddingModelMiddleware<M> for NoopEmbeddingMiddleware {
        type OverrideMaxEmbeddingsPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            M: 'a;

        type OverrideSupportsParallelCallsFuture<'a>
            = Ready<bool>
        where
            Self: 'a,
            M: 'a;

        type TransformParamsFuture<'a>
            = Ready<EmbeddingModelCallOptions>
        where
            Self: 'a,
            M: 'a;

        type WrapEmbedFuture<'a>
            = Ready<EmbeddingModelResult>
        where
            Self: 'a,
            M: 'a;
    }

    #[derive(Clone, Copy)]
    struct AppendEmbeddingValueMiddleware(&'static str);

    impl<M: EmbeddingModel> EmbeddingModelMiddleware<M> for AppendEmbeddingValueMiddleware {
        type OverrideMaxEmbeddingsPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            M: 'a;

        type OverrideSupportsParallelCallsFuture<'a>
            = Ready<bool>
        where
            Self: 'a,
            M: 'a;

        type TransformParamsFuture<'a>
            = Ready<EmbeddingModelCallOptions>
        where
            Self: 'a,
            M: 'a;

        type WrapEmbedFuture<'a>
            = Ready<EmbeddingModelResult>
        where
            Self: 'a,
            M: 'a;

        fn transform_params<'a>(
            &'a self,
            mut options: EmbeddingModelTransformParamsOptions<'a, M>,
        ) -> Option<Self::TransformParamsFuture<'a>>
        where
            Self: 'a,
            M: 'a,
        {
            options.params.values.push(self.0.to_string());
            Some(ready(options.params))
        }
    }

    #[derive(Clone, Copy)]
    struct AppendEmbeddingWarningMiddleware(&'static str);

    impl<M: EmbeddingModel> EmbeddingModelMiddleware<M> for AppendEmbeddingWarningMiddleware {
        type OverrideMaxEmbeddingsPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            M: 'a;

        type OverrideSupportsParallelCallsFuture<'a>
            = Ready<bool>
        where
            Self: 'a,
            M: 'a;

        type TransformParamsFuture<'a>
            = Ready<EmbeddingModelCallOptions>
        where
            Self: 'a,
            M: 'a;

        type WrapEmbedFuture<'a>
            = Pin<Box<dyn Future<Output = EmbeddingModelResult> + Send + 'a>>
        where
            Self: 'a,
            M: 'a;

        fn wrap_embed<'a>(
            &'a self,
            options: EmbeddingModelWrapEmbedOptions<'a, M>,
        ) -> Option<Self::WrapEmbedFuture<'a>>
        where
            Self: 'a,
            M: 'a,
        {
            Some(Box::pin(async move {
                let mut result = (options.do_embed)().await;
                result.warnings.push(Warning::Other {
                    message: self.0.to_string(),
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

    fn embedding_provider_options(value: serde_json::Value) -> crate::provider::ProviderOptions {
        serde_json::from_value(value).expect("provider options deserialize")
    }

    fn transform_default_embedding_settings(
        settings: EmbeddingModelDefaultSettings,
        params: EmbeddingModelCallOptions,
    ) -> EmbeddingModelCallOptions {
        let middleware = default_embedding_settings_middleware(settings);
        let transformed = middleware
            .transform_params(EmbeddingModelTransformParamsOptions::new(
                params,
                &StaticEmbeddingModel,
            ))
            .expect("default settings implements transform params");

        poll_ready(transformed)
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
        let model = StaticEmbeddingModel;
        let middleware = NoopEmbeddingMiddleware;

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
    fn wrap_embedding_model_applies_identity_and_capability_overrides() {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, StaticEmbeddingMiddleware);

        assert_eq!(wrapped.specification_version(), SpecificationVersion::V4);
        assert_eq!(wrapped.provider(), "base-provider-middleware");
        assert_eq!(wrapped.model_id(), "embed-base-wrapped");
        assert_eq!(poll_boxed(wrapped.max_embeddings_per_call()), Some(8));
        assert!(!poll_boxed(wrapped.supports_parallel_calls()));

        let explicit = wrap_embedding_model(StaticEmbeddingModel, StaticEmbeddingMiddleware)
            .with_provider_id("explicit-provider")
            .with_model_id("explicit-model");

        assert_eq!(explicit.provider(), "explicit-provider");
        assert_eq!(explicit.model_id(), "explicit-model");
    }

    #[test]
    fn wrap_embedding_model_transforms_params_before_wrapping_embed() {
        let wrapped =
            wrap_embedding_model(ParamEchoEmbeddingModel, TransformAndWrapEmbeddingMiddleware);

        let result =
            poll_boxed(wrapped.do_embed(EmbeddingModelCallOptions::new(vec!["input".to_string()])));

        assert_eq!(result.embeddings, vec![vec![2.0]]);
        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "input|transformed".to_string(),
                },
                Warning::Other {
                    message: "wrapped echo-embedding".to_string(),
                }
            ]
        );
    }

    #[test]
    fn wrap_embedding_model_model_property_should_pass_through_by_default() {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, NoopEmbeddingMiddleware);

        assert_eq!(wrapped.model_id(), "embed-base");
    }

    #[test]
    fn wrap_embedding_model_model_property_should_use_middleware_override_model_id_if_provided() {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, StaticEmbeddingMiddleware);

        assert_eq!(wrapped.model_id(), "embed-base-wrapped");
    }

    #[test]
    fn wrap_embedding_model_model_property_should_use_model_id_parameter_if_provided() {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, StaticEmbeddingMiddleware)
            .with_model_id("override-model");

        assert_eq!(wrapped.model_id(), "override-model");
    }

    #[test]
    fn wrap_embedding_model_provider_property_should_pass_through_by_default() {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, NoopEmbeddingMiddleware);

        assert_eq!(wrapped.provider(), "base-provider");
    }

    #[test]
    fn wrap_embedding_model_provider_property_should_use_middleware_override_provider_if_provided()
    {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, StaticEmbeddingMiddleware);

        assert_eq!(wrapped.provider(), "base-provider-middleware");
    }

    #[test]
    fn wrap_embedding_model_provider_property_should_use_provider_id_parameter_if_provided() {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, StaticEmbeddingMiddleware)
            .with_provider_id("override-provider");

        assert_eq!(wrapped.provider(), "override-provider");
    }

    #[test]
    fn wrap_embedding_model_max_embeddings_per_call_property_should_pass_through_by_default() {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, NoopEmbeddingMiddleware);

        assert_eq!(poll_boxed(wrapped.max_embeddings_per_call()), Some(4));
    }

    #[test]
    fn wrap_embedding_model_max_embeddings_per_call_property_should_use_middleware_override_if_provided()
     {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, StaticEmbeddingMiddleware);

        assert_eq!(poll_boxed(wrapped.max_embeddings_per_call()), Some(8));
    }

    #[test]
    fn wrap_embedding_model_supports_parallel_calls_property_should_pass_through_by_default() {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, NoopEmbeddingMiddleware);

        assert!(poll_boxed(wrapped.supports_parallel_calls()));
    }

    #[test]
    fn wrap_embedding_model_supports_parallel_calls_property_should_use_middleware_override_if_provided()
     {
        let wrapped = wrap_embedding_model(StaticEmbeddingModel, StaticEmbeddingMiddleware);

        assert!(!poll_boxed(wrapped.supports_parallel_calls()));
    }

    #[test]
    fn wrap_embedding_model_should_call_transform_params_middleware_for_do_embed() {
        let wrapped = wrap_embedding_model(
            ParamEchoEmbeddingModel,
            AppendEmbeddingValueMiddleware("step"),
        );

        let result =
            poll_boxed(wrapped.do_embed(EmbeddingModelCallOptions::new(vec!["input".to_string()])));

        assert_eq!(result.embeddings, vec![vec![2.0]]);
        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "input|step".to_string(),
            }]
        );
    }

    #[test]
    fn wrap_embedding_model_should_call_wrap_embed_middleware() {
        let wrapped = wrap_embedding_model(
            ParamEchoEmbeddingModel,
            AppendEmbeddingWarningMiddleware("wrap"),
        );

        let result =
            poll_boxed(wrapped.do_embed(EmbeddingModelCallOptions::new(vec!["input".to_string()])));

        assert_eq!(result.embeddings, vec![vec![1.0]]);
        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "input".to_string(),
                },
                Warning::Other {
                    message: "wrap".to_string(),
                }
            ]
        );
    }

    #[test]
    fn wrap_embedding_model_should_call_multiple_transform_params_middlewares_in_sequence_for_do_embed()
     {
        let wrapped = wrap_embedding_model(
            wrap_embedding_model(
                ParamEchoEmbeddingModel,
                AppendEmbeddingValueMiddleware("step-1"),
            ),
            AppendEmbeddingValueMiddleware("step-2"),
        );

        let result =
            poll_boxed(wrapped.do_embed(EmbeddingModelCallOptions::new(vec!["input".to_string()])));

        assert_eq!(result.embeddings, vec![vec![3.0]]);
        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "input|step-2|step-1".to_string(),
            }]
        );
    }

    #[test]
    fn wrap_embedding_model_should_chain_multiple_wrap_embed_middlewares_in_the_correct_order() {
        let wrapped = wrap_embedding_model(
            wrap_embedding_model(
                ParamEchoEmbeddingModel,
                AppendEmbeddingWarningMiddleware("inner"),
            ),
            AppendEmbeddingWarningMiddleware("outer"),
        );

        let result =
            poll_boxed(wrapped.do_embed(EmbeddingModelCallOptions::new(vec!["input".to_string()])));

        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "input".to_string(),
                },
                Warning::Other {
                    message: "inner".to_string(),
                },
                Warning::Other {
                    message: "outer".to_string(),
                }
            ]
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

    #[test]
    fn default_embedding_settings_middleware_should_merge_headers() {
        let params = transform_default_embedding_settings(
            EmbeddingModelDefaultSettings::new()
                .with_header("X-Custom-Header", "test")
                .with_header("X-Another-Header", "test2"),
            EmbeddingModelCallOptions::new(vec!["hello world".to_string()])
                .with_header("X-Custom-Header", "test2"),
        );

        assert_eq!(
            params.headers,
            Some(BTreeMap::from([
                ("X-Custom-Header".to_string(), "test2".to_string()),
                ("X-Another-Header".to_string(), "test2".to_string()),
            ]))
        );
    }

    #[test]
    fn default_embedding_settings_middleware_should_handle_empty_default_headers() {
        let mut settings = EmbeddingModelDefaultSettings::new();
        settings.headers = Some(BTreeMap::new());

        let params = transform_default_embedding_settings(
            settings,
            EmbeddingModelCallOptions::new(vec!["hello world".to_string()])
                .with_header("X-Param-Header", "param"),
        );

        assert_eq!(
            params.headers,
            Some(BTreeMap::from([(
                "X-Param-Header".to_string(),
                "param".to_string()
            )]))
        );
    }

    #[test]
    fn default_embedding_settings_middleware_should_handle_empty_param_headers() {
        let mut call_options = EmbeddingModelCallOptions::new(vec!["hello world".to_string()]);
        call_options.headers = Some(BTreeMap::new());

        let params = transform_default_embedding_settings(
            EmbeddingModelDefaultSettings::new().with_header("X-Default-Header", "default"),
            call_options,
        );

        assert_eq!(
            params.headers,
            Some(BTreeMap::from([(
                "X-Default-Header".to_string(),
                "default".to_string()
            )]))
        );
    }

    #[test]
    fn default_embedding_settings_middleware_should_handle_both_headers_being_undefined() {
        let params = transform_default_embedding_settings(
            EmbeddingModelDefaultSettings::new(),
            EmbeddingModelCallOptions::new(vec!["hello world".to_string()]),
        );

        assert_eq!(params.headers, None);
    }

    #[test]
    fn default_embedding_settings_middleware_should_handle_empty_default_provider_options() {
        let params = transform_default_embedding_settings(
            EmbeddingModelDefaultSettings::new()
                .with_provider_options(embedding_provider_options(json!({}))),
            EmbeddingModelCallOptions::new(vec!["hello world".to_string()]).with_provider_options(
                embedding_provider_options(json!({ "openai": { "user": "param-user" } })),
            ),
        );

        assert_eq!(
            serde_json::to_value(params.provider_options).expect("provider options serialize"),
            json!({ "openai": { "user": "param-user" } })
        );
    }

    #[test]
    fn default_embedding_settings_middleware_should_handle_empty_param_provider_options() {
        let params = transform_default_embedding_settings(
            EmbeddingModelDefaultSettings::new().with_provider_options(embedding_provider_options(
                json!({ "anthropic": { "user": "default-user" } }),
            )),
            EmbeddingModelCallOptions::new(vec!["hello world".to_string()])
                .with_provider_options(embedding_provider_options(json!({}))),
        );

        assert_eq!(
            serde_json::to_value(params.provider_options).expect("provider options serialize"),
            json!({ "anthropic": { "user": "default-user" } })
        );
    }

    #[test]
    fn default_embedding_settings_middleware_should_handle_both_provider_options_being_undefined() {
        let params = transform_default_embedding_settings(
            EmbeddingModelDefaultSettings::new(),
            EmbeddingModelCallOptions::new(vec!["hello world".to_string()]),
        );

        assert_eq!(params.provider_options, None);
    }
}
