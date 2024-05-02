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
use crate::raw::{BuildRawClient, DefaultRawClient, RawBody, Service};
use crate::service::gzip::{DecodedBody, GzipLayer};
use crate::service::http_error::HttpErrorLayer;
use crate::service::map_error::MapErrorLayer;
use crate::service::metrics::MetricsLayer;
use crate::service::node::{NodeMetricsLayer, NodeSelectorLayer, NodeUriLayer};
use crate::service::proxy::{ProxyConfig, ProxyLayer};
use crate::service::response_body::ResponseBodyLayer;
use crate::service::retry::RetryLayer;
use crate::service::root_span::RootSpanLayer;
use crate::service::trace_propagation::TracePropagationLayer;
use crate::service::user_agent::UserAgentLayer;
use crate::service::wait_for_spans::{WaitForSpansBody, WaitForSpansLayer};
use crate::service::{Identity, Layer, ServiceBuilder, Stack};
use crate::weak_cache::Cached;
use crate::{builder, BodyWriter, Builder, ResponseBody};
use arc_swap::ArcSwap;
use bytes::Bytes;
use conjure_error::Error;
use conjure_http::client::{AsyncClient, AsyncRequestBody, AsyncService};
use conjure_runtime_config::ServiceConfig;
use http::{Request, Response};
use refreshable::Subscription;
use std::error;
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

type BaseService<T> = <BaseLayer as Layer<T>>::Service;

pub(crate) type BaseBody<B> = WaitForSpansBody<DecodedBody<B>>;

pub(crate) struct ClientState<T> {
    service: BaseService<T>,
}

impl<T> ClientState<T> {
    pub(crate) fn new<U>(builder: &Builder<builder::Complete<U>>) -> Result<ClientState<T>, Error>
    where
        U: BuildRawClient<RawClient = T>,
    {
        let client = builder.get_raw_client_builder().build_raw_client(builder)?;

        let proxy = ProxyConfig::from_config(builder.get_proxy())?;

        let service = ServiceBuilder::new()
            .layer(ResponseBodyLayer)
            .layer(MetricsLayer::new(builder))
            .layer(RootSpanLayer)
            .layer(RetryLayer::new(builder))
            .layer(HttpErrorLayer::new(builder))
            .layer(WaitForSpansLayer)
            .layer(NodeSelectorLayer::new(builder)?)
            .layer(NodeUriLayer)
            .layer(NodeMetricsLayer)
            .layer(ProxyLayer::new(&proxy))
            .layer(TracePropagationLayer)
            .layer(UserAgentLayer::new(builder))
            .layer(GzipLayer)
            .layer(MapErrorLayer)
            .service(client);

        Ok(ClientState { service })
    }
}

/// An asynchronous HTTP client to a remote service.
///
/// It implements the Conjure `AsyncClient` trait, but also offers a "raw" request interface for use with services that
/// don't provide Conjure service definitions.
pub struct Client<T = DefaultRawClient> {
    state: Arc<ArcSwap<Cached<CachedConfig, ClientState<T>>>>,
    subscription: Option<Arc<Subscription<ServiceConfig, Error>>>,
}

impl<T> Clone for Client<T> {
    fn clone(&self) -> Self {
        Client {
            state: self.state.clone(),
            subscription: self.subscription.clone(),
        }
    }
}

impl Client {
    /// Creates a new `Builder` for clients.
    #[inline]
    pub fn builder() -> Builder<builder::ServiceStage> {
        Builder::new()
    }
}

impl<T> Client<T> {
    pub(crate) fn new(
        state: Arc<ArcSwap<Cached<CachedConfig, ClientState<T>>>>,
        subscription: Option<Subscription<ServiceConfig, Error>>,
    ) -> Client<T> {
        Client {
            state,
            subscription: subscription.map(Arc::new),
        }
    }
}

impl<T> AsyncService<Client<T>> for Client<T> {
    fn new(client: Client<T>) -> Self {
        client
    }
}

impl<T, B> AsyncClient for Client<T>
where
    T: Service<http::Request<RawBody>, Response = http::Response<B>> + 'static + Sync + Send,
    T::Error: Into<Box<dyn error::Error + Sync + Send>>,
    B: http_body::Body<Data = Bytes> + 'static + Send,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type BodyWriter = BodyWriter;

    type ResponseBody = ResponseBody<B>;

    async fn send(
        &self,
        request: Request<AsyncRequestBody<'_, Self::BodyWriter>>,
    ) -> Result<Response<Self::ResponseBody>, Error> {
        self.state.load().service.call(request).await
    }
}
