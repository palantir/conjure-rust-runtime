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
use crate::errors::{RemoteError, ThrottledError, UnavailableError};
use crate::raw::Service;
use crate::raw::{BodyError, RawBody};
use crate::rng::ConjureRng;
use crate::service::map_error::RawClientError;
use crate::service::Layer;
use crate::util::spans::{self, HttpSpanFuture};
use crate::{builder, BodyWriter, Builder, Idempotency};
use conjure_error::{Error, ErrorKind};
use conjure_http::client::{AsyncRequestBody, AsyncWriteBody, BoxAsyncWriteBody, Endpoint};
use futures::future;
use http::request::Parts;
use http::{Request, Response, StatusCode};
use rand::Rng;
use std::error;
use std::future::Future;
use std::pin::Pin;
use tokio::time::{self, Duration};
use witchcraft_log::info;

/// A layer which retries failed requests in certain cases.
///
/// All requests that fail with a cause of `ThrottledError` or `UnavailableError` will be retried. Requests failing with
/// a cause of `RawClientError` or `RemoteError` and a status code of `INTERNAL_SERVER_ERROR` will be retried if the
/// request is considered idempotent. This is configured by the `Idempotency` enum.
///
/// The layer retries up to a specified maximum number of times, applying exponential backoff with full jitter in
/// between each attempt.
///
/// Due to https://github.com/rust-lang/rust/issues/71462, this layer also converts the Conjure `Body` into an
/// `http_body::Body`, but this should be factored out to a separate layer when the compiler bug has been fixed.
pub struct RetryLayer {
    idempotency: Idempotency,
    max_num_retries: u32,
    backoff_slot_size: Duration,
    rng: ConjureRng,
}

impl RetryLayer {
    pub fn new<T>(builder: &Builder<builder::Complete<T>>) -> RetryLayer {
        RetryLayer {
            idempotency: builder.get_idempotency(),
            max_num_retries: if builder.mesh_mode() {
                0
            } else {
                builder.get_max_num_retries()
            },
            backoff_slot_size: builder.get_backoff_slot_size(),
            rng: ConjureRng::new(builder),
        }
    }
}

impl<S> Layer<S> for RetryLayer {
    type Service = RetryService<S>;

    fn layer(self, inner: S) -> Self::Service {
        RetryService {
            inner,
            idempotency: self.idempotency,
            max_num_retries: self.max_num_retries,
            backoff_slot_size: self.backoff_slot_size,
            rng: self.rng,
        }
    }
}

pub struct RetryService<S> {
    inner: S,
    idempotency: Idempotency,
    max_num_retries: u32,
    backoff_slot_size: Duration,
    rng: ConjureRng,
}

impl<'a, S, B> Service<Request<AsyncRequestBody<'a, BodyWriter>>> for RetryService<S>
where
    S: Service<Request<RawBody>, Response = Response<B>, Error = Error> + 'a + Sync + Send,
    S::Response: Send,
    B: 'static,
{
    type Response = S::Response;
    type Error = S::Error;

    fn call(
        &self,
        req: Request<AsyncRequestBody<'a, BodyWriter>>,
    ) -> impl Future<Output = Result<Self::Response, Self::Error>> {
        let idempotent = match self.idempotency {
            Idempotency::Always => true,
            Idempotency::ByMethod => req.method().is_idempotent(),
            Idempotency::Never => false,
        };

        let state = State {
            service: self,
            idempotent,
            attempt: 0,
        };

        state.call(req)
    }
}

struct State<'a, S> {
    service: &'a RetryService<S>,
    idempotent: bool,
    attempt: u32,
}

impl<S, B> State<'_, S>
where
    S: Service<Request<RawBody>, Response = Response<B>, Error = Error>,
{
    async fn call(
        mut self,
        req: Request<AsyncRequestBody<'_, BodyWriter>>,
    ) -> Result<S::Response, Error> {
        let (parts, mut body) = req.into_parts();

        loop {
            let mut tracked = None;
            let body: AsyncRequestBody<'_, BodyWriter> = match &mut body {
                AsyncRequestBody::Empty => AsyncRequestBody::Empty,
                AsyncRequestBody::Fixed(bytes) => AsyncRequestBody::Fixed(bytes.clone()),
                AsyncRequestBody::Streaming(writer) => {
                    let body =
                        BoxAsyncWriteBody::new(tracked.insert(ResetTracker::new(writer)).writer());
                    AsyncRequestBody::Streaming(body)
                }
            };

            let attempt_req = self.clone_request(&parts, body);
            let (error, retry_after) = match self.send_attempt(attempt_req).await? {
                AttemptOutcome::Ok(response) => return Ok(response),
                AttemptOutcome::Retry { error, retry_after } => (error, retry_after),
            };

            self.prepare_for_retry(tracked.as_mut(), error, retry_after)
                .await?;
        }
    }

    // Request.extensions isn't clone, so we need to handle this manually
    fn clone_request<'a>(
        &self,
        parts: &Parts,
        body: AsyncRequestBody<'a, BodyWriter>,
    ) -> Request<AsyncRequestBody<'a, BodyWriter>> {
        let mut new_req = Request::new(());

        *new_req.method_mut() = parts.method.clone();
        *new_req.uri_mut() = parts.uri.clone();
        *new_req.headers_mut() = parts.headers.clone();

        if let Some(endpoint) = parts.extensions.get::<Endpoint>() {
            new_req.extensions_mut().insert(endpoint.clone());
        }

        let parts = new_req.into_parts().0;
        Request::from_parts(parts, body)
    }

    async fn send_attempt(
        &mut self,
        req: Request<AsyncRequestBody<'_, BodyWriter>>,
    ) -> Result<AttemptOutcome<S::Response>, Error> {
        let mut span = zipkin::next_span()
            .with_name("conjure-runtime: attempt")
            .with_tag("failures", &self.attempt.to_string())
            .detach();
        spans::add_request_tags(&mut span, &req);

        match HttpSpanFuture::new(self.send_raw(req), span).await {
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
                        .is_some_and(|e| *e.status() == StatusCode::INTERNAL_SERVER_ERROR)
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
    async fn send_raw(
        &mut self,
        req: Request<AsyncRequestBody<'_, BodyWriter>>,
    ) -> Result<S::Response, Error> {
        let (parts, body) = req.into_parts();
        let (body, writer) = RawBody::new(body);
        let req = Request::from_parts(parts, body);

        let (body_result, response_result) =
            future::join(writer.write(), self.service.inner.call(req)).await;

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
        body: Option<&mut ResetTracker<'_, '_>>,
        error: Error,
        retry_after: Option<Duration>,
    ) -> Result<(), Error> {
        self.attempt += 1;
        if self.attempt > self.service.max_num_retries {
            info!("exceeded retry limits");
            return Err(error);
        }

        if let Some(body) = body {
            let needs_reset = body.needs_reset;
            if needs_reset && !Pin::new(&mut *body.body_writer).reset().await {
                info!("unable to reset body when retrying request");
                return Err(error);
            }
        }

        let backoff = match retry_after {
            Some(backoff) => backoff,
            None => {
                let scale = 1 << (self.attempt - 1);
                let max = self.service.backoff_slot_size * scale;
                // gen_range panics when min == max
                if max == Duration::from_secs(0) {
                    Duration::from_secs(0)
                } else {
                    self.service
                        .rng
                        .with(|rng| rng.random_range(Duration::from_secs(0)..max))
                }
            }
        };

        let _span = zipkin::next_span()
            .with_name("conjure-runtime: backoff-with-jitter")
            .detach();

        time::sleep(backoff).await;

        Ok(())
    }
}

struct ResetTracker<'a, 'b> {
    needs_reset: bool,
    body_writer: &'a mut BoxAsyncWriteBody<'b, BodyWriter>,
}

impl<'a, 'b> ResetTracker<'a, 'b> {
    fn new(body_writer: &'a mut BoxAsyncWriteBody<'b, BodyWriter>) -> Self {
        ResetTracker {
            needs_reset: false,
            body_writer,
        }
    }

    fn writer<'c>(&'c mut self) -> ResetTrackingBodyWriter<'c, 'a, 'b> {
        ResetTrackingBodyWriter { tracker: self }
    }
}

struct ResetTrackingBodyWriter<'a, 'b, 'c> {
    tracker: &'a mut ResetTracker<'b, 'c>,
}

impl AsyncWriteBody<BodyWriter> for ResetTrackingBodyWriter<'_, '_, '_> {
    async fn write_body(mut self: Pin<&mut Self>, w: Pin<&mut BodyWriter>) -> Result<(), Error> {
        self.tracker.needs_reset = true;
        Pin::new(&mut *self.tracker.body_writer).write_body(w).await
    }

    async fn reset(mut self: Pin<&mut Self>) -> bool {
        let ok = Pin::new(&mut *self.tracker.body_writer).reset().await;
        if ok {
            self.tracker.needs_reset = false;
        }
        ok
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
    use crate::BodyWriter;
    use bytes::Bytes;
    use http::Method;
    use http_body_util::BodyExt;
    use std::pin::pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::AsyncWriteExt;

    fn endpoint() -> Endpoint {
        Endpoint::new("service", None, "name", "path")
    }

    #[tokio::test]
    async fn no_body() {
        let service = RetryLayer::new(&Builder::for_test()).layer(service::service_fn(
            |req: Request<RawBody>| async move {
                match req.body().inner {
                    RawBodyInner::Empty => {}
                    _ => panic!("expected empty body"),
                }

                Ok(Response::new(()))
            },
        ));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Empty)
            .unwrap();
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn fixed_size_body() {
        let body = "hello world";

        let service = RetryLayer::new(&Builder::for_test()).layer(service::service_fn(
            |req: Request<RawBody>| async move {
                match &req.body().inner {
                    RawBodyInner::Single(chunk) => assert_eq!(chunk.data_ref().unwrap(), body),
                    _ => panic!("expected single chunk body"),
                }

                Ok(Response::new(()))
            },
        ));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Fixed(Bytes::from(body)))
            .unwrap();
        service.call(request).await.unwrap();
    }

    struct StreamedBody;

    impl AsyncWriteBody<BodyWriter> for StreamedBody {
        async fn write_body(
            self: Pin<&mut Self>,
            mut w: Pin<&mut BodyWriter>,
        ) -> Result<(), Error> {
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
        let service = RetryLayer::new(&Builder::for_test()).layer(service::service_fn(
            |req: Request<RawBody>| async move {
                match req.body().inner {
                    RawBodyInner::Stream { .. } => {}
                    _ => panic!("expected streaming body"),
                }
                let body = req.into_body().collect().await.unwrap();
                assert_eq!(body.to_bytes(), "hello world");

                Ok(Response::new(()))
            },
        ));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                StreamedBody,
            )))
            .unwrap();
        service.call(request).await.unwrap();
    }

    struct StreamedInfiniteBody;

    impl AsyncWriteBody<BodyWriter> for StreamedInfiniteBody {
        async fn write_body(
            self: Pin<&mut Self>,
            mut w: Pin<&mut BodyWriter>,
        ) -> Result<(), Error> {
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
        let service = RetryLayer::new(&Builder::for_test()).layer(service::service_fn(
            |req: Request<RawBody>| async move {
                let mut body = pin!(req.into_body());
                body.frame().await.unwrap().unwrap();

                Err::<Response<()>, _>(Error::internal_safe("blammo"))
            },
        ));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                StreamedInfiniteBody,
            )))
            .unwrap();
        let err = service.call(request).await.err().unwrap();

        assert_eq!(err.cause().to_string(), "blammo");
    }

    struct StreamedErrorBody;

    impl AsyncWriteBody<BodyWriter> for StreamedErrorBody {
        async fn write_body(
            self: Pin<&mut Self>,
            mut w: Pin<&mut BodyWriter>,
        ) -> Result<(), Error> {
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
        let service = RetryLayer::new(&Builder::for_test()).layer(service::service_fn(
            |req: Request<RawBody>| async move {
                req.into_body()
                    .collect()
                    .await
                    .map_err(Error::internal_safe)?;

                Ok(Response::new(()))
            },
        ));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                StreamedErrorBody,
            )))
            .unwrap();
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

    impl AsyncWriteBody<BodyWriter> for RetryingBody {
        async fn write_body(
            mut self: Pin<&mut Self>,
            mut w: Pin<&mut BodyWriter>,
        ) -> Result<(), Error> {
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
            &Builder::for_test()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = req.into_body().collect().await.unwrap();
                    assert_eq!(body.to_bytes(), "hello world");

                    match attempt {
                        0 => Err(Error::internal_safe(RawClientError("blammo".into()))),
                        1 => Ok(Response::new(())),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                RetryingBody::new(1),
            )))
            .unwrap();
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn retry_after_unavailable() {
        let service = RetryLayer::new(
            &Builder::for_test()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = req.into_body().collect().await.unwrap();
                    assert_eq!(body.to_bytes(), "hello world");

                    match attempt {
                        0 => Err(Error::internal_safe(UnavailableError(()))),
                        1 => Ok(Response::new(())),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                RetryingBody::new(1),
            )))
            .unwrap();
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn retry_after_throttled() {
        let service = RetryLayer::new(
            &Builder::for_test()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = req.into_body().collect().await.unwrap();
                    assert_eq!(body.to_bytes(), "hello world");

                    match attempt {
                        0 => Err(Error::internal_safe(ThrottledError { retry_after: None })),
                        1 => Ok(Response::new(())),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                RetryingBody::new(1),
            )))
            .unwrap();
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn no_retry_after_propagated_unavailable() {
        let service = RetryLayer::new(
            &Builder::for_test()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = req.into_body().collect().await.unwrap();
                    assert_eq!(body.to_bytes(), "hello world");

                    match attempt {
                        0 => Err(Error::throttle_safe(UnavailableError(()))),
                        1 => Ok(Response::new(())),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                RetryingBody::new(0),
            )))
            .unwrap();
        service.call(request).await.err().unwrap();
    }

    #[tokio::test]
    async fn no_retry_after_propagated_throttled() {
        let service = RetryLayer::new(
            &Builder::for_test()
                .max_num_retries(2)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = req.into_body().collect().await.unwrap();
                    assert_eq!(body.to_bytes(), "hello world");

                    match attempt {
                        0 => Err(Error::throttle_safe(ThrottledError { retry_after: None })),
                        1 => Ok(Response::new(())),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                RetryingBody::new(0),
            )))
            .unwrap();
        service.call(request).await.err().unwrap();
    }

    #[tokio::test]
    async fn no_retry_non_idempotent() {
        let service = RetryLayer::new(
            &Builder::for_test()
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
                        1 => Ok(Response::new(())),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .method(Method::POST)
            .extension(endpoint())
            .body(AsyncRequestBody::Empty)
            .unwrap();
        let err = service.call(request).await.err().unwrap();
        assert_eq!(err.cause().to_string(), "blammo");
    }

    #[tokio::test]
    async fn retry_non_idempotent_for_qos_errors() {
        let service = RetryLayer::new(
            &Builder::for_test()
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
                        1 => Ok(Response::new(())),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .method(Method::POST)
            .extension(endpoint())
            .body(AsyncRequestBody::Empty)
            .unwrap();
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn no_reset_unread_body() {
        let service = RetryLayer::new(
            &Builder::for_test()
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
                            let body = req.into_body().collect().await.unwrap();
                            assert_eq!(body.to_bytes(), "hello world");

                            Ok(Response::new(()))
                        }
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                RetryingBody::new(0),
            )))
            .unwrap();
        service.call(request).await.unwrap();
    }

    #[tokio::test]
    async fn give_up_after_limit() {
        let service = RetryLayer::new(
            &Builder::for_test()
                .max_num_retries(1)
                .backoff_slot_size(Duration::from_secs(0)),
        )
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<RawBody>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    let body = req.into_body().collect().await.unwrap();
                    assert_eq!(body.to_bytes(), "hello world");

                    match attempt {
                        0 | 1 => Err::<Response<()>, _>(Error::internal_safe(UnavailableError(()))),
                        _ => panic!(),
                    }
                }
            }
        }));

        let request = Request::builder()
            .extension(endpoint())
            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                RetryingBody::new(1),
            )))
            .unwrap();
        service.call(request).await.err().unwrap();
    }
}
