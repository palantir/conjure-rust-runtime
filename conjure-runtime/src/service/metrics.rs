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
use crate::raw::Service;
use crate::service::Layer;
use crate::Builder;
use conjure_error::Error;
use conjure_http::client::Endpoint;
use futures::ready;
use http::{Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::time::Instant;
use witchcraft_metrics::{MetricId, MetricRegistry};

struct Metrics {
    metrics: Arc<MetricRegistry>,
    service_name: String,
}

/// A layer which updates the `client.response` and `client.response.error` metrics.
///
/// Only errors with a cause of `RawClientError` will be treated as IO errors.
pub struct MetricsLayer {
    metrics: Option<Arc<Metrics>>,
}

impl MetricsLayer {
    pub fn new<T>(service: &str, builder: &Builder<T>) -> MetricsLayer {
        MetricsLayer {
            metrics: builder.get_metrics().map(|m| {
                Arc::new(Metrics {
                    metrics: m.clone(),
                    service_name: service.to_string(),
                })
            }),
        }
    }
}

impl<S> Layer<S> for MetricsLayer {
    type Service = MetricsService<S>;

    fn layer(self, inner: S) -> Self::Service {
        MetricsService {
            inner,
            metrics: self.metrics,
        }
    }
}

pub struct MetricsService<S> {
    inner: S,
    metrics: Option<Arc<Metrics>>,
}

impl<S, B1, B2> Service<Request<B1>> for MetricsService<S>
where
    S: Service<Request<B1>, Response = Response<B2>, Error = Error>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = MetricsFuture<S::Future>;

    fn call(&self, req: Request<B1>) -> Self::Future {
        MetricsFuture {
            endpoint: req
                .extensions()
                .get::<Endpoint>()
                .expect("Request extensions missing Endpoint")
                .clone(),
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
    endpoint: Endpoint,
    metrics: Option<Arc<Metrics>>,
}

impl<F, B> Future for MetricsFuture<F>
where
    F: Future<Output = Result<Response<B>, Error>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let result = ready!(this.future.poll(cx));

        if let Some(metrics) = this.metrics {
            let status = match &result {
                Ok(_) => "success",
                Err(_) => "failure",
            };

            metrics
                .metrics
                .timer(
                    MetricId::new("client.response")
                        .with_tag("channel-name", metrics.service_name.clone())
                        .with_tag("service-name", this.endpoint.service().to_string())
                        .with_tag("endpoint", this.endpoint.name().to_string())
                        .with_tag("status", status),
                )
                .update(this.start.elapsed());
        }

        Poll::Ready(result)
    }
}
