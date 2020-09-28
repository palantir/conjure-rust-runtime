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
use futures::future::BoxFuture;
use http::Uri;
use hyper_openssl::MaybeHttpsStream;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::layer::Layer;
use tower::Service;
use witchcraft_metrics::{MetricId, MetricRegistry};

struct Shared {
    metrics: Arc<MetricRegistry>,
    service: String,
}

/// A connector layer which records a metric tracking TLS handshakes tagged by protocol and cipher.
pub struct TlsMetricsLayer {
    shared: Arc<Shared>,
}

impl TlsMetricsLayer {
    pub fn new(metrics: &Arc<MetricRegistry>, service: &str) -> TlsMetricsLayer {
        TlsMetricsLayer {
            shared: Arc::new(Shared {
                metrics: metrics.clone(),
                service: service.to_string(),
            }),
        }
    }
}

impl<S> Layer<S> for TlsMetricsLayer {
    type Service = TlsMetricsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TlsMetricsService {
            inner,
            shared: self.shared.clone(),
        }
    }
}

#[derive(Clone)]
pub struct TlsMetricsService<S> {
    inner: S,
    shared: Arc<Shared>,
}

impl<S, T> Service<Uri> for TlsMetricsService<S>
where
    S: Service<Uri, Response = MaybeHttpsStream<T>>,
    S::Future: 'static + Send,
{
    type Response = MaybeHttpsStream<T>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Uri) -> Self::Future {
        let shared = self.shared.clone();
        let future = self.inner.call(req);
        Box::pin(async move {
            let stream = future.await?;

            if let MaybeHttpsStream::Https(s) = &stream {
                let cipher = s.ssl().current_cipher().expect("session is active");
                shared
                    .metrics
                    .meter(
                        MetricId::new("tls.handshake")
                            .with_tag("context", shared.service.clone())
                            .with_tag("protocol", s.ssl().version_str())
                            .with_tag(
                                "cipher",
                                cipher.standard_name().unwrap_or_else(|| cipher.name()),
                            ),
                    )
                    .mark(1);
            }

            Ok(stream)
        })
    }
}
