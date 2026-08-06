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
use crate::builder::CachedConfig;
use crate::service::gzip::{DecodedBody, GzipLayer};
use crate::service::http_error::HttpErrorLayer;
use crate::service::map_error::MapErrorLayer;
use crate::service::metrics::MetricsLayer;
use crate::service::node::{NodeMetricsLayer, NodeSelectorLayer, NodeUriLayer};
use crate::service::proxy::ProxyLayer;
use crate::service::raw::{RawClient, RawResponseBody};
use crate::service::response_body::ResponseBodyLayer;
use crate::service::retry::RetryLayer;
use crate::service::root_span::RootSpanLayer;
use crate::service::trace_propagation::TracePropagationLayer;
use crate::service::user_agent::UserAgentLayer;
use crate::service::wait_for_spans::{WaitForSpansBody, WaitForSpansLayer};
use crate::service::Service;
use crate::service::{Identity, Layer, ServiceBuilder, Stack};
use crate::weak_cache::Cached;
use crate::{builder, BodyWriter, Builder, ResponseBody};
use arc_swap::ArcSwap;
use conjure_error::Error;
#[cfg(not(target_arch = "wasm32"))]
use conjure_http::client::{AsyncClient, AsyncRequestBody};
use conjure_http::client::{AsyncService, ConjureRuntime};
#[cfg(target_arch = "wasm32")]
use conjure_http::client::{
    LocalAsyncClient as AsyncClient, LocalAsyncRequestBody as AsyncRequestBody,
};
use conjure_runtime_config::ServiceConfig;
use http::{Request, Response};
use refreshable::Subscription;
use std::sync::Arc;

macro_rules! layers {
    () => { Identity };
    ($layer:ty, $($rem:tt)*) => { Stack<$layer, layers!($($rem)*)> };
}

type BaseLayer = layers!(
    ResponseBodyLayer,
    MetricsLayer,
    RootSpanLayer,
    RetryLayer,
    HttpErrorLayer,
    WaitForSpansLayer,
    NodeSelectorLayer,
    NodeUriLayer,
    NodeMetricsLayer,
    ProxyLayer,
    TracePropagationLayer,
    UserAgentLayer,
    GzipLayer,
    MapErrorLayer,
);

type BaseService = <BaseLayer as Layer<RawClient>>::Service;

pub(crate) type BaseBody = WaitForSpansBody<DecodedBody<RawResponseBody>>;

pub(crate) struct ClientState {
    service: BaseService,
}

impl ClientState {
    pub(crate) fn new(builder: &Builder<builder::Complete>) -> Result<ClientState, Error> {
        let client = RawClient::new(builder)?;

        let service = ServiceBuilder::new()
            .layer(ResponseBodyLayer)
            .layer(MetricsLayer::new(builder))
            .layer(RootSpanLayer)
            .layer(RetryLayer::new(builder))
            .layer(HttpErrorLayer::new(builder))
            .layer(WaitForSpansLayer)
            .layer(NodeSelectorLayer::new(builder))
            .layer(NodeUriLayer)
            .layer(NodeMetricsLayer)
            .layer(ProxyLayer::new(builder)?)
            .layer(TracePropagationLayer)
            .layer(UserAgentLayer::new(builder))
            .layer(GzipLayer)
            .layer(MapErrorLayer)
            .service(client);

        Ok(ClientState { service })
    }
}

/// An asynchronous HTTP client to a remote service.
#[derive(Clone)]
pub struct Client {
    state: Arc<ArcSwap<Cached<CachedConfig, ClientState>>>,
    _subscription: Option<Arc<Subscription<ServiceConfig, Error>>>,
}

impl Client {
    /// Creates a new `Builder` for clients.
    #[inline]
    pub fn builder() -> Builder<builder::ServiceStage> {
        Builder::new()
    }
}

impl Client {
    pub(crate) fn new(
        state: Arc<ArcSwap<Cached<CachedConfig, ClientState>>>,
        subscription: Option<Subscription<ServiceConfig, Error>>,
    ) -> Client {
        Client {
            state,
            _subscription: subscription.map(Arc::new),
        }
    }
}

impl AsyncService<Client> for Client {
    fn new(client: Client, _: &Arc<ConjureRuntime>) -> Self {
        client
    }
}

impl AsyncClient for Client {
    type BodyWriter = BodyWriter;

    type ResponseBody = ResponseBody;

    async fn send(
        &self,
        request: Request<AsyncRequestBody<'_, Self::BodyWriter>>,
    ) -> Result<Response<Self::ResponseBody>, Error> {
        self.state.load().service.call(request).await
    }
}
