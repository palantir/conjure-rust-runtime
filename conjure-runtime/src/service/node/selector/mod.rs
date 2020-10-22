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
use crate::service::node::selector::pin_until_error::{
    FixedNodes, PinUntilErrorNodeSelectorFuture, PinUntilErrorNodeSelectorLayer,
    PinUntilErrorNodeSelectorService, ReshufflingNodes,
};
use crate::service::node::Node;
use crate::{Builder, NodeSelectionStrategy};
use http::{Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::layer::Layer;
use tower::Service;

mod pin_until_error;

/// A layer which selects a node to use for the request, injecting it into the request's extensions map.
///
/// Multiple selection strategies are supported, the choice of which is controlled by the builder's
/// `NodeSelectionStrategy`.
pub enum NodeSelectorLayer {
    PinUntilError(PinUntilErrorNodeSelectorLayer<ReshufflingNodes>),
    PinUntilErrorWithoutReshuffle(PinUntilErrorNodeSelectorLayer<FixedNodes>),
}

impl NodeSelectorLayer {
    pub fn new(service: &str, builder: &Builder) -> NodeSelectorLayer {
        let nodes = builder
            .uris
            .iter()
            .map(|url| {
                // normalize by stripping a trailing `/` if present
                let mut url = url.clone();
                url.path_segments_mut().unwrap().pop_if_empty();

                Arc::new(Node {
                    host_metrics: builder.host_metrics.as_ref().map(|m| {
                        m.get(
                            service,
                            url.host_str().unwrap(),
                            url.port_or_known_default().unwrap(),
                        )
                    }),
                    url,
                })
            })
            .collect();

        match builder.node_selection_strategy {
            NodeSelectionStrategy::PinUntilError => NodeSelectorLayer::PinUntilError(
                PinUntilErrorNodeSelectorLayer::new(ReshufflingNodes::new(nodes)),
            ),
            NodeSelectionStrategy::PinUntilErrorWithoutReshuffle => {
                NodeSelectorLayer::PinUntilErrorWithoutReshuffle(
                    PinUntilErrorNodeSelectorLayer::new(FixedNodes::new(nodes)),
                )
            }
        }
    }
}

impl<S> Layer<S> for NodeSelectorLayer {
    type Service = NodeSelectorService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        match self {
            NodeSelectorLayer::PinUntilError(l) => {
                NodeSelectorService::PinUntilError(l.layer(inner))
            }
            NodeSelectorLayer::PinUntilErrorWithoutReshuffle(l) => {
                NodeSelectorService::PinUntilErrorWithoutReshuffle(l.layer(inner))
            }
        }
    }
}

pub enum NodeSelectorService<S> {
    PinUntilError(PinUntilErrorNodeSelectorService<ReshufflingNodes, S>),
    PinUntilErrorWithoutReshuffle(PinUntilErrorNodeSelectorService<FixedNodes, S>),
}

impl<S, B1, B2> Service<Request<B1>> for NodeSelectorService<S>
where
    S: Service<Request<B1>, Response = Response<B2>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = NodeSelectorFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self {
            NodeSelectorService::PinUntilError(s) => s.poll_ready(cx),
            NodeSelectorService::PinUntilErrorWithoutReshuffle(s) => s.poll_ready(cx),
        }
    }

    fn call(&mut self, req: Request<B1>) -> Self::Future {
        match self {
            NodeSelectorService::PinUntilError(s) => NodeSelectorFuture::PinUntilError(s.call(req)),
            NodeSelectorService::PinUntilErrorWithoutReshuffle(s) => {
                NodeSelectorFuture::PinUntilErrorWithoutReshuffle(s.call(req))
            }
        }
    }
}

#[pin_project(project = Projection)]
pub enum NodeSelectorFuture<F> {
    PinUntilError(#[pin] PinUntilErrorNodeSelectorFuture<ReshufflingNodes, F>),
    PinUntilErrorWithoutReshuffle(#[pin] PinUntilErrorNodeSelectorFuture<FixedNodes, F>),
}

impl<F, B, E> Future for NodeSelectorFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            Projection::PinUntilError(f) => f.poll(cx),
            Projection::PinUntilErrorWithoutReshuffle(f) => f.poll(cx),
        }
    }
}
