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
use http::Request;
use std::sync::Arc;

/// A node selector layer which always selects a single node.
///
/// This isn't necessary (the normal selectors will work with one node), but avoids a bit of extra unnecessary
/// bookkeeping when there's only one node for a service.
pub struct SingleNodeSelectorLayer {
    node: Arc<Node>,
}

impl SingleNodeSelectorLayer {
    pub fn new(node: Arc<Node>) -> SingleNodeSelectorLayer {
        SingleNodeSelectorLayer { node }
    }
}

impl<S> Layer<S> for SingleNodeSelectorLayer {
    type Service = SingleNodeSelectorService<S>;

    fn layer(self, inner: S) -> SingleNodeSelectorService<S> {
        SingleNodeSelectorService {
            inner,
            node: self.node,
        }
    }
}

pub struct SingleNodeSelectorService<S> {
    inner: S,
    node: Arc<Node>,
}

impl<S, B> Service<Request<B>> for SingleNodeSelectorService<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = SingleNodeSelectorFuture<S::Future>;

    fn call(&self, mut req: Request<B>) -> Self::Future {
        req.extensions_mut().insert(self.node.clone());
        self.inner.call(req)
    }
}

pub type SingleNodeSelectorFuture<F> = F;
