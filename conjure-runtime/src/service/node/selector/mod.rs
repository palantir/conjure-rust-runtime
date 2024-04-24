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
    BalancedNodeSelectorLayer, BalancedNodeSelectorService,
};
use crate::service::node::selector::empty::{EmptyNodeSelectorLayer, EmptyNodeSelectorService};
use crate::service::node::selector::pin_until_error::{
    FixedNodes, PinUntilErrorNodeSelectorLayer, PinUntilErrorNodeSelectorService, ReshufflingNodes,
};
use crate::service::node::selector::single::{SingleNodeSelectorLayer, SingleNodeSelectorService};
use crate::service::node::LimitedNode;
use crate::service::Layer;
use crate::{builder, Builder, NodeSelectionStrategy};
use conjure_error::Error;
use http::{Request, Response};

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
    pub fn new<T>(builder: &Builder<builder::Complete<T>>) -> Result<NodeSelectorLayer, Error> {
        let mut nodes = builder
            .postprocessed_uris()?
            .iter()
            .enumerate()
            .map(|(i, url)| {
                LimitedNode::new(
                    builder.get_override_host_index().unwrap_or(i),
                    url,
                    builder.get_service(),
                    builder,
                )
            })
            .collect::<Vec<_>>();

        let layer = if nodes.is_empty() {
            NodeSelectorLayer::Empty(EmptyNodeSelectorLayer::new(builder.get_service()))
        } else if nodes.len() == 1 {
            NodeSelectorLayer::Single(SingleNodeSelectorLayer::new(nodes.pop().unwrap()))
        } else {
            match builder.get_node_selection_strategy() {
                NodeSelectionStrategy::PinUntilError => NodeSelectorLayer::PinUntilError(
                    PinUntilErrorNodeSelectorLayer::new(ReshufflingNodes::new(nodes, builder)),
                ),
                NodeSelectionStrategy::PinUntilErrorWithoutReshuffle => {
                    NodeSelectorLayer::PinUntilErrorWithoutReshuffle(
                        PinUntilErrorNodeSelectorLayer::new(FixedNodes::new(nodes, builder)),
                    )
                }
                NodeSelectionStrategy::Balanced => {
                    NodeSelectorLayer::Balanced(BalancedNodeSelectorLayer::new(nodes, builder))
                }
            }
        };

        Ok(layer)
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
    S: Service<Request<B1>, Response = Response<B2>, Error = Error> + Sync + Send,
    B1: Sync + Send,
{
    type Response = S::Response;
    type Error = S::Error;

    async fn call(&self, req: Request<B1>) -> Result<Self::Response, Self::Error> {
        match self {
            NodeSelectorService::Empty(s) => s.call(req).await,
            NodeSelectorService::Single(s) => s.call(req).await,
            NodeSelectorService::PinUntilError(s) => s.call(req).await,
            NodeSelectorService::PinUntilErrorWithoutReshuffle(s) => s.call(req).await,
            NodeSelectorService::Balanced(s) => s.call(req).await,
        }
    }
}
