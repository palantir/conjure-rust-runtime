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
use crate::service::node::Node;
use crate::service::Layer;
use futures::ready;
use http::{Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::time::Instant;

/// A layer which updates the host metrics for the node stored in the request's extensions map.
pub struct NodeMetricsLayer;

impl<S> Layer<S> for NodeMetricsLayer {
    type Service = NodeMetricsService<S>;

    fn layer(self, inner: S) -> NodeMetricsService<S> {
        NodeMetricsService { inner }
    }
}

pub struct NodeMetricsService<S> {
    inner: S,
}

impl<S, B1, B2> Service<Request<B1>> for NodeMetricsService<S>
where
    S: Service<Request<B1>, Response = Response<B2>>,
{
    type Error = S::Error;
    type Response = S::Response;
    type Future = NodeMetricsFuture<S::Future>;

    fn call(&self, req: Request<B1>) -> Self::Future {
        let node = req
            .extensions()
            .get::<Arc<Node>>()
            .expect("should have a Node extension")
            .clone();

        NodeMetricsFuture {
            inner: self.inner.call(req),
            start: Instant::now(),
            node,
        }
    }
}

#[pin_project]
pub struct NodeMetricsFuture<F> {
    #[pin]
    inner: F,
    start: Instant,
    node: Arc<Node>,
}

impl<F, B, E> Future for NodeMetricsFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let result = ready!(this.inner.poll(cx));

        if let Some(host_metrics) = &this.node.host_metrics {
            match &result {
                Ok(response) => host_metrics.update(response.status(), this.start.elapsed()),
                Err(_) => host_metrics.update_io_error(),
            }
        }

        Poll::Ready(result)
    }
}
