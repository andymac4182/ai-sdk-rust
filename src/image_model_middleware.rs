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

#[cfg(test)]
mod tests {
    use super::{
        ImageModelMiddleware, ImageModelMiddlewareModelOptions, ImageModelTransformParamsOptions,
        ImageModelWrapGenerateOptions,
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
    fn image_model_middleware_hooks_are_optional_by_default() {
        struct NoopMiddleware;

        impl ImageModelMiddleware<StaticImageModel> for NoopMiddleware {
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
                = Ready<ImageModelResult>
            where
                Self: 'a,
                StaticImageModel: 'a;
        }

        let model = StaticImageModel;
        let middleware = NoopMiddleware;

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
