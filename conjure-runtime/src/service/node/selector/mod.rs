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
use crate::service::node::selector::balanced::{
    BalancedNodeSelectorFuture, BalancedNodeSelectorLayer, BalancedNodeSelectorService,
};
use crate::service::node::selector::empty::{
    EmptyNodeSelectorFuture, EmptyNodeSelectorLayer, EmptyNodeSelectorService,
};
use crate::service::node::selector::pin_until_error::{
    FixedNodes, PinUntilErrorNodeSelectorFuture, PinUntilErrorNodeSelectorLayer,
    PinUntilErrorNodeSelectorService, ReshufflingNodes,
};
use crate::service::node::selector::single::{
    SingleNodeSelectorFuture, SingleNodeSelectorLayer, SingleNodeSelectorService,
};
use crate::service::node::Node;
use crate::service::Layer;
use crate::{Builder, NodeSelectionStrategy};
use conjure_error::Error;
use http::{Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

mod balanced;
mod empty;
mod pin_until_error;
mod single;

/// A layer which selects a node to use for the request, injecting it into the request's extensions map.
///
/// Multiple selection strategies are supported, the choice of which is controlled by the builder's
/// `NodeSelectionStrategy`.
pub enum NodeSelectorLayer {
    Empty(EmptyNodeSelectorLayer),
    Single(SingleNodeSelectorLayer),
    PinUntilError(PinUntilErrorNodeSelectorLayer<ReshufflingNodes>),
    PinUntilErrorWithoutReshuffle(PinUntilErrorNodeSelectorLayer<FixedNodes>),
    Balanced(BalancedNodeSelectorLayer),
}

impl NodeSelectorLayer {
    pub fn new<T>(service: &str, builder: &Builder<T>) -> NodeSelectorLayer {
        let mut nodes = builder
            .get_uris()
            .iter()
            .map(|url| {
                // normalize by stripping a trailing `/` if present
                let mut url = url.clone();
                url.path_segments_mut().unwrap().pop_if_empty();

                Arc::new(Node {
                    host_metrics: builder.get_host_metrics().map(|m| {
                        m.get(
                            service,
                            url.host_str().unwrap(),
                            url.port_or_known_default().unwrap(),
                        )
                    }),
                    url,
                })
            })
            .collect::<Vec<_>>();

        if nodes.is_empty() {
            NodeSelectorLayer::Empty(EmptyNodeSelectorLayer::new(service))
        } else if nodes.len() == 1 {
            NodeSelectorLayer::Single(SingleNodeSelectorLayer::new(nodes.pop().unwrap()))
        } else {
            match builder.get_node_selection_strategy() {
                NodeSelectionStrategy::PinUntilError => NodeSelectorLayer::PinUntilError(
                    PinUntilErrorNodeSelectorLayer::new(ReshufflingNodes::new(nodes)),
                ),
                NodeSelectionStrategy::PinUntilErrorWithoutReshuffle => {
                    NodeSelectorLayer::PinUntilErrorWithoutReshuffle(
                        PinUntilErrorNodeSelectorLayer::new(FixedNodes::new(nodes)),
                    )
                }
                NodeSelectionStrategy::Balanced => {
                    NodeSelectorLayer::Balanced(BalancedNodeSelectorLayer::new(nodes))
                }
            }
        }
    }
}

impl<S> Layer<S> for NodeSelectorLayer {
    type Service = NodeSelectorService<S>;

    fn layer(self, inner: S) -> Self::Service {
        match self {
            NodeSelectorLayer::Empty(l) => NodeSelectorService::Empty(l.layer(inner)),
            NodeSelectorLayer::Single(l) => NodeSelectorService::Single(l.layer(inner)),
            NodeSelectorLayer::PinUntilError(l) => {
                NodeSelectorService::PinUntilError(l.layer(inner))
            }
            NodeSelectorLayer::PinUntilErrorWithoutReshuffle(l) => {
                NodeSelectorService::PinUntilErrorWithoutReshuffle(l.layer(inner))
            }
            NodeSelectorLayer::Balanced(l) => NodeSelectorService::Balanced(l.layer(inner)),
        }
    }
}

pub enum NodeSelectorService<S> {
    Empty(EmptyNodeSelectorService<S>),
    Single(SingleNodeSelectorService<S>),
    PinUntilError(PinUntilErrorNodeSelectorService<ReshufflingNodes, S>),
    PinUntilErrorWithoutReshuffle(PinUntilErrorNodeSelectorService<FixedNodes, S>),
    Balanced(BalancedNodeSelectorService<S>),
}

impl<S, B1, B2> Service<Request<B1>> for NodeSelectorService<S>
where
    S: Service<Request<B1>, Response = Response<B2>, Error = Error>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = NodeSelectorFuture<S::Future>;

    fn call(&self, req: Request<B1>) -> Self::Future {
        match self {
            NodeSelectorService::Empty(s) => NodeSelectorFuture::Empty(s.call(req)),
            NodeSelectorService::Single(s) => NodeSelectorFuture::Single(s.call(req)),
            NodeSelectorService::PinUntilError(s) => NodeSelectorFuture::PinUntilError(s.call(req)),
            NodeSelectorService::PinUntilErrorWithoutReshuffle(s) => {
                NodeSelectorFuture::PinUntilErrorWithoutReshuffle(s.call(req))
            }
            NodeSelectorService::Balanced(s) => NodeSelectorFuture::Balanced(s.call(req)),
        }
    }
}

#[pin_project(project = Projection)]
pub enum NodeSelectorFuture<F> {
    Empty(#[pin] EmptyNodeSelectorFuture<F>),
    Single(#[pin] SingleNodeSelectorFuture<F>),
    PinUntilError(#[pin] PinUntilErrorNodeSelectorFuture<ReshufflingNodes, F>),
    PinUntilErrorWithoutReshuffle(#[pin] PinUntilErrorNodeSelectorFuture<FixedNodes, F>),
    Balanced(#[pin] BalancedNodeSelectorFuture<F>),
}

impl<F, B> Future for NodeSelectorFuture<F>
where
    F: Future<Output = Result<Response<B>, Error>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            Projection::Empty(f) => f.poll(cx),
            Projection::Single(f) => f.poll(cx),
            Projection::PinUntilError(f) => f.poll(cx),
            Projection::PinUntilErrorWithoutReshuffle(f) => f.poll(cx),
            Projection::Balanced(f) => f.poll(cx),
        }
    }
}
