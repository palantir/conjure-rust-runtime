// Copyright 2020 Palantir Technologies, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
use crate::body::ResetTrackingBody;
use crate::errors::{ThrottledError, UnavailableError};
use crate::payload::{BodyError, HyperBody};
use crate::{Body, Builder, Idempotency};
use conjure_error::{Error, ErrorKind};
use futures::future::{self, BoxFuture};
use http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use http::{HeaderValue, Request};
use rand::Rng;
use std::error;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time;
use tower::layer::Layer;
use tower::Service;
use witchcraft_log::info;

#[derive(Copy, Clone)]
pub struct RetryConfig {
    pub idempotent: bool,
}

/// A layer which retries failed requests in certain cases.
///
/// The `RetryLayer` wraps another layer which is used to create a new service for each request that handles all of that
/// request's attempts. The service passed to the layer must be cloneable as well.
///
/// A request is retried if it fails with a service error with a cause of `ThrottledError`, `UnavailableError`, or
/// `hyper::Error`. Only idempotent requests can be retried which is configured by the `Idempotency` enum, and can be
/// overridden on a request-by-request basis by passing the `RetryConfig` type in the request's extensions map.
///
/// The layer retries up to a specified maximum number of times, applying exponential backoff with full jitter in
/// between each attempt.
///
/// Due to https://github.com/rust-lang/rust/issues/71462, this layer also converts the Conjure `Body` into an
/// `http_body::Body` and sets the `Content-Length` and `Content-Type` headers based off the `Body` implementation, but
/// this should be factored out to a separate layer when the compiler bug has been fixed.
pub struct RetryLayer<L> {
    layer: Arc<L>,
    idempotency: Idempotency,
    max_num_retries: u32,
    backoff_slot_size: Duration,
}

impl<L> RetryLayer<L> {
    pub fn new(layer: Arc<L>, builder: &Builder) -> RetryLayer<L> {
        RetryLayer {
            layer,
            idempotency: builder.idempotency,
            max_num_retries: builder.max_num_retries,
            backoff_slot_size: builder.backoff_slot_size,
        }
    }
}

impl<L, S> Layer<S> for RetryLayer<L>
where
    L: Layer<S>,
    S: Clone,
{
    type Service = RetryService<L, S>;

    fn layer(&self, base: S) -> Self::Service {
        RetryService {
            layer: self.layer.clone(),
            base,
            inner: None,
            idempotency: self.idempotency,
            max_num_retries: self.max_num_retries,
            backoff_slot_size: self.backoff_slot_size,
        }
    }
}

type RetryBody<'a> = Option<Pin<Box<ResetTrackingBody<dyn Body + 'a + Sync + Send>>>>;
type RetryBodyRef<'a, 'b> = Option<Pin<&'a mut ResetTrackingBody<dyn Body + 'b + Sync + Send>>>;

pub struct RetryService<L, S>
where
    L: Layer<S>,
{
    layer: Arc<L>,
    base: S,
    inner: Option<L::Service>,
    idempotency: Idempotency,
    max_num_retries: u32,
    backoff_slot_size: Duration,
}

impl<'a, L, S> Service<Request<RetryBody<'a>>> for RetryService<L, S>
where
    L: Layer<S>,
    S: Clone,
    <L as Layer<S>>::Service: Service<Request<HyperBody>, Error = Error> + 'a + Send,
    <<L as Layer<S>>::Service as Service<Request<HyperBody>>>::Response: Send,
    <<L as Layer<S>>::Service as Service<Request<HyperBody>>>::Future: Send,
{
    type Response = <<L as Layer<S>>::Service as Service<Request<HyperBody>>>::Response;
    type Error = Error;
    type Future = BoxFuture<'a, Result<Self::Response, Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let layer = &self.layer;
        let base = &self.base;
        self.inner
            .get_or_insert_with(|| layer.layer(base.clone()))
            .poll_ready(cx)
    }

    fn call(&mut self, req: Request<RetryBody<'a>>) -> Self::Future {
        let idempotent = match req.extensions().get::<RetryConfig>() {
            Some(config) => config.idempotent,
            None => match self.idempotency {
                Idempotency::Always => true,
                Idempotency::ByMethod => req.method().is_idempotent(),
                Idempotency::Never => false,
            },
        };

        let state = State {
            inner: self.inner.take().expect("call invoked before poll_ready"),
            idempotent,
            max_num_retries: self.max_num_retries,
            backoff_slot_size: self.backoff_slot_size,
            attempt: 0,
        };

        Box::pin(state.call(req))
    }
}

struct State<S> {
    inner: S,
    idempotent: bool,
    max_num_retries: u32,
    backoff_slot_size: Duration,
    attempt: u32,
}

impl<S> State<S>
where
    S: Service<Request<HyperBody>, Error = Error>,
{
    async fn call(mut self, mut req: Request<RetryBody<'_>>) -> Result<S::Response, Error> {
        loop {
            let span = zipkin::next_span()
                .with_name(&format!("conjure-runtime: attempt {}", self.attempt))
                .detach();
            let attempt_req = self.clone_request(&mut req);
            let future = self.send_attempt(attempt_req);
            let (error, retry_after) = match span.bind(future).await? {
                AttemptOutcome::Ok(response) => return Ok(response),
                AttemptOutcome::Retry { error, retry_after } => (error, retry_after),
            };

            self.prepare_for_retry(
                req.body_mut().as_mut().map(|b| b.as_mut()),
                error,
                retry_after,
            )
            .await?;
        }
    }

    // Request.extensions isn't clone, so we need to handle this manually
    fn clone_request<'a, 'b>(
        &self,
        req: &'a mut Request<RetryBody<'b>>,
    ) -> Request<RetryBodyRef<'a, 'b>> {
        let mut new_req = Request::new(());

        *new_req.method_mut() = req.method().clone();
        *new_req.uri_mut() = req.uri().clone();
        *new_req.headers_mut() = req.headers().clone();

        let parts = new_req.into_parts().0;
        Request::from_parts(parts, req.body_mut().as_mut().map(|r| r.as_mut()))
    }

    async fn send_attempt(
        &mut self,
        req: Request<RetryBodyRef<'_, '_>>,
    ) -> Result<AttemptOutcome<S::Response>, Error> {
        match self.send_raw(req).await {
            Ok(response) => Ok(AttemptOutcome::Ok(response)),
            Err(error) => {
                // we don't want to retry after propagated QoS errors
                match error.kind() {
                    ErrorKind::Service(_) => {}
                    _ => return Err(error),
                }

                if let Some(throttled) = error.cause().downcast_ref::<ThrottledError>() {
                    Ok(AttemptOutcome::Retry {
                        retry_after: throttled.retry_after,
                        error,
                    })
                } else if error.cause().is::<UnavailableError>()
                    || error.cause().is::<hyper::Error>()
                {
                    Ok(AttemptOutcome::Retry {
                        error,
                        retry_after: None,
                    })
                } else {
                    Err(error)
                }
            }
        }
    }

    // As noted above, this should be a separate layer!
    async fn send_raw(&mut self, req: Request<RetryBodyRef<'_, '_>>) -> Result<S::Response, Error> {
        future::poll_fn(|cx| self.inner.poll_ready(cx)).await?;

        let (mut parts, body) = req.into_parts();
        if let Some(body) = &body {
            if let Some(length) = body.content_length() {
                parts
                    .headers
                    .insert(CONTENT_LENGTH, HeaderValue::from(length));
            }
            parts.headers.insert(CONTENT_TYPE, body.content_type());
        }
        let (body, writer) = HyperBody::new(body);
        let req = Request::from_parts(parts, body);

        let (body_result, response_result) =
            future::join(writer.write(), self.inner.call(req)).await;

        match (body_result, response_result) {
            (Ok(()), Ok(response)) => Ok(response),
            (Ok(()), Err(e)) => Err(e),
            (Err(e), Ok(response)) => {
                info!(
                    "body write reported an error on a successful request",
                    error: e,
                );
                Ok(response)
            }
            (Err(body), Err(hyper)) => Err(self.deconflict_errors(body, hyper)),
        }
    }

    // An error in the body write will cause an error on the hyper side, and vice versa. To pick the right one, we see
    // if the hyper error was due to the body write aborting or not.
    fn deconflict_errors(&self, body_error: Error, hyper_error: Error) -> Error {
        let mut cause: &(dyn error::Error + 'static) = hyper_error.cause();
        loop {
            if cause.is::<BodyError>() {
                return body_error;
            }
            cause = match cause.source() {
                Some(cause) => cause,
                None => return hyper_error,
            };
        }
    }

    async fn prepare_for_retry(
        &mut self,
        body: RetryBodyRef<'_, '_>,
        error: Error,
        retry_after: Option<Duration>,
    ) -> Result<(), Error> {
        self.attempt += 1;
        if self.attempt >= self.max_num_retries {
            info!("exceeded retry limits");
            return Err(error);
        }

        if !self.idempotent {
            info!("unable to retry non-idempotent request");
            return Err(error);
        }

        if let Some(body) = body {
            let needs_reset = body.needs_reset();
            if needs_reset && !body.reset().await {
                info!("unable to reset body when retrying request");
                return Err(error);
            }
        }

        let backoff = match retry_after {
            Some(backoff) => backoff,
            None => {
                let scale = 1 << self.attempt;
                let max = self.backoff_slot_size * scale;
                // gen_range panics when min == max
                if max == Duration::from_secs(0) {
                    Duration::from_secs(0)
                } else {
                    rand::thread_rng().gen_range(Duration::from_secs(0), max)
                }
            }
        };

        let _span = zipkin::next_span()
            .with_name("conjure-runtime: backoff-with-jitter")
            .detach();

        time::delay_for(backoff).await;

        Ok(())
    }
}

enum AttemptOutcome<R> {
    Ok(R),
    Retry {
        error: Error,
        retry_after: Option<Duration>,
    },
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::body::BytesBody;
    use crate::payload::BodyWriter;
    use async_trait::async_trait;
    use http_body::Body as _;
    use tokio::io::AsyncWriteExt;
    use tower::layer::Identity;
    use tower::ServiceExt;

    #[tokio::test]
    async fn no_body() {
        let service = RetryLayer::new(Arc::new(Identity::new()), &Builder::new()).layer(
            tower::service_fn(|req: Request<_>| async move {
                assert_eq!(req.headers().get(CONTENT_LENGTH), None);
                assert_eq!(req.headers().get(CONTENT_TYPE), None);

                match req.body() {
                    HyperBody::Empty => {}
                    _ => panic!("expected empty body"),
                }

                Ok(())
            }),
        );

        let request = Request::new(None);
        service.oneshot(request).await.unwrap();
    }

    #[tokio::test]
    async fn fixed_size_body() {
        let body = "hello world";

        let service = RetryLayer::new(Arc::new(Identity::new()), &Builder::new()).layer(
            tower::service_fn(|req: Request<_>| async move {
                assert_eq!(
                    req.headers().get(CONTENT_LENGTH).unwrap(),
                    &*body.len().to_string()
                );
                assert_eq!(req.headers().get(CONTENT_TYPE).unwrap(), "text/plain");

                match req.body() {
                    HyperBody::Single(chunk) => assert_eq!(chunk, body),
                    _ => panic!("expected single chunk body"),
                }

                Ok(())
            }),
        );

        let request = Request::new(Some(Box::pin(ResetTrackingBody::new(BytesBody::new(
            body,
            HeaderValue::from_static("text/plain"),
        ))) as _));
        service.oneshot(request).await.unwrap();
    }

    struct StreamedBody;

    #[async_trait]
    impl Body for StreamedBody {
        fn content_length(&self) -> Option<u64> {
            None
        }

        fn content_type(&self) -> HeaderValue {
            HeaderValue::from_static("text/plain")
        }

        async fn write(self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
            w.write_all(b"hello ").await.unwrap();
            w.flush().await.unwrap();
            w.write_all(b"world").await.unwrap();
            Ok(())
        }

        async fn reset(self: Pin<&mut Self>) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn streamed_body() {
        let service = RetryLayer::new(Arc::new(Identity::new()), &Builder::new()).layer(
            tower::service_fn(|req: Request<_>| async move {
                assert_eq!(req.headers().get(CONTENT_LENGTH), None);
                assert_eq!(req.headers().get(CONTENT_TYPE).unwrap(), "text/plain");

                match req.body() {
                    HyperBody::Stream { .. } => {}
                    _ => panic!("expected streaming body"),
                }
                let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                assert_eq!(body, "hello world");

                Ok(())
            }),
        );

        let request = Request::new(Some(Box::pin(ResetTrackingBody::new(StreamedBody)) as _));
        service.oneshot(request).await.unwrap();
    }

    struct StreamedInfiniteBody;

    #[async_trait]
    impl Body for StreamedInfiniteBody {
        fn content_length(&self) -> Option<u64> {
            None
        }

        fn content_type(&self) -> HeaderValue {
            HeaderValue::from_static("text/plain")
        }

        async fn write(self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
            loop {
                w.write_all(b"hello").await.map_err(Error::internal_safe)?;
                w.flush().await.map_err(Error::internal_safe)?;
            }
        }

        async fn reset(self: Pin<&mut Self>) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn streamed_body_hangup() {
        let service = RetryLayer::new(Arc::new(Identity::new()), &Builder::new()).layer(
            tower::service_fn(|req: Request<HyperBody>| async move {
                req.into_body().data().await.unwrap().unwrap();

                Err::<(), _>(Error::internal_safe("blammo"))
            }),
        );

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(StreamedInfiniteBody)) as _
        ));
        let err = service.oneshot(request).await.err().unwrap();

        assert_eq!(err.cause().to_string(), "blammo");
    }

    struct StreamedErrorBody;

    #[async_trait]
    impl Body for StreamedErrorBody {
        fn content_length(&self) -> Option<u64> {
            None
        }

        fn content_type(&self) -> HeaderValue {
            HeaderValue::from_static("text/plain")
        }

        async fn write(self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
            w.write_all(b"hello ").await.unwrap();
            w.flush().await.unwrap();
            Err(Error::internal_safe("uh oh"))
        }

        async fn reset(self: Pin<&mut Self>) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn streamed_body_error() {
        let service = RetryLayer::new(Arc::new(Identity::new()), &Builder::new()).layer(
            tower::service_fn(|req: Request<HyperBody>| async move {
                hyper::body::to_bytes(req.into_body())
                    .await
                    .map_err(Error::internal_safe)?;

                Ok(())
            }),
        );

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(StreamedErrorBody)) as _
        ));
        let err = service.oneshot(request).await.err().unwrap();

        assert_eq!(err.cause().to_string(), "uh oh");
    }

    struct RetryingBody {
        retries: u32,
        needs_reset: bool,
    }

    impl RetryingBody {
        fn new(retries: u32) -> RetryingBody {
            RetryingBody {
                retries,
                needs_reset: false,
            }
        }
    }

    #[async_trait]
    impl Body for RetryingBody {
        fn content_length(&self) -> Option<u64> {
            None
        }

        fn content_type(&self) -> HeaderValue {
            HeaderValue::from_static("text/plain")
        }

        async fn write(mut self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
            assert!(!self.needs_reset);
            self.needs_reset = true;

            w.write_all(b"hello ").await.unwrap();
            w.flush().await.unwrap();
            w.write_all(b"world").await.unwrap();

            Ok(())
        }

        async fn reset(mut self: Pin<&mut Self>) -> bool {
            assert!(self.needs_reset);
            assert!(self.retries > 0);
            self.needs_reset = false;
            self.retries -= 1;

            true
        }
    }

    #[tokio::test]
    async fn retry_after_unavailable() {
        let mut attempt = 0;
        let service = RetryLayer::new(
            Arc::new(Identity::new()),
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(tower::service_fn(move |req: Request<HyperBody>| {
            attempt += 1;
            async move {
                let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                assert_eq!(body, "hello world");

                match attempt {
                    1 => Err(Error::internal_safe(UnavailableError(()))),
                    2 => Ok(()),
                    _ => panic!(),
                }
            }
        }));

        let request = Request::builder()
            .extension(RetryConfig { idempotent: true })
            .body(Some(
                Box::pin(ResetTrackingBody::new(RetryingBody::new(1))) as _
            ))
            .unwrap();
        service.oneshot(request).await.unwrap();
    }

    #[tokio::test]
    async fn retry_after_throttled() {
        let mut attempt = 0;
        let service = RetryLayer::new(
            Arc::new(Identity::new()),
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(tower::service_fn(move |req: Request<HyperBody>| {
            attempt += 1;
            async move {
                let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                assert_eq!(body, "hello world");

                match attempt {
                    1 => Err(Error::internal_safe(ThrottledError { retry_after: None })),
                    2 => Ok(()),
                    _ => panic!(),
                }
            }
        }));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(RetryingBody::new(1))) as _
        ));
        service.oneshot(request).await.unwrap();
    }

    #[tokio::test]
    async fn no_retry_after_propagated_unavailable() {
        let mut attempt = 0;
        let service = RetryLayer::new(
            Arc::new(Identity::new()),
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(tower::service_fn(move |req: Request<HyperBody>| {
            attempt += 1;
            async move {
                let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                assert_eq!(body, "hello world");

                match attempt {
                    1 => Err(Error::throttle_safe(UnavailableError(()))),
                    2 => Ok(()),
                    _ => panic!(),
                }
            }
        }));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(RetryingBody::new(0))) as _
        ));
        service.oneshot(request).await.err().unwrap();
    }

    #[tokio::test]
    async fn no_retry_after_propagated_throttled() {
        let mut attempt = 0;
        let service = RetryLayer::new(
            Arc::new(Identity::new()),
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(tower::service_fn(move |req: Request<HyperBody>| {
            attempt += 1;
            async move {
                let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                assert_eq!(body, "hello world");

                match attempt {
                    1 => Err(Error::throttle_safe(ThrottledError { retry_after: None })),
                    2 => Ok(()),
                    _ => panic!(),
                }
            }
        }));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(RetryingBody::new(0))) as _
        ));
        service.oneshot(request).await.err().unwrap();
    }

    #[tokio::test]
    async fn no_retry_non_idempotent() {
        let mut attempt = 0;
        let service = RetryLayer::new(
            Arc::new(Identity::new()),
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(tower::service_fn(move |_| {
            attempt += 1;
            async move {
                match attempt {
                    1 => Err(Error::internal_safe(UnavailableError(()))),
                    2 => Ok(()),
                    _ => panic!(),
                }
            }
        }));

        let request = Request::builder()
            .extension(RetryConfig { idempotent: false })
            .body(None)
            .unwrap();
        let err = service.oneshot(request).await.err().unwrap();
        assert!(err.cause().is::<UnavailableError>());
    }

    #[tokio::test]
    async fn no_reset_unread_body() {
        let mut attempt = 0;
        let service = RetryLayer::new(
            Arc::new(Identity::new()),
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(tower::service_fn(move |req: Request<HyperBody>| {
            attempt += 1;
            async move {
                match attempt {
                    1 => Err(Error::internal_safe(UnavailableError(()))),
                    2 => {
                        let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                        assert_eq!(body, "hello world");

                        Ok(())
                    }
                    _ => panic!(),
                }
            }
        }));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(RetryingBody::new(0))) as _
        ));
        service.oneshot(request).await.unwrap();
    }

    #[tokio::test]
    async fn give_up_after_limit() {
        let mut attempt = 0;
        let service = RetryLayer::new(
            Arc::new(Identity::new()),
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(tower::service_fn(move |req: Request<HyperBody>| {
            attempt += 1;
            async move {
                let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                assert_eq!(body, "hello world");

                match attempt {
                    1 | 2 => Err::<(), _>(Error::internal_safe(UnavailableError(()))),
                    _ => panic!(),
                }
            }
        }));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(RetryingBody::new(1))) as _
        ));
        service.oneshot(request).await.err().unwrap();
    }
}
