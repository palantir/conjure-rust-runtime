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
use pin_project::pin_project;
use std::future::Future;
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

impl<S, R> Service<R> for TimeoutService<S>
where
    S: Service<R, Error = Error>,
{
    type Response = S::Response;
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

impl<F, T> Future for TimeoutFuture<F>
where
    F: Future<Output = Result<T, Error>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if this.delay.poll(cx).is_ready() {
            return Poll::Ready(Err(Error::internal_safe(TimeoutError(()))));
        }

        this.future.poll(cx)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn no_timeout() {
        time::pause();

        let service = TimeoutLayer::new(Duration::from_secs(1))
            .layer(tower::service_fn(|_| async move { Ok(()) }));

        service.oneshot(()).await.unwrap();
    }

    #[tokio::test]
    async fn timeout() {
        time::pause();

        let service =
            TimeoutLayer::new(Duration::from_secs(1)).layer(tower::service_fn(|_| async move {
                time::advance(Duration::from_secs(2)).await;
                Ok(())
            }));

        service.oneshot(()).await.err().unwrap();
    }
}
