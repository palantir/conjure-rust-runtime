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
use futures::ready;
use http::Uri;
use hyper_rustls::MaybeHttpsStream;
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower_layer::Layer;
use tower_service::Service;
use witchcraft_metrics::{MetricId, MetricRegistry};

struct Shared {
    metrics: Option<Arc<MetricRegistry>>,
    service: String,
}

/// A connector layer which records a metric tracking TLS handshakes tagged by protocol and cipher.
pub struct TlsMetricsLayer {
    shared: Arc<Shared>,
}

impl TlsMetricsLayer {
    pub fn new(builder: &Builder) -> TlsMetricsLayer {
        TlsMetricsLayer {
            shared: Arc::new(Shared {
                metrics: builder.get_metrics().cloned(),
                service: builder.get_service().to_string(),
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
{
    type Response = MaybeHttpsStream<T>;
    type Error = S::Error;
    type Future = TlsMetricsFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Uri) -> Self::Future {
        TlsMetricsFuture {
            future: self.inner.call(req),
            shared: self.shared.clone(),
        }
    }
}

#[pin_project]
pub struct TlsMetricsFuture<F> {
    #[pin]
    future: F,
    shared: Arc<Shared>,
}

impl<F, T, E> Future for TlsMetricsFuture<F>
where
    F: Future<Output = Result<MaybeHttpsStream<T>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let stream = ready!(this.future.poll(cx))?;

        if let (Some(metrics), MaybeHttpsStream::Https(s)) = (&this.shared.metrics, &stream) {
            let protocol = s.get_ref().1.protocol_version().expect("session is active");
            let cipher = s
                .get_ref()
                .1
                .negotiated_cipher_suite()
                .expect("session is active");

            metrics
                .meter(
                    MetricId::new("tls.handshake")
                        .with_tag("context", this.shared.service.clone())
                        .with_tag("protocol", protocol.as_str().unwrap_or("unknown"))
                        .with_tag("cipher", cipher.suite().as_str().unwrap_or("unknown")),
                )
                .mark(1);
        }

        Poll::Ready(Ok(stream))
    }
}
