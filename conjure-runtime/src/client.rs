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
use crate::raw::{BuildRawClient, DefaultRawClient, RawBody, Service};
use crate::service::gzip::{DecodedBody, GzipLayer};
use crate::service::http_error::HttpErrorLayer;
use crate::service::map_error::MapErrorLayer;
use crate::service::metrics::MetricsLayer;
use crate::service::node::{NodeMetricsLayer, NodeSelectorLayer, NodeUriLayer};
use crate::service::proxy::{ProxyConfig, ProxyLayer};
use crate::service::request::RequestLayer;
use crate::service::response::ResponseLayer;
use crate::service::retry::RetryLayer;
use crate::service::root_span::RootSpanLayer;
use crate::service::trace_propagation::TracePropagationLayer;
use crate::service::user_agent::UserAgentLayer;
use crate::service::wait_for_spans::{WaitForSpansBody, WaitForSpansLayer};
use crate::service::{Identity, Layer, ServiceBuilder, Stack};
use crate::{Agent, Builder, Request, RequestBuilder, Response};
use arc_swap::ArcSwap;
use bytes::Bytes;
use conjure_error::Error;
use conjure_runtime_config::ServiceConfig;
use http::Method;
use refreshable::Subscription;
use std::error;
use std::sync::Arc;

macro_rules! layers {
    () => { Identity };
    ($layer:ty, $($rem:tt)*) => { Stack<$layer, layers!($($rem)*)> };
}

type BaseLayer = layers!(
    MetricsLayer,
    RequestLayer,
    ResponseLayer,
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
    pub(crate) fn new<U>(builder: &Builder<U>) -> Result<ClientState<T>, Error>
    where
        U: BuildRawClient<RawClient = T>,
    {
        let service = builder.get_service().expect("service not set");

        let mut user_agent = builder
            .get_user_agent()
            .cloned()
            .expect("user agent not set");
        user_agent.push_agent(Agent::new("conjure-runtime", env!("CARGO_PKG_VERSION")));

        let client = builder.get_raw_client_builder().build_raw_client(builder)?;

        let proxy = ProxyConfig::from_config(builder.get_proxy())?;

        let service = ServiceBuilder::new()
            .layer(MetricsLayer::new(service, builder))
            .layer(RequestLayer)
            .layer(ResponseLayer)
            .layer(RootSpanLayer)
            .layer(RetryLayer::new(builder))
            .layer(HttpErrorLayer::new(builder))
            .layer(WaitForSpansLayer)
            .layer(NodeSelectorLayer::new(service, builder)?)
            .layer(NodeUriLayer)
            .layer(NodeMetricsLayer)
            .layer(ProxyLayer::new(&proxy))
            .layer(TracePropagationLayer)
            .layer(UserAgentLayer::new(&user_agent))
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
    state: Arc<ArcSwap<ClientState<T>>>,
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
    pub fn builder() -> Builder {
        Builder::new()
    }
}

impl<T> Client<T> {
    pub(crate) fn new(
        state: Arc<ArcSwap<ClientState<T>>>,
        subscription: Option<Subscription<ServiceConfig, Error>>,
    ) -> Client<T> {
        Client {
            state,
            subscription: subscription.map(Arc::new),
        }
    }

    /// Returns a new request builder.
    ///
    /// The `pattern` argument is a template for the request path. The `param` method on the builder is used to fill
    /// in the parameters in the pattern with dynamic values.
    pub fn request(&self, method: Method, pattern: &'static str) -> RequestBuilder<'_, T> {
        RequestBuilder::new(self, method, pattern)
    }

    /// Returns a new builder for a GET request.
    pub fn get(&self, pattern: &'static str) -> RequestBuilder<'_, T> {
        self.request(Method::GET, pattern)
    }

    /// Returns a new builder for a POST request.
    pub fn post(&self, pattern: &'static str) -> RequestBuilder<'_, T> {
        self.request(Method::POST, pattern)
    }

    /// Returns a new builder for a PUT request.
    pub fn put(&self, pattern: &'static str) -> RequestBuilder<'_, T> {
        self.request(Method::PUT, pattern)
    }

    /// Returns a new builder for a DELETE request.
    pub fn delete(&self, pattern: &'static str) -> RequestBuilder<'_, T> {
        self.request(Method::DELETE, pattern)
    }

    /// Returns a new builder for a PATCH request.
    pub fn patch(&self, pattern: &'static str) -> RequestBuilder<'_, T> {
        self.request(Method::PATCH, pattern)
    }
}

impl<T, B> Client<T>
where
    T: Service<http::Request<RawBody>, Response = http::Response<B>> + 'static + Sync + Send,
    T::Error: Into<Box<dyn error::Error + Sync + Send>>,
    T::Future: Send,
    B: http_body::Body<Data = Bytes> + 'static + Send,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    pub(crate) async fn send(&self, request: Request<'_>) -> Result<Response<B>, Error> {
        // split into 2 statements to avoid holding onto the state while awaiting the future
        let future = self.state.load().service.call(request);
        future.await
    }
}
