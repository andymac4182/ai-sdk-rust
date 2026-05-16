use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::language_model::{
    LanguageModel, LanguageModelCallOptions, LanguageModelGenerateResult,
    LanguageModelStreamResult, LanguageModelSupportedUrls,
};
use crate::provider::SpecificationVersion;

/// Original language generation operation passed to middleware wrappers.
pub type LanguageModelDoGenerate<'a> = Box<
    dyn FnOnce() -> Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
        + Send
        + 'a,
>;

/// Original language streaming operation passed to middleware wrappers.
pub type LanguageModelDoStream<'a, S> = Box<
    dyn FnOnce() -> Pin<Box<dyn Future<Output = LanguageModelStreamResult<S>> + Send + 'a>>
        + Send
        + 'a,
>;

/// Language model operation whose parameters are being transformed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LanguageModelMiddlewareCallType {
    /// Non-streaming generation.
    Generate,

    /// Streaming generation.
    Stream,
}

/// Options passed to language middleware hooks that only inspect the model.
#[derive(Debug)]
pub struct LanguageModelMiddlewareModelOptions<'a, M: LanguageModel> {
    /// The language model being wrapped.
    pub model: &'a M,
}

impl<'a, M: LanguageModel> LanguageModelMiddlewareModelOptions<'a, M> {
    /// Creates model-only middleware hook options.
    pub fn new(model: &'a M) -> Self {
        Self { model }
    }
}

/// Options passed to language middleware parameter transforms.
#[derive(Debug)]
pub struct LanguageModelTransformParamsOptions<'a, M: LanguageModel> {
    /// Whether the transformed parameters will be used for generate or stream.
    pub call_type: LanguageModelMiddlewareCallType,

    /// Original language model call options.
    pub params: LanguageModelCallOptions,

    /// The language model being wrapped.
    pub model: &'a M,
}

impl<'a, M: LanguageModel> LanguageModelTransformParamsOptions<'a, M> {
    /// Creates transform-params middleware hook options.
    pub fn new(
        call_type: LanguageModelMiddlewareCallType,
        params: LanguageModelCallOptions,
        model: &'a M,
    ) -> Self {
        Self {
            call_type,
            params,
            model,
        }
    }
}

/// Options passed to language generation middleware wrappers.
pub struct LanguageModelWrapGenerateOptions<'a, M: LanguageModel> {
    /// Original language generation operation.
    pub do_generate: LanguageModelDoGenerate<'a>,

    /// Original language streaming operation.
    pub do_stream: LanguageModelDoStream<'a, M::Stream>,

    /// Language model call options, transformed if a transform hook ran first.
    pub params: LanguageModelCallOptions,

    /// The language model being wrapped.
    pub model: &'a M,
}

impl<'a, M: LanguageModel> LanguageModelWrapGenerateOptions<'a, M> {
    /// Creates wrap-generate middleware hook options.
    pub fn new(
        do_generate: LanguageModelDoGenerate<'a>,
        do_stream: LanguageModelDoStream<'a, M::Stream>,
        params: LanguageModelCallOptions,
        model: &'a M,
    ) -> Self {
        Self {
            do_generate,
            do_stream,
            params,
            model,
        }
    }
}

/// Options passed to language stream middleware wrappers.
pub struct LanguageModelWrapStreamOptions<'a, M: LanguageModel> {
    /// Original language generation operation.
    pub do_generate: LanguageModelDoGenerate<'a>,

    /// Original language streaming operation.
    pub do_stream: LanguageModelDoStream<'a, M::Stream>,

    /// Language model call options, transformed if a transform hook ran first.
    pub params: LanguageModelCallOptions,

    /// The language model being wrapped.
    pub model: &'a M,
}

impl<'a, M: LanguageModel> LanguageModelWrapStreamOptions<'a, M> {
    /// Creates wrap-stream middleware hook options.
    pub fn new(
        do_generate: LanguageModelDoGenerate<'a>,
        do_stream: LanguageModelDoStream<'a, M::Stream>,
        params: LanguageModelCallOptions,
        model: &'a M,
    ) -> Self {
        Self {
            do_generate,
            do_stream,
            params,
            model,
        }
    }
}

/// Middleware for provider-v4 language models.
///
/// Upstream `LanguageModelV4Middleware` exposes optional hooks for overriding
/// identity/supported-url values, transforming call options, and wrapping both
/// `doGenerate` and `doStream`. This Rust trait represents optional hooks as
/// methods that return `None` when the middleware does not handle that step.
pub trait LanguageModelMiddleware<M: LanguageModel> {
    /// Future returned by [`LanguageModelMiddleware::override_supported_urls`].
    type OverrideSupportedUrlsFuture<'a>: Future<Output = LanguageModelSupportedUrls> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`LanguageModelMiddleware::transform_params`].
    type TransformParamsFuture<'a>: Future<Output = LanguageModelCallOptions> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`LanguageModelMiddleware::wrap_generate`].
    type WrapGenerateFuture<'a>: Future<Output = LanguageModelGenerateResult> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`LanguageModelMiddleware::wrap_stream`].
    type WrapStreamFuture<'a>: Future<Output = LanguageModelStreamResult<M::Stream>> + Send + 'a
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
        options: LanguageModelMiddlewareModelOptions<'_, M>,
    ) -> Option<String> {
        let _ = options;
        None
    }

    /// Optionally overrides the provider-specific model id.
    fn override_model_id(
        &self,
        options: LanguageModelMiddlewareModelOptions<'_, M>,
    ) -> Option<String> {
        let _ = options;
        None
    }

    /// Optionally overrides supported URL patterns grouped by media type.
    fn override_supported_urls<'a>(
        &'a self,
        options: LanguageModelMiddlewareModelOptions<'a, M>,
    ) -> Option<Self::OverrideSupportedUrlsFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally transforms call options before invoking the model.
    fn transform_params<'a>(
        &'a self,
        options: LanguageModelTransformParamsOptions<'a, M>,
    ) -> Option<Self::TransformParamsFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally wraps the model's non-streaming generation operation.
    fn wrap_generate<'a>(
        &'a self,
        options: LanguageModelWrapGenerateOptions<'a, M>,
    ) -> Option<Self::WrapGenerateFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally wraps the model's streaming generation operation.
    fn wrap_stream<'a>(
        &'a self,
        options: LanguageModelWrapStreamOptions<'a, M>,
    ) -> Option<Self::WrapStreamFuture<'a>>
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
        LanguageModelMiddleware, LanguageModelMiddlewareCallType,
        LanguageModelMiddlewareModelOptions, LanguageModelTransformParamsOptions,
        LanguageModelWrapGenerateOptions, LanguageModelWrapStreamOptions,
    };
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelStreamPart,
        LanguageModelStreamResult, LanguageModelStreamStart, LanguageModelSupportedUrls,
        LanguageModelText, LanguageModelUsage,
    };
    use crate::provider::SpecificationVersion;
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    struct StaticLanguageModel;

    impl LanguageModel for StaticLanguageModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a;

        type Stream = Vec<LanguageModelStreamPart>;

        type StreamFuture<'a>
            = Ready<LanguageModelStreamResult<Self::Stream>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "base-provider"
        }

        fn model_id(&self) -> &str {
            "language-base"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(BTreeMap::from([(
                "image/*".to_string(),
                vec!["^https://base\\.example/images/".to_string()],
            )]))
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(language_result("generated"))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
            ]))
        }
    }

    struct StaticLanguageMiddleware;

    impl LanguageModelMiddleware<StaticLanguageModel> for StaticLanguageMiddleware {
        type OverrideSupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type TransformParamsFuture<'a>
            = Ready<LanguageModelCallOptions>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type WrapGenerateFuture<'a>
            = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type WrapStreamFuture<'a>
            = Pin<
            Box<
                dyn Future<Output = LanguageModelStreamResult<Vec<LanguageModelStreamPart>>>
                    + Send
                    + 'a,
            >,
        >
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        fn override_provider(
            &self,
            options: LanguageModelMiddlewareModelOptions<'_, StaticLanguageModel>,
        ) -> Option<String> {
            Some(format!("{}-middleware", options.model.provider()))
        }

        fn override_model_id(
            &self,
            options: LanguageModelMiddlewareModelOptions<'_, StaticLanguageModel>,
        ) -> Option<String> {
            Some(format!("{}-wrapped", options.model.model_id()))
        }

        fn override_supported_urls<'a>(
            &'a self,
            options: LanguageModelMiddlewareModelOptions<'a, StaticLanguageModel>,
        ) -> Option<Self::OverrideSupportedUrlsFuture<'a>>
        where
            Self: 'a,
            StaticLanguageModel: 'a,
        {
            assert_eq!(options.model.model_id(), "language-base");
            Some(ready(BTreeMap::from([(
                "application/pdf".to_string(),
                vec!["\\.pdf$".to_string()],
            )])))
        }

        fn transform_params<'a>(
            &'a self,
            mut options: LanguageModelTransformParamsOptions<'a, StaticLanguageModel>,
        ) -> Option<Self::TransformParamsFuture<'a>>
        where
            Self: 'a,
            StaticLanguageModel: 'a,
        {
            assert_eq!(options.model.provider(), "base-provider");

            match options.call_type {
                LanguageModelMiddlewareCallType::Generate => {
                    options.params.max_output_tokens = Some(128);
                }
                LanguageModelMiddlewareCallType::Stream => {
                    options.params.include_raw_chunks = Some(true);
                }
            }

            Some(ready(options.params))
        }

        fn wrap_generate<'a>(
            &'a self,
            options: LanguageModelWrapGenerateOptions<'a, StaticLanguageModel>,
        ) -> Option<Self::WrapGenerateFuture<'a>>
        where
            Self: 'a,
            StaticLanguageModel: 'a,
        {
            assert_eq!(options.params.max_output_tokens, Some(32));
            assert_eq!(options.model.model_id(), "language-base");

            Some(Box::pin(async move {
                let mut result = (options.do_generate)().await;
                result.warnings.push(Warning::Other {
                    message: "wrapped-generate".to_string(),
                });
                result
            }))
        }

        fn wrap_stream<'a>(
            &'a self,
            options: LanguageModelWrapStreamOptions<'a, StaticLanguageModel>,
        ) -> Option<Self::WrapStreamFuture<'a>>
        where
            Self: 'a,
            StaticLanguageModel: 'a,
        {
            assert_eq!(options.params.include_raw_chunks, Some(true));
            assert_eq!(options.model.provider(), "base-provider");

            Some(Box::pin(async move {
                let mut result = (options.do_stream)().await;
                result.stream.push(LanguageModelStreamPart::StreamStart(
                    LanguageModelStreamStart::new(vec![Warning::Other {
                        message: "wrapped-stream".to_string(),
                    }]),
                ));
                result
            }))
        }
    }

    fn language_result(text: &str) -> LanguageModelGenerateResult {
        LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(text))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: None,
            },
            LanguageModelUsage::default(),
        )
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
    fn language_model_middleware_call_type_uses_upstream_literals() {
        assert_eq!(
            serde_json::to_value(LanguageModelMiddlewareCallType::Generate)
                .expect("call type serializes"),
            json!("generate")
        );
        assert_eq!(
            serde_json::from_value::<LanguageModelMiddlewareCallType>(json!("stream"))
                .expect("call type deserializes"),
            LanguageModelMiddlewareCallType::Stream
        );
    }

    #[test]
    fn language_model_middleware_exposes_upstream_v4_hooks() {
        let model = StaticLanguageModel;
        let middleware = StaticLanguageMiddleware;

        assert_eq!(middleware.specification_version(), SpecificationVersion::V4);
        assert_eq!(
            middleware.override_provider(LanguageModelMiddlewareModelOptions::new(&model)),
            Some("base-provider-middleware".to_string())
        );
        assert_eq!(
            middleware.override_model_id(LanguageModelMiddlewareModelOptions::new(&model)),
            Some("language-base-wrapped".to_string())
        );

        let supported_urls = middleware
            .override_supported_urls(LanguageModelMiddlewareModelOptions::new(&model))
            .expect("supported URLs hook is implemented");
        assert_eq!(
            poll_ready(supported_urls),
            BTreeMap::from([("application/pdf".to_string(), vec!["\\.pdf$".to_string()])])
        );

        let transformed_generate = middleware
            .transform_params(LanguageModelTransformParamsOptions::new(
                LanguageModelMiddlewareCallType::Generate,
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("generate transform hook is implemented");
        assert_eq!(
            poll_ready(transformed_generate).max_output_tokens,
            Some(128)
        );

        let transformed_stream = middleware
            .transform_params(LanguageModelTransformParamsOptions::new(
                LanguageModelMiddlewareCallType::Stream,
                LanguageModelCallOptions::new(Vec::new()),
                &model,
            ))
            .expect("stream transform hook is implemented");
        assert_eq!(
            poll_ready(transformed_stream).include_raw_chunks,
            Some(true)
        );

        let wrapped_generate = middleware
            .wrap_generate(LanguageModelWrapGenerateOptions::new(
                Box::new(|| Box::pin(ready(language_result("wrapped")))),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()).with_max_output_tokens(32),
                &model,
            ))
            .expect("wrap-generate hook is implemented");
        let generate_result = poll_boxed(wrapped_generate);

        assert_eq!(
            generate_result.warnings,
            vec![Warning::Other {
                message: "wrapped-generate".to_string(),
            }]
        );

        let wrapped_stream = middleware
            .wrap_stream(LanguageModelWrapStreamOptions::new(
                Box::new(|| Box::pin(ready(language_result("fallback")))),
                Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                LanguageModelCallOptions::new(Vec::new()).with_include_raw_chunks(true),
                &model,
            ))
            .expect("wrap-stream hook is implemented");
        let stream_result = poll_boxed(wrapped_stream);

        assert_eq!(stream_result.stream.len(), 1);
        assert_eq!(
            stream_result.stream,
            vec![LanguageModelStreamPart::StreamStart(
                LanguageModelStreamStart::new(vec![Warning::Other {
                    message: "wrapped-stream".to_string(),
                }],)
            )]
        );
    }

    #[test]
    fn language_model_middleware_hooks_are_optional_by_default() {
        struct NoopMiddleware;

        impl LanguageModelMiddleware<StaticLanguageModel> for NoopMiddleware {
            type OverrideSupportedUrlsFuture<'a>
                = Ready<LanguageModelSupportedUrls>
            where
                Self: 'a,
                StaticLanguageModel: 'a;

            type TransformParamsFuture<'a>
                = Ready<LanguageModelCallOptions>
            where
                Self: 'a,
                StaticLanguageModel: 'a;

            type WrapGenerateFuture<'a>
                = Ready<LanguageModelGenerateResult>
            where
                Self: 'a,
                StaticLanguageModel: 'a;

            type WrapStreamFuture<'a>
                = Ready<LanguageModelStreamResult<Vec<LanguageModelStreamPart>>>
            where
                Self: 'a,
                StaticLanguageModel: 'a;
        }

        let model = StaticLanguageModel;
        let middleware = NoopMiddleware;

        assert_eq!(
            middleware.override_provider(LanguageModelMiddlewareModelOptions::new(&model)),
            None
        );
        assert_eq!(
            middleware.override_model_id(LanguageModelMiddlewareModelOptions::new(&model)),
            None
        );
        assert!(
            middleware
                .override_supported_urls(LanguageModelMiddlewareModelOptions::new(&model))
                .is_none()
        );
        assert!(
            middleware
                .transform_params(LanguageModelTransformParamsOptions::new(
                    LanguageModelMiddlewareCallType::Generate,
                    LanguageModelCallOptions::new(Vec::new()),
                    &model,
                ))
                .is_none()
        );
        assert!(
            middleware
                .wrap_generate(LanguageModelWrapGenerateOptions::new(
                    Box::new(|| Box::pin(ready(language_result("base")))),
                    Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                    LanguageModelCallOptions::new(Vec::new()),
                    &model,
                ))
                .is_none()
        );
        assert!(
            middleware
                .wrap_stream(LanguageModelWrapStreamOptions::new(
                    Box::new(|| Box::pin(ready(language_result("base")))),
                    Box::new(|| Box::pin(ready(LanguageModelStreamResult::new(Vec::new())))),
                    LanguageModelCallOptions::new(Vec::new()),
                    &model,
                ))
                .is_none()
        );
    }
}
