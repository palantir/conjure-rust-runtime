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
use crate::service::node::LimitedNode;
use crate::service::Layer;
use conjure_error::Error;
use http::{Request, Response};

/// A node selector layer which always selects a single node.
///
/// This isn't necessary (the normal selectors will work with one node), but avoids a bit of extra unnecessary
/// bookkeeping when there's only one node for a service.
pub struct SingleNodeSelectorLayer {
    node: LimitedNode,
}

impl SingleNodeSelectorLayer {
    pub fn new(node: LimitedNode) -> SingleNodeSelectorLayer {
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
    node: LimitedNode,
}

impl<S, B1, B2> Service<Request<B1>> for SingleNodeSelectorService<S>
where
    S: Service<Request<B1>, Response = Response<B2>, Error = Error> + Sync + Send,
    B1: Sync + Send,
{
    type Response = S::Response;
    type Error = S::Error;

    async fn call(&self, req: Request<B1>) -> Result<Self::Response, Self::Error> {
        self.node.wrap(&self.inner, req).await
    }
}
