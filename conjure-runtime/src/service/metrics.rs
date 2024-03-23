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
use crate::service::Layer;
use crate::Builder;
use crate::{builder, raw::Service};
use conjure_error::Error;
use conjure_http::client::Endpoint;
use http::{Request, Response};
use std::sync::Arc;
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
    metrics: Option<Metrics>,
}

impl MetricsLayer {
    pub fn new<T>(builder: &Builder<builder::Complete<T>>) -> MetricsLayer {
        MetricsLayer {
            metrics: builder.get_metrics().map(|m| Metrics {
                metrics: m.clone(),
                service_name: builder.get_service().to_string(),
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
    metrics: Option<Metrics>,
}

impl<S, B1, B2> Service<Request<B1>> for MetricsService<S>
where
    S: Service<Request<B1>, Response = Response<B2>, Error = Error> + Sync + Send,
    B1: Send,
{
    type Response = S::Response;
    type Error = S::Error;

    async fn call(&self, req: Request<B1>) -> Result<Self::Response, Self::Error> {
        let endpoint = req
            .extensions()
            .get::<Endpoint>()
            .expect("Request extensions missing Endpoint")
            .clone();

        let start = Instant::now();
        let result = self.inner.call(req).await;

        if let Some(metrics) = &self.metrics {
            let status = match &result {
                Ok(_) => "success",
                Err(_) => "failure",
            };

            metrics
                .metrics
                .timer(
                    MetricId::new("client.response")
                        .with_tag("channel-name", metrics.service_name.clone())
                        .with_tag("service-name", endpoint.service())
                        .with_tag("endpoint", endpoint.name())
                        .with_tag("status", status),
                )
                .update(start.elapsed());
        }

        result
    }
}
