use std::future::Future;
use std::pin::Pin;

use crate::image_model::{ImageModel, ImageModelCallOptions, ImageModelResult};
use crate::provider::SpecificationVersion;

/// Original image generation operation passed to middleware wrappers.
pub type ImageModelDoGenerate<'a> =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>> + Send + 'a>;

/// Options passed to image middleware hooks that only inspect the model.
#[derive(Debug)]
pub struct ImageModelMiddlewareModelOptions<'a, M: ImageModel> {
    /// The image model being wrapped.
    pub model: &'a M,
}

impl<'a, M: ImageModel> ImageModelMiddlewareModelOptions<'a, M> {
    /// Creates model-only middleware hook options.
    pub fn new(model: &'a M) -> Self {
        Self { model }
    }
}

/// Options passed to image middleware parameter transforms.
#[derive(Debug)]
pub struct ImageModelTransformParamsOptions<'a, M: ImageModel> {
    /// Original image generation call options.
    pub params: ImageModelCallOptions,

    /// The image model being wrapped.
    pub model: &'a M,
}

impl<'a, M: ImageModel> ImageModelTransformParamsOptions<'a, M> {
    /// Creates transform-params middleware hook options.
    pub fn new(params: ImageModelCallOptions, model: &'a M) -> Self {
        Self { params, model }
    }
}

/// Options passed to image middleware operation wrappers.
pub struct ImageModelWrapGenerateOptions<'a, M: ImageModel> {
    /// Original image generation operation.
    pub do_generate: ImageModelDoGenerate<'a>,

    /// Image generation call options, transformed if a transform hook ran first.
    pub params: ImageModelCallOptions,

    /// The image model being wrapped.
    pub model: &'a M,
}

impl<'a, M: ImageModel> ImageModelWrapGenerateOptions<'a, M> {
    /// Creates wrap-generate middleware hook options.
    pub fn new(
        do_generate: ImageModelDoGenerate<'a>,
        params: ImageModelCallOptions,
        model: &'a M,
    ) -> Self {
        Self {
            do_generate,
            params,
            model,
        }
    }
}

/// Middleware for provider-v4 image models.
///
/// Upstream `ImageModelV4Middleware` exposes optional hooks for overriding
/// identity/capability values, transforming call options, and wrapping
/// `doGenerate`. This Rust trait represents optional hooks as methods that
/// return `None` when the middleware does not handle that step.
pub trait ImageModelMiddleware<M: ImageModel> {
    /// Future returned by [`ImageModelMiddleware::override_max_images_per_call`].
    type OverrideMaxImagesPerCallFuture<'a>: Future<Output = Option<usize>> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`ImageModelMiddleware::transform_params`].
    type TransformParamsFuture<'a>: Future<Output = ImageModelCallOptions> + Send + 'a
    where
        Self: 'a,
        M: 'a;

    /// Future returned by [`ImageModelMiddleware::wrap_generate`].
    type WrapGenerateFuture<'a>: Future<Output = ImageModelResult> + Send + 'a
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
        options: ImageModelMiddlewareModelOptions<'_, M>,
    ) -> Option<String> {
        let _ = options;
        None
    }

    /// Optionally overrides the provider-specific model id.
    fn override_model_id(
        &self,
        options: ImageModelMiddlewareModelOptions<'_, M>,
    ) -> Option<String> {
        let _ = options;
        None
    }

    /// Optionally overrides the model's maximum images per call capability.
    fn override_max_images_per_call<'a>(
        &'a self,
        options: ImageModelMiddlewareModelOptions<'a, M>,
    ) -> Option<Self::OverrideMaxImagesPerCallFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally transforms call options before invoking the model.
    fn transform_params<'a>(
        &'a self,
        options: ImageModelTransformParamsOptions<'a, M>,
    ) -> Option<Self::TransformParamsFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }

    /// Optionally wraps the model's image generation operation.
    fn wrap_generate<'a>(
        &'a self,
        options: ImageModelWrapGenerateOptions<'a, M>,
    ) -> Option<Self::WrapGenerateFuture<'a>>
    where
        M: 'a,
    {
        let _ = options;
        None
    }
}

/// Image model wrapper that applies one middleware around a provider-v4 model.
///
/// Upstream `wrapImageModel` accepts one or more middlewares. This Rust wrapper
/// models the same behavior for a single middleware without allocating a
/// middleware collection; callers can wrap the returned model again to compose
/// additional middleware.
#[derive(Clone, Debug)]
pub struct WrappedImageModel<M, W> {
    model: M,
    middleware: W,
    provider_id: String,
    model_id: String,
}

impl<M, W> WrappedImageModel<M, W>
where
    M: ImageModel,
    W: ImageModelMiddleware<M>,
{
    /// Creates an image model wrapper using middleware-provided identity
    /// overrides when present.
    pub fn new(model: M, middleware: W) -> Self {
        let provider_id = middleware
            .override_provider(ImageModelMiddlewareModelOptions::new(&model))
            .unwrap_or_else(|| model.provider().to_string());
        let model_id = middleware
            .override_model_id(ImageModelMiddlewareModelOptions::new(&model))
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

    /// Returns the wrapped base image model.
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

/// Wraps an image model with middleware.
pub fn wrap_image_model<M, W>(model: M, middleware: W) -> WrappedImageModel<M, W>
where
    M: ImageModel,
    W: ImageModelMiddleware<M>,
{
    WrappedImageModel::new(model, middleware)
}

impl<M, W> ImageModel for WrappedImageModel<M, W>
where
    M: ImageModel + Sync,
    W: ImageModelMiddleware<M> + Sync,
{
    type MaxImagesPerCallFuture<'a>
        = Pin<Box<dyn Future<Output = Option<usize>> + Send + 'a>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.provider_id
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        Box::pin(async move {
            if let Some(max_images_per_call) = self
                .middleware
                .override_max_images_per_call(ImageModelMiddlewareModelOptions::new(&self.model))
            {
                max_images_per_call.await
            } else {
                self.model.max_images_per_call().await
            }
        })
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(async move {
            let params = if let Some(transform_params) =
                self.middleware
                    .transform_params(ImageModelTransformParamsOptions::new(
                        options.clone(),
                        &self.model,
                    )) {
                transform_params.await
            } else {
                options
            };

            let do_generate_params = params.clone();
            let fallback_params = params.clone();
            let model = &self.model;
            let do_generate: ImageModelDoGenerate<'_> =
                Box::new(move || Box::pin(model.do_generate(do_generate_params)));

            if let Some(wrap_generate) =
                self.middleware
                    .wrap_generate(ImageModelWrapGenerateOptions::new(
                        do_generate,
                        params,
                        &self.model,
                    ))
            {
                wrap_generate.await
            } else {
                self.model.do_generate(fallback_params).await
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ImageModelMiddleware, ImageModelMiddlewareModelOptions, ImageModelTransformParamsOptions,
        ImageModelWrapGenerateOptions, wrap_image_model,
    };
    use crate::file_data::FileDataContent;
    use crate::image_model::{
        ImageModel, ImageModelCallOptions, ImageModelResponse, ImageModelResult,
    };
    use crate::provider::SpecificationVersion;
    use crate::warning::Warning;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    use time::OffsetDateTime;

    struct StaticImageModel;

    impl ImageModel for StaticImageModel {
        type MaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<ImageModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "base-provider"
        }

        fn model_id(&self) -> &str {
            "image-base"
        }

        fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
            ready(Some(4))
        }

        fn do_generate(&self, _options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(image_result("image-base", "iVBORw0KGgo="))
        }
    }

    struct StaticImageMiddleware;

    impl ImageModelMiddleware<StaticImageModel> for StaticImageMiddleware {
        type OverrideMaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            StaticImageModel: 'a;

        type TransformParamsFuture<'a>
            = Ready<ImageModelCallOptions>
        where
            Self: 'a,
            StaticImageModel: 'a;

        type WrapGenerateFuture<'a>
            = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
        where
            Self: 'a,
            StaticImageModel: 'a;

        fn override_provider(
            &self,
            options: ImageModelMiddlewareModelOptions<'_, StaticImageModel>,
        ) -> Option<String> {
            Some(format!("{}-middleware", options.model.provider()))
        }

        fn override_model_id(
            &self,
            options: ImageModelMiddlewareModelOptions<'_, StaticImageModel>,
        ) -> Option<String> {
            Some(format!("{}-wrapped", options.model.model_id()))
        }

        fn override_max_images_per_call<'a>(
            &'a self,
            options: ImageModelMiddlewareModelOptions<'a, StaticImageModel>,
        ) -> Option<Self::OverrideMaxImagesPerCallFuture<'a>>
        where
            Self: 'a,
            StaticImageModel: 'a,
        {
            assert_eq!(options.model.model_id(), "image-base");
            Some(ready(Some(8)))
        }

        fn transform_params<'a>(
            &'a self,
            mut options: ImageModelTransformParamsOptions<'a, StaticImageModel>,
        ) -> Option<Self::TransformParamsFuture<'a>>
        where
            Self: 'a,
            StaticImageModel: 'a,
        {
            assert_eq!(options.model.provider(), "base-provider");
            options.params.prompt = Some("added by middleware".to_string());
            Some(ready(options.params))
        }

        fn wrap_generate<'a>(
            &'a self,
            options: ImageModelWrapGenerateOptions<'a, StaticImageModel>,
        ) -> Option<Self::WrapGenerateFuture<'a>>
        where
            Self: 'a,
            StaticImageModel: 'a,
        {
            assert_eq!(options.params.prompt.as_deref(), Some("input"));

            Some(Box::pin(async move {
                let mut result = (options.do_generate)().await;
                result.warnings.push(Warning::Other {
                    message: "wrapped".to_string(),
                });
                result
            }))
        }
    }

    struct ParamEchoImageModel;

    impl ImageModel for ParamEchoImageModel {
        type MaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<ImageModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "echo-provider"
        }

        fn model_id(&self) -> &str {
            "echo-image"
        }

        fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
            ready(Some(2))
        }

        fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
            let prompt = options.prompt.unwrap_or_default();
            ready(
                image_result("echo-image", "echo-generated")
                    .with_warning(Warning::Other { message: prompt }),
            )
        }
    }

    struct TransformAndWrapImageMiddleware;

    impl ImageModelMiddleware<ParamEchoImageModel> for TransformAndWrapImageMiddleware {
        type OverrideMaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            ParamEchoImageModel: 'a;

        type TransformParamsFuture<'a>
            = Ready<ImageModelCallOptions>
        where
            Self: 'a,
            ParamEchoImageModel: 'a;

        type WrapGenerateFuture<'a>
            = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
        where
            Self: 'a,
            ParamEchoImageModel: 'a;

        fn transform_params<'a>(
            &'a self,
            mut options: ImageModelTransformParamsOptions<'a, ParamEchoImageModel>,
        ) -> Option<Self::TransformParamsFuture<'a>>
        where
            Self: 'a,
            ParamEchoImageModel: 'a,
        {
            options.params.prompt = Some(format!(
                "{} transformed",
                options.params.prompt.as_deref().unwrap_or_default()
            ));
            Some(ready(options.params))
        }

        fn wrap_generate<'a>(
            &'a self,
            options: ImageModelWrapGenerateOptions<'a, ParamEchoImageModel>,
        ) -> Option<Self::WrapGenerateFuture<'a>>
        where
            Self: 'a,
            ParamEchoImageModel: 'a,
        {
            assert_eq!(options.params.prompt.as_deref(), Some("input transformed"));

            Some(Box::pin(async move {
                let mut result = (options.do_generate)().await;
                result.warnings.push(Warning::Other {
                    message: format!("wrapped {}", options.model.model_id()),
                });
                result
            }))
        }
    }

    #[derive(Clone, Copy)]
    struct NoopImageMiddleware;

    impl<M: ImageModel> ImageModelMiddleware<M> for NoopImageMiddleware {
        type OverrideMaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            M: 'a;

        type TransformParamsFuture<'a>
            = Ready<ImageModelCallOptions>
        where
            Self: 'a,
            M: 'a;

        type WrapGenerateFuture<'a>
            = Ready<ImageModelResult>
        where
            Self: 'a,
            M: 'a;
    }

    #[derive(Clone, Copy)]
    struct AppendPromptMiddleware(&'static str);

    impl<M: ImageModel> ImageModelMiddleware<M> for AppendPromptMiddleware {
        type OverrideMaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            M: 'a;

        type TransformParamsFuture<'a>
            = Ready<ImageModelCallOptions>
        where
            Self: 'a,
            M: 'a;

        type WrapGenerateFuture<'a>
            = Ready<ImageModelResult>
        where
            Self: 'a,
            M: 'a;

        fn transform_params<'a>(
            &'a self,
            mut options: ImageModelTransformParamsOptions<'a, M>,
        ) -> Option<Self::TransformParamsFuture<'a>>
        where
            Self: 'a,
            M: 'a,
        {
            let prompt = options.params.prompt.get_or_insert_with(String::new);
            prompt.push_str(self.0);
            Some(ready(options.params))
        }
    }

    #[derive(Clone, Copy)]
    struct AppendImageWarningMiddleware(&'static str);

    impl<M: ImageModel> ImageModelMiddleware<M> for AppendImageWarningMiddleware {
        type OverrideMaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            M: 'a;

        type TransformParamsFuture<'a>
            = Ready<ImageModelCallOptions>
        where
            Self: 'a,
            M: 'a;

        type WrapGenerateFuture<'a>
            = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
        where
            Self: 'a,
            M: 'a;

        fn wrap_generate<'a>(
            &'a self,
            options: ImageModelWrapGenerateOptions<'a, M>,
        ) -> Option<Self::WrapGenerateFuture<'a>>
        where
            Self: 'a,
            M: 'a,
        {
            Some(Box::pin(async move {
                let mut result = (options.do_generate)().await;
                result.warnings.push(Warning::Other {
                    message: self.0.to_string(),
                });
                result
            }))
        }
    }

    struct StatefulMaxImagesModel {
        value: usize,
    }

    impl ImageModel for StatefulMaxImagesModel {
        type MaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<ImageModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "stateful-provider"
        }

        fn model_id(&self) -> &str {
            "stateful-image"
        }

        fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
            ready(Some(self.value))
        }

        fn do_generate(&self, _options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(image_result("stateful-image", "stateful-generated"))
        }
    }

    fn image_result(model_id: &str, image: &str) -> ImageModelResult {
        let response_timestamp = OffsetDateTime::parse(
            "2024-01-02T03:04:05Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("timestamp parses");

        ImageModelResult::new(
            vec![FileDataContent::Base64(image.to_string())],
            ImageModelResponse::new(response_timestamp, model_id),
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
    fn image_model_middleware_exposes_upstream_v4_hooks() {
        let model = StaticImageModel;
        let middleware = StaticImageMiddleware;

        assert_eq!(middleware.specification_version(), SpecificationVersion::V4);
        assert_eq!(
            middleware.override_provider(ImageModelMiddlewareModelOptions::new(&model)),
            Some("base-provider-middleware".to_string())
        );
        assert_eq!(
            middleware.override_model_id(ImageModelMiddlewareModelOptions::new(&model)),
            Some("image-base-wrapped".to_string())
        );

        let max_images = middleware
            .override_max_images_per_call(ImageModelMiddlewareModelOptions::new(&model))
            .expect("max images hook is implemented");
        assert_eq!(poll_ready(max_images), Some(8));

        let transformed = middleware
            .transform_params(ImageModelTransformParamsOptions::new(
                ImageModelCallOptions::new(1),
                &model,
            ))
            .expect("transform hook is implemented");
        assert_eq!(
            poll_ready(transformed).prompt,
            Some("added by middleware".to_string())
        );

        let wrapped = middleware
            .wrap_generate(ImageModelWrapGenerateOptions::new(
                Box::new(|| Box::pin(ready(image_result("image-wrapper", "wrapped-image")))),
                ImageModelCallOptions::new(1).with_prompt("input"),
                &model,
            ))
            .expect("wrap hook is implemented");
        let result = poll_boxed(wrapped);

        assert_eq!(
            result.images,
            vec![FileDataContent::Base64("wrapped-image".to_string())]
        );
        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "wrapped".to_string(),
            }]
        );
    }

    #[test]
    fn wrap_image_model_applies_identity_and_capability_overrides() {
        let wrapped = wrap_image_model(StaticImageModel, StaticImageMiddleware);

        assert_eq!(wrapped.specification_version(), SpecificationVersion::V4);
        assert_eq!(wrapped.provider(), "base-provider-middleware");
        assert_eq!(wrapped.model_id(), "image-base-wrapped");
        assert_eq!(poll_boxed(wrapped.max_images_per_call()), Some(8));

        let explicit = wrap_image_model(StaticImageModel, StaticImageMiddleware)
            .with_provider_id("explicit-provider")
            .with_model_id("explicit-model");

        assert_eq!(explicit.provider(), "explicit-provider");
        assert_eq!(explicit.model_id(), "explicit-model");
    }

    #[test]
    fn wrap_image_model_model_property_should_pass_through_by_default() {
        let wrapped = wrap_image_model(StaticImageModel, NoopImageMiddleware);

        assert_eq!(wrapped.model_id(), "image-base");
    }

    #[test]
    fn wrap_image_model_model_property_should_use_middleware_override_model_id_if_provided() {
        let wrapped = wrap_image_model(StaticImageModel, StaticImageMiddleware);

        assert_eq!(wrapped.model_id(), "image-base-wrapped");
    }

    #[test]
    fn wrap_image_model_model_property_should_use_model_id_parameter_if_provided() {
        let wrapped =
            wrap_image_model(StaticImageModel, NoopImageMiddleware).with_model_id("override-model");

        assert_eq!(wrapped.model_id(), "override-model");
    }

    #[test]
    fn wrap_image_model_provider_property_should_pass_through_by_default() {
        let wrapped = wrap_image_model(StaticImageModel, NoopImageMiddleware);

        assert_eq!(wrapped.provider(), "base-provider");
    }

    #[test]
    fn wrap_image_model_provider_property_should_use_middleware_override_provider_if_provided() {
        let wrapped = wrap_image_model(StaticImageModel, StaticImageMiddleware);

        assert_eq!(wrapped.provider(), "base-provider-middleware");
    }

    #[test]
    fn wrap_image_model_provider_property_should_use_provider_id_parameter_if_provided() {
        let wrapped = wrap_image_model(StaticImageModel, NoopImageMiddleware)
            .with_provider_id("override-provider");

        assert_eq!(wrapped.provider(), "override-provider");
    }

    #[test]
    fn wrap_image_model_max_images_per_call_property_should_pass_through_by_default() {
        let wrapped = wrap_image_model(StaticImageModel, NoopImageMiddleware);

        assert_eq!(poll_boxed(wrapped.max_images_per_call()), Some(4));
    }

    #[test]
    fn wrap_image_model_max_images_per_call_property_should_use_middleware_override_if_provided() {
        let wrapped = wrap_image_model(StaticImageModel, StaticImageMiddleware);

        assert_eq!(poll_boxed(wrapped.max_images_per_call()), Some(8));
    }

    #[test]
    fn wrap_image_model_should_call_transform_params_middleware_for_do_generate() {
        let wrapped = wrap_image_model(ParamEchoImageModel, AppendPromptMiddleware(" transformed"));

        let result =
            poll_boxed(wrapped.do_generate(ImageModelCallOptions::new(1).with_prompt("original")));

        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "original transformed".to_string(),
            }]
        );
    }

    #[test]
    fn wrap_image_model_should_call_wrap_generate_middleware() {
        let wrapped = wrap_image_model(
            ParamEchoImageModel,
            AppendImageWarningMiddleware("wrapped generate"),
        );

        let result =
            poll_boxed(wrapped.do_generate(ImageModelCallOptions::new(1).with_prompt("original")));

        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "original".to_string(),
                },
                Warning::Other {
                    message: "wrapped generate".to_string(),
                },
            ]
        );
    }

    #[test]
    fn wrap_image_model_should_support_models_that_use_context_in_max_images_per_call() {
        let wrapped = wrap_image_model(StatefulMaxImagesModel { value: 42 }, NoopImageMiddleware);

        assert_eq!(poll_boxed(wrapped.max_images_per_call()), Some(42));
    }

    #[test]
    fn wrap_image_model_should_call_multiple_transform_params_middlewares_in_sequence_for_do_generate()
     {
        let first = wrap_image_model(ParamEchoImageModel, AppendPromptMiddleware(" step1"));
        let second = wrap_image_model(first, AppendPromptMiddleware(" step2"));

        let result =
            poll_boxed(second.do_generate(ImageModelCallOptions::new(1).with_prompt("original")));

        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "original step2 step1".to_string(),
            }]
        );
    }

    #[test]
    fn wrap_image_model_should_chain_multiple_wrap_generate_middlewares_in_the_correct_order() {
        let first = wrap_image_model(ParamEchoImageModel, AppendImageWarningMiddleware("wrap1"));
        let second = wrap_image_model(first, AppendImageWarningMiddleware("wrap2"));

        let result =
            poll_boxed(second.do_generate(ImageModelCallOptions::new(1).with_prompt("original")));

        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "original".to_string(),
                },
                Warning::Other {
                    message: "wrap1".to_string(),
                },
                Warning::Other {
                    message: "wrap2".to_string(),
                },
            ]
        );
    }

    #[test]
    fn wrap_image_model_transforms_params_before_wrapping_generate() {
        let wrapped = wrap_image_model(ParamEchoImageModel, TransformAndWrapImageMiddleware);

        let result =
            poll_boxed(wrapped.do_generate(ImageModelCallOptions::new(1).with_prompt("input")));

        assert_eq!(
            result.images,
            vec![FileDataContent::Base64("echo-generated".to_string())]
        );
        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "input transformed".to_string(),
                },
                Warning::Other {
                    message: "wrapped echo-image".to_string(),
                }
            ]
        );
    }

    #[test]
    fn image_model_middleware_hooks_are_optional_by_default() {
        let model = StaticImageModel;
        let middleware = NoopImageMiddleware;

        assert_eq!(
            middleware.override_provider(ImageModelMiddlewareModelOptions::new(&model)),
            None
        );
        assert_eq!(
            middleware.override_model_id(ImageModelMiddlewareModelOptions::new(&model)),
            None
        );
        assert!(
            middleware
                .override_max_images_per_call(ImageModelMiddlewareModelOptions::new(&model))
                .is_none()
        );
        assert!(
            middleware
                .transform_params(ImageModelTransformParamsOptions::new(
                    ImageModelCallOptions::new(1),
                    &model,
                ))
                .is_none()
        );
        assert!(
            middleware
                .wrap_generate(ImageModelWrapGenerateOptions::new(
                    Box::new(|| Box::pin(ready(image_result("image-base", "base-image")))),
                    ImageModelCallOptions::new(1),
                    &model,
                ))
                .is_none()
        );
    }
}
