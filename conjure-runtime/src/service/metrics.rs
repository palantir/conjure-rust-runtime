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
use crate::Builder;
use conjure_error::Error;
use futures::ready;
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::time::Instant;
use tower::layer::Layer;
use tower::Service;
use witchcraft_metrics::{Meter, MetricId, Timer};

struct Metrics {
    response_timer: Arc<Timer>,
    io_error_meter: Arc<Meter>,
}

/// A layer which updates the `client.response` and `client.response.error` metrics.
///
/// Only errors with a cause of `hyper::Error` will be treated as IO errors.
pub struct MetricsLayer {
    metrics: Option<Arc<Metrics>>,
}

impl MetricsLayer {
    pub fn new(service: &str, builder: &Builder) -> MetricsLayer {
        MetricsLayer {
            metrics: builder.metrics.as_ref().map(|m| {
                Arc::new(Metrics {
                    response_timer: m.timer(
                        MetricId::new("client.response")
                            .with_tag("service-name", service.to_string()),
                    ),
                    io_error_meter: m.meter(
                        MetricId::new("client.response.error")
                            .with_tag("service-name", service.to_string())
                            .with_tag("reason", "IOException"),
                    ),
                })
            }),
        }
    }
}

impl<S> Layer<S> for MetricsLayer {
    type Service = MetricsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MetricsService {
            inner,
            metrics: self.metrics.clone(),
        }
    }
}

pub struct MetricsService<S> {
    inner: S,
    metrics: Option<Arc<Metrics>>,
}

impl<S, R> Service<R> for MetricsService<S>
where
    S: Service<R, Error = Error>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = MetricsFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        MetricsFuture {
            future: self.inner.call(req),
            start: Instant::now(),
            metrics: self.metrics.clone(),
        }
    }
}

#[pin_project]
pub struct MetricsFuture<F> {
    #[pin]
    future: F,
    start: Instant,
    metrics: Option<Arc<Metrics>>,
}

impl<F, R> Future for MetricsFuture<F>
where
    F: Future<Output = Result<R, Error>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let result = ready!(this.future.poll(cx));

        if let Some(metrics) = this.metrics {
            match &result {
                Ok(_) => metrics.response_timer.update(this.start.elapsed()),
                Err(e) if e.cause().is::<hyper::Error>() => metrics.io_error_meter.mark(1),
                _ => {}
            }
        }

        Poll::Ready(result)
    }
}
