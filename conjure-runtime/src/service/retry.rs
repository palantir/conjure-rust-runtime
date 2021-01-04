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
use crate::errors::{RemoteError, ThrottledError, UnavailableError};
use crate::raw::Service;
use crate::raw::{BodyError, RawBody};
use crate::rng::ConjureRng;
use crate::service::map_error::RawClientError;
use crate::service::request::Pattern;
use crate::service::Layer;
use crate::{Body, Builder, Idempotency};
use conjure_error::{Error, ErrorKind};
use futures::future::{self, BoxFuture};
use http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use http::{HeaderValue, Request, StatusCode};
use rand::Rng;
use std::error;
use std::pin::Pin;
use std::sync::Arc;
use tokio::time::{self, Duration};
use witchcraft_log::info;

#[derive(Copy, Clone)]
pub struct RetryConfig {
    pub idempotent: bool,
}

/// A layer which retries failed requests in certain cases.
///
/// All requests that fail with a cause of `ThrottledError` or `UnavailableError` will be retried. Requests failing with
/// a cause of `RawClientError` or `RemoteError` and a status code of `INTERNAL_SERVER_ERROR` will be retried if the
/// request is considered idempotent. This is configured by the `Idempotency` enum, and can be overridden on a
/// request-by-request basis by passing the `RetryConfig` type in the request's extensions map.
///
/// The layer retries up to a specified maximum number of times, applying exponential backoff with full jitter in
/// between each attempt.
///
/// Due to https://github.com/rust-lang/rust/issues/71462, this layer also converts the Conjure `Body` into an
/// `http_body::Body` and sets the `Content-Length` and `Content-Type` headers based off the `Body` implementation, but
/// this should be factored out to a separate layer when the compiler bug has been fixed.
pub struct RetryLayer {
    idempotency: Idempotency,
    max_num_retries: u32,
    backoff_slot_size: Duration,
    rng: Arc<ConjureRng>,
}

impl RetryLayer {
    pub fn new<T>(builder: &Builder<T>) -> RetryLayer {
        RetryLayer {
            idempotency: builder.get_idempotency(),
            max_num_retries: if builder.mesh_mode() {
                0
            } else {
                builder.get_max_num_retries()
            },
            backoff_slot_size: builder.get_backoff_slot_size(),
            rng: Arc::new(ConjureRng::new(builder)),
        }
    }
}

impl<S> Layer<S> for RetryLayer {
    type Service = RetryService<S>;

    fn layer(self, inner: S) -> Self::Service {
        RetryService {
            inner: Arc::new(inner),
            idempotency: self.idempotency,
            max_num_retries: self.max_num_retries,
            backoff_slot_size: self.backoff_slot_size,
            rng: self.rng,
        }
    }
}

type RetryBody<'a> = Option<Pin<Box<ResetTrackingBody<dyn Body + 'a + Sync + Send>>>>;
type RetryBodyRef<'a, 'b> = Option<Pin<&'a mut ResetTrackingBody<dyn Body + 'b + Sync + Send>>>;

pub struct RetryService<S> {
    inner: Arc<S>,
    idempotency: Idempotency,
    max_num_retries: u32,
    backoff_slot_size: Duration,
    rng: Arc<ConjureRng>,
}

impl<'a, S> Service<Request<RetryBody<'a>>> for RetryService<S>
where
    S: Service<Request<RawBody>, Error = Error> + 'a + Sync + Send,
    S::Response: Send,
    S::Future: Send,
{
    type Response = S::Response;
    type Error = Error;
    type Future = BoxFuture<'a, Result<Self::Response, Error>>;

    fn call(&self, req: Request<RetryBody<'a>>) -> Self::Future {
        let idempotent = match req.extensions().get::<RetryConfig>() {
            Some(config) => config.idempotent,
            None => match self.idempotency {
                Idempotency::Always => true,
                Idempotency::ByMethod => req.method().is_idempotent(),
                Idempotency::Never => false,
            },
        };

        let state = State {
            inner: self.inner.clone(),
            idempotent,
            max_num_retries: self.max_num_retries,
            backoff_slot_size: self.backoff_slot_size,
            rng: self.rng.clone(),
            attempt: 0,
        };

        Box::pin(state.call(req))
    }
}

struct State<S> {
    inner: Arc<S>,
    idempotent: bool,
    max_num_retries: u32,
    backoff_slot_size: Duration,
    rng: Arc<ConjureRng>,
    attempt: u32,
}

impl<S> State<S>
where
    S: Service<Request<RawBody>, Error = Error>,
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

        if let Some(pattern) = req.extensions().get::<Pattern>() {
            new_req.extensions_mut().insert(*pattern);
        }

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

                #[allow(clippy::if_same_then_else)] // conditionals get too crazy if combined
                if let Some(throttled) = error.cause().downcast_ref::<ThrottledError>() {
                    Ok(AttemptOutcome::Retry {
                        retry_after: throttled.retry_after,
                        error,
                    })
                } else if error.cause().is::<UnavailableError>() {
                    Ok(AttemptOutcome::Retry {
                        error,
                        retry_after: None,
                    })
                } else if self.idempotent && error.cause().is::<RawClientError>() {
                    Ok(AttemptOutcome::Retry {
                        error,
                        retry_after: None,
                    })
                } else if self.idempotent
                    && error
                        .cause()
                        .downcast_ref::<RemoteError>()
                        .map_or(false, |e| *e.status() == StatusCode::INTERNAL_SERVER_ERROR)
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
        let (mut parts, body) = req.into_parts();
        if let Some(body) = &body {
            if let Some(length) = body.content_length() {
                parts
                    .headers
                    .insert(CONTENT_LENGTH, HeaderValue::from(length));
            }
            parts.headers.insert(CONTENT_TYPE, body.content_type());
        }
        let (body, writer) = RawBody::new(body);
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
        if self.attempt > self.max_num_retries {
            info!("exceeded retry limits");
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
                    self.rng
                        .with(|rng| rng.gen_range(Duration::from_secs(0)..max))
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
    use crate::raw::RawBodyInner;
    use crate::service;
    use crate::{BodyWriter, BytesBody};
    use async_trait::async_trait;
    use futures::pin_mut;
    use http_body::Body as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn no_body() {
        let service = RetryLayer::new(&Builder::new()).layer(service::service_fn(
            |req: Request<RawBody>| async move {
                assert_eq!(req.headers().get(CONTENT_LENGTH), None);
                assert_eq!(req.headers().get(CONTENT_TYPE), None);

                match req.body().inner {
                    RawBodyInner::Empty => {}
                    _ => panic!("expected empty body"),
                }

                Ok(())
            },
        ));

        let request = Request::new(None);
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn fixed_size_body() {
        let body = "hello world";

        let service = RetryLayer::new(&Builder::new()).layer(service::service_fn(
            |req: Request<RawBody>| async move {
                assert_eq!(
                    req.headers().get(CONTENT_LENGTH).unwrap(),
                    &*body.len().to_string()
                );
                assert_eq!(req.headers().get(CONTENT_TYPE).unwrap(), "text/plain");

                match &req.body().inner {
                    RawBodyInner::Single(chunk) => assert_eq!(chunk, body),
                    _ => panic!("expected single chunk body"),
                }

                Ok(())
            },
        ));

        let request = Request::new(Some(Box::pin(ResetTrackingBody::new(BytesBody::new(
            body,
            HeaderValue::from_static("text/plain"),
        ))) as _));
        service.call(request).await.unwrap();
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
        let service = RetryLayer::new(&Builder::new()).layer(service::service_fn(
            |req: Request<RawBody>| async move {
                assert_eq!(req.headers().get(CONTENT_LENGTH), None);
                assert_eq!(req.headers().get(CONTENT_TYPE).unwrap(), "text/plain");

                match req.body().inner {
                    RawBodyInner::Stream { .. } => {}
                    _ => panic!("expected streaming body"),
                }
                let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                assert_eq!(body, "hello world");

                Ok(())
            },
        ));

        let request = Request::new(Some(Box::pin(ResetTrackingBody::new(StreamedBody)) as _));
        service.call(request).await.unwrap();
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
        let service = RetryLayer::new(&Builder::new()).layer(service::service_fn(
            |req: Request<RawBody>| async move {
                let body = req.into_body();
                pin_mut!(body);
                body.data().await.unwrap().unwrap();

                Err::<(), _>(Error::internal_safe("blammo"))
            },
        ));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(StreamedInfiniteBody)) as _
        ));
        let err = service.call(request).await.err().unwrap();

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
        let service = RetryLayer::new(&Builder::new()).layer(service::service_fn(
            |req: Request<RawBody>| async move {
                hyper::body::to_bytes(req.into_body())
                    .await
                    .map_err(Error::internal_safe)?;

                Ok(())
            },
        ));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(StreamedErrorBody)) as _
        ));
        let err = service.call(request).await.err().unwrap();

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
    async fn retry_after_raw_client_error() {
        let service = RetryLayer::new(
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                    assert_eq!(body, "hello world");

                    match attempt {
                        0 => Err(Error::internal_safe(RawClientError("blammo".into()))),
                        1 => Ok(()),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(RetryConfig { idempotent: true })
            .body(Some(
                Box::pin(ResetTrackingBody::new(RetryingBody::new(1))) as _
            ))
            .unwrap();
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn retry_after_unavailable() {
        let service = RetryLayer::new(
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                    assert_eq!(body, "hello world");

                    match attempt {
                        0 => Err(Error::internal_safe(UnavailableError(()))),
                        1 => Ok(()),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(RetryConfig { idempotent: true })
            .body(Some(
                Box::pin(ResetTrackingBody::new(RetryingBody::new(1))) as _
            ))
            .unwrap();
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn retry_after_throttled() {
        let service = RetryLayer::new(
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                    assert_eq!(body, "hello world");

                    match attempt {
                        0 => Err(Error::internal_safe(ThrottledError { retry_after: None })),
                        1 => Ok(()),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(RetryingBody::new(1))) as _
        ));
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn no_retry_after_propagated_unavailable() {
        let service = RetryLayer::new(
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                    assert_eq!(body, "hello world");

                    match attempt {
                        0 => Err(Error::throttle_safe(UnavailableError(()))),
                        1 => Ok(()),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(RetryingBody::new(0))) as _
        ));
        service.call(request).await.err().unwrap();
    }

    #[tokio::test]
    async fn no_retry_after_propagated_throttled() {
        let service = RetryLayer::new(
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                    assert_eq!(body, "hello world");

                    match attempt {
                        0 => Err(Error::throttle_safe(ThrottledError { retry_after: None })),
                        1 => Ok(()),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(RetryingBody::new(0))) as _
        ));
        service.call(request).await.err().unwrap();
    }

    #[tokio::test]
    async fn no_retry_non_idempotent() {
        let service = RetryLayer::new(
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |_| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    match attempt {
                        0 => Err(Error::internal_safe("blammo")),
                        1 => Ok(()),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(RetryConfig { idempotent: false })
            .body(None)
            .unwrap();
        let err = service.call(request).await.err().unwrap();
        assert_eq!(err.cause().to_string(), "blammo");
    }

    #[tokio::test]
    async fn retry_non_idempotent_for_qos_errors() {
        let service = RetryLayer::new(
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |_| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    match attempt {
                        0 => Err(Error::internal_safe(UnavailableError(()))),
                        1 => Ok(()),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(RetryConfig { idempotent: false })
            .body(None)
            .unwrap();
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn no_reset_unread_body() {
        let service = RetryLayer::new(
            Builder::new()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    match attempt {
                        0 => Err(Error::internal_safe(UnavailableError(()))),
                        1 => {
                            let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                            assert_eq!(body, "hello world");

                            Ok(())
                        }
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(RetryingBody::new(0))) as _
        ));
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn give_up_after_limit() {
        let service = RetryLayer::new(
            Builder::new()
                .max_num_retries(1)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
                    assert_eq!(body, "hello world");

                    match attempt {
                        0 | 1 => Err::<(), _>(Error::internal_safe(UnavailableError(()))),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::new(Some(
            Box::pin(ResetTrackingBody::new(RetryingBody::new(1))) as _
        ));
        service.call(request).await.err().unwrap();
    }
}
