use std::future::Future;
use std::pin::Pin;

use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult};
use crate::provider::SpecificationVersion;

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

#[cfg(test)]
mod tests {
    use super::{
        EmbeddingModelMiddleware, EmbeddingModelMiddlewareModelOptions,
        EmbeddingModelTransformParamsOptions, EmbeddingModelWrapEmbedOptions,
    };
    use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult};
    use crate::provider::SpecificationVersion;
    use crate::warning::Warning;
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
}
