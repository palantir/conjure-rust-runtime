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
use crate::service::http_error::PropagationConfig;
use crate::Body;
use conjure_error::{Error, ErrorKind};
use futures::future::BoxFuture;
use http::Request;
use rand::Rng;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time;
use tower::layer::Layer;
use tower::{Service, ServiceExt};
use witchcraft_log::info;

#[derive(Copy, Clone)]
pub struct RetryConfig {
    pub idempotent: bool,
}

pub trait MakeService {
    type Service;

    fn make_service(&mut self) -> Self::Service;
}

pub struct LayerServiceMaker<L, S> {
    layer: L,
    service: S,
}

impl<L, S> LayerServiceMaker<L, S> {
    pub fn new(layer: L, service: S) -> LayerServiceMaker<L, S> {
        LayerServiceMaker { layer, service }
    }
}

impl<L, S> MakeService for LayerServiceMaker<L, S>
where
    S: Clone,
    L: Layer<S>,
{
    type Service = L::Service;

    fn make_service(&mut self) -> Self::Service {
        self.layer.layer(self.service.clone())
    }
}

/// A layer which retries failed requests in certain cases.
///
/// Unlike most layers, it does not wrap a `Service`, but instead a `MakeService`. A new inner service is created for
/// each request, which handles all of the attempts for that single request.
///
/// A request is retried if it fails with a service error with a cause of `ThrottledError`, `UnavailableError`, or
/// `hyper::Error`. Only idempotent requests can be retried, as specified by the `RetryConfig` struct in the request's
/// extensions map. Additionally, if the request body was partially read by an attempt, it must be successfully reset
/// before the request can be retried.
///
/// The layer retries up to a specified maximum number of times, applying exponential backoff with full jitter in
/// between each attempt.
///
/// Since the extensions map is not cloneable, only a certain set of extensions will be propagated to the inner service:
///     * PropagationConfig
pub struct RetryLayer {
    max_num_retries: u32,
    backoff_slot_size: Duration,
}

impl RetryLayer {
    pub fn new(max_num_retries: u32, backoff_slot_size: Duration) -> RetryLayer {
        RetryLayer {
            max_num_retries,
            backoff_slot_size,
        }
    }
}

impl<S> Layer<S> for RetryLayer
where
    S: MakeService,
{
    type Service = RetryService<S>;

    fn layer(&self, source: S) -> Self::Service {
        RetryService {
            source,
            inner: None,
            max_num_retries: self.max_num_retries,
            backoff_slot_size: self.backoff_slot_size,
        }
    }
}

type RetryBody<'a, B> = Option<Pin<&'a mut ResetTrackingBody<B>>>;

pub struct RetryService<S>
where
    S: MakeService,
{
    source: S,
    inner: Option<S::Service>,
    max_num_retries: u32,
    backoff_slot_size: Duration,
}

impl<'a, S, B, R> Service<Request<RetryBody<'a, B>>> for RetryService<S>
where
    S: MakeService,
    for<'b> <S as MakeService>::Service:
        Service<Request<RetryBody<'b, B>>, Response = R, Error = Error> + 'a + Send,
    for<'b> <<S as MakeService>::Service as Service<Request<RetryBody<'b, B>>>>::Future: Send,
    B: ?Sized + Body + 'a + Send,
    R: 'a,
{
    type Response = R;
    type Error = Error;
    type Future = BoxFuture<'a, Result<Self::Response, Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let source = &mut self.source;
        self.inner
            .get_or_insert_with(|| source.make_service())
            .poll_ready(cx)
    }

    fn call(&mut self, req: Request<RetryBody<'a, B>>) -> Self::Future {
        let inner = self.inner.take().expect("call invoked before poll_ready");
        let max_num_retries = self.max_num_retries;
        let backoff_slot_size = self.backoff_slot_size;
        let config = *req
            .extensions()
            .get::<RetryConfig>()
            .expect("request missing RetryConfig extension");

        let state = State {
            inner,
            max_num_retries,
            backoff_slot_size,
            config,
            attempt: 0,
        };

        Box::pin(state.call(req))
    }
}

struct State<S> {
    inner: S,
    max_num_retries: u32,
    backoff_slot_size: Duration,
    config: RetryConfig,
    attempt: u32,
}

impl<S> State<S> {
    async fn call<B, R>(mut self, mut req: Request<RetryBody<'_, B>>) -> Result<R, Error>
    where
        S: for<'a> Service<Request<RetryBody<'a, B>>, Response = R, Error = Error>,
        B: ?Sized + Body + Send,
    {
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
    fn clone_request<'a, B>(
        &self,
        req: &'a mut Request<RetryBody<'_, B>>,
    ) -> Request<RetryBody<'a, B>>
    where
        B: ?Sized,
    {
        let mut new_req = Request::new(());

        *new_req.method_mut() = req.method().clone();
        *new_req.uri_mut() = req.uri().clone();
        *new_req.headers_mut() = req.headers().clone();

        if let Some(propagation_config) = req.extensions().get::<PropagationConfig>() {
            new_req.extensions_mut().insert(*propagation_config);
        }

        let parts = new_req.into_parts().0;
        Request::from_parts(parts, req.body_mut().as_mut().map(|r| r.as_mut()))
    }

    async fn send_attempt<R>(&mut self, req: R) -> Result<AttemptOutcome<S::Response>, Error>
    where
        S: Service<R, Error = Error>,
    {
        match self.inner.ready_and().await?.call(req).await {
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

    async fn prepare_for_retry<B>(
        &mut self,
        body: RetryBody<'_, B>,
        error: Error,
        retry_after: Option<Duration>,
    ) -> Result<(), Error>
    where
        B: ?Sized + Body + Send,
    {
        self.attempt += 1;
        if self.attempt >= self.max_num_retries {
            info!("exceeded retry limits");
            return Err(error);
        }

        if !self.config.idempotent {
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
                rand::thread_rng().gen_range(Duration::from_secs(0), max)
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
    use futures::future::{self, Ready};
    use http::HeaderValue;
    use tower_util::ServiceFn;

    struct MakerFn<S>(Option<S>);

    impl<S> MakeService for MakerFn<S> {
        type Service = S;

        fn make_service(&mut self) -> Self::Service {
            self.0.take().unwrap()
        }
    }

    fn maker_fn<F>(service: F) -> MakerFn<ServiceFn<F>> {
        MakerFn(Some(tower::service_fn(service)))
    }

    struct TestService;

    impl<'a, B> Service<Request<RetryBody<'a, B>>> for TestService {
        type Response = ();
        type Error = Error;
        type Future = Ready<Result<(), Error>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: Request<RetryBody<'a, B>>) -> Self::Future {
            future::ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn success() {
        fn foo<S>(_: S)
        where
            for<'a> S: Service<Request<RetryBody<'a, BytesBody>>>,
        {
        }

        let service = RetryLayer::new(0, Duration::from_secs(0)).layer(MakerFn(Some(TestService)));
        foo(service);

        // service
        //     .oneshot(Request::new(None::<RetryBody<BytesBody>>))
        //     .await
        //     .unwrap();
    }
}
