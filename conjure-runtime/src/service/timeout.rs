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
use crate::errors::TimeoutError;
use conjure_error::Error;
use futures::ready;
use http::{HeaderMap, Response};
use http_body::{Body, SizeHint};
use pin_project::pin_project;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::{self, Delay};
use tower::layer::Layer;
use tower::Service;

/// A layer which applies a timeout to an inner service's request, including the time waiting for ready.
pub struct TimeoutLayer {
    timeout: Duration,
}

impl TimeoutLayer {
    pub fn new(timeout: Duration) -> TimeoutLayer {
        TimeoutLayer { timeout }
    }
}

impl<S> Layer<S> for TimeoutLayer {
    type Service = TimeoutService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TimeoutService {
            inner,
            timeout: self.timeout,
            delay: None,
        }
    }
}

pub struct TimeoutService<S> {
    inner: S,
    timeout: Duration,
    delay: Option<Delay>,
}

impl<S, R, B> Service<R> for TimeoutService<S>
where
    S: Service<R, Response = Response<B>, Error = Error>,
    B: Body,
{
    type Response = Response<TimeoutBody<B>>;
    type Error = Error;
    type Future = TimeoutFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let timeout = self.timeout;
        let delay = self.delay.get_or_insert_with(|| time::delay_for(timeout));
        if Pin::new(delay).poll(cx).is_ready() {
            return Poll::Ready(Err(Error::internal_safe(TimeoutError(()))));
        }

        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        TimeoutFuture {
            future: self.inner.call(req),
            delay: self.delay.take().expect("call invoked before poll_ready"),
        }
    }
}

#[pin_project]
pub struct TimeoutFuture<F> {
    #[pin]
    future: F,
    #[pin]
    delay: Delay,
}

impl<F, B> Future for TimeoutFuture<F>
where
    F: Future<Output = Result<Response<B>, Error>>,
{
    type Output = Result<Response<TimeoutBody<B>>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        if this.delay.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(Error::internal_safe(TimeoutError(()))));
        }

        let response = ready!(this.future.poll(cx))?;

        let deadline = this.delay.deadline();
        Poll::Ready(Ok(response.map(|body| TimeoutBody {
            body,
            delay: time::delay_until(deadline),
        })))
    }
}

#[pin_project]
pub struct TimeoutBody<B> {
    #[pin]
    body: B,
    #[pin]
    delay: Delay,
}

impl<B> Body for TimeoutBody<B>
where
    B: Body<Error = io::Error>,
{
    type Data = B::Data;
    type Error = io::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();

        if this.delay.poll(cx).is_ready() {
            return Poll::Ready(Some(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                TimeoutError(()),
            ))));
        }

        this.body.poll_data(cx)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        let this = self.project();

        if this.delay.poll(cx).is_ready() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                TimeoutError(()),
            )));
        }

        this.body.poll_trailers(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tower::ServiceExt;

    struct TestBody;

    impl Body for TestBody {
        type Data = &'static [u8];
        type Error = io::Error;

        fn poll_data(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
            Poll::Ready(Some(Ok(b"hello world")))
        }

        fn poll_trailers(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
            Poll::Ready(Ok(None))
        }
    }

    #[tokio::test]
    async fn no_timeout() {
        time::pause();

        let service =
            TimeoutLayer::new(Duration::from_secs(1)).layer(tower::service_fn(|_| async move {
                Ok(Response::new(TestBody))
            }));

        service.oneshot(()).await.unwrap();
    }

    #[tokio::test]
    async fn timeout() {
        time::pause();

        let service =
            TimeoutLayer::new(Duration::from_secs(1)).layer(tower::service_fn(|_| async move {
                time::advance(Duration::from_secs(2)).await;

                Ok(Response::new(TestBody))
            }));

        service.oneshot(()).await.err().unwrap();
    }

    #[tokio::test]
    async fn body_timeout() {
        time::pause();

        let service =
            TimeoutLayer::new(Duration::from_secs(1)).layer(tower::service_fn(|_| async move {
                Ok(Response::new(TestBody))
            }));

        let mut response = service.oneshot(()).await.unwrap();
        assert_eq!(response.data().await.unwrap().unwrap(), b"hello world");

        time::advance(Duration::from_secs(2)).await;
        response.data().await.unwrap().err().unwrap();
    }
}
