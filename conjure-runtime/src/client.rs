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
use crate::raw::{DefaultRawClient, RawBody};
use crate::service::gzip::GzipLayer;
use crate::service::http_error::HttpErrorLayer;
use crate::service::map_error::MapErrorLayer;
use crate::service::metrics::MetricsLayer;
use crate::service::node::{NodeMetricsLayer, NodeSelectorLayer, NodeUriLayer};
use crate::service::proxy::{ProxyConfig, ProxyLayer};
use crate::service::request::RequestLayer;
use crate::service::response::ResponseLayer;
use crate::service::retry::RetryLayer;
use crate::service::span::SpanLayer;
use crate::service::timeout::TimeoutLayer;
use crate::service::trace_propagation::TracePropagationLayer;
use crate::service::user_agent::UserAgentLayer;
use crate::{Agent, Builder, Request, RequestBuilder, Response};
use arc_swap::ArcSwap;
use bytes::Bytes;
use conjure_error::Error;
use conjure_runtime_config::ServiceConfig;
use http::Method;
use refreshable::Subscription;
use std::error;
use std::sync::Arc;
use tower::layer::{Identity, Layer, Stack};
use tower::{Service, ServiceBuilder, ServiceExt};

macro_rules! layers {
    () => { Identity };
    ($layer:ty, $($rem:tt)*) => { Stack<$layer, layers!($($rem)*)> };
}

// NB: The types here are declared in reverse order compared to the ServiceBuilder declarations below since it was
// easier to define the macro that way.
type AttemptLayer = layers!(
    MapErrorLayer,
    GzipLayer,
    UserAgentLayer,
    TracePropagationLayer,
    ProxyLayer,
    NodeMetricsLayer,
    NodeUriLayer,
    NodeSelectorLayer,
    SpanLayer,
    HttpErrorLayer,
);

type BaseLayer = layers!(
    RetryLayer<AttemptLayer>,
    TimeoutLayer,
    ResponseLayer,
    RequestLayer,
    MetricsLayer,
);

pub(crate) struct ClientState<T> {
    client: T,
    layer: BaseLayer,
}

impl ClientState<DefaultRawClient> {
    pub(crate) fn new(builder: &Builder) -> Result<ClientState<DefaultRawClient>, Error> {
        let service = builder.service.as_ref().expect("service not set");

        let mut user_agent = builder.user_agent.clone().expect("user agent not set");
        user_agent.push_agent(Agent::new("conjure-runtime", env!("CARGO_PKG_VERSION")));

        let client = DefaultRawClient::new(service, builder)?;

        let proxy = ProxyConfig::from_config(&builder.proxy)?;

        let attempt_layer = ServiceBuilder::new()
            .layer(HttpErrorLayer::new(builder))
            .layer(SpanLayer)
            .layer(NodeSelectorLayer::new(service, builder))
            .layer(NodeUriLayer)
            .layer(NodeMetricsLayer)
            .layer(ProxyLayer::new(&proxy))
            .layer(TracePropagationLayer)
            .layer(UserAgentLayer::new(&user_agent))
            .layer(GzipLayer)
            .layer(MapErrorLayer)
            .into_inner();

        let layer = ServiceBuilder::new()
            .layer(MetricsLayer::new(service, builder))
            .layer(RequestLayer)
            .layer(ResponseLayer)
            .layer(TimeoutLayer::new(builder.request_timeout))
            .layer(RetryLayer::new(Arc::new(attempt_layer), builder))
            .into_inner();

        Ok(ClientState { client, layer })
    }
}

impl<T, B> ClientState<T>
where
    T: Service<http::Request<RawBody>, Response = http::Response<B>> + Clone + 'static + Send,
    T::Error: Into<Box<dyn error::Error + Sync + Send>>,
    T::Future: Send,
    B: http_body::Body<Data = Bytes> + 'static + Sync + Send,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    async fn send(&self, request: Request<'_>) -> Result<Response, Error> {
        self.layer.layer(self.client.clone()).oneshot(request).await
    }
}

/// An asynchronous HTTP client to a remote service.
///
/// It implements the Conjure `AsyncClient` trait, but also offers a "raw" request interface for use with services that
/// don't provide Conjure service definitions.
#[derive(Clone)]
pub struct Client<T = DefaultRawClient> {
    state: Arc<ArcSwap<ClientState<T>>>,
    _subscription: Option<Arc<Subscription<ServiceConfig, Error>>>,
}

impl Client {
    /// Creates a new `Builder` for clients.
    pub fn builder() -> Builder {
        Builder::new()
    }

    pub(crate) fn new(
        state: Arc<ArcSwap<ClientState<DefaultRawClient>>>,
        subscription: Option<Subscription<ServiceConfig, Error>>,
    ) -> Client {
        Client {
            state,
            _subscription: subscription.map(Arc::new),
        }
    }

    /// Returns a new request builder.
    ///
    /// The `pattern` argument is a template for the request path. The `param` method on the builder is used to fill
    /// in the parameters in the pattern with dynamic values.
    pub fn request(&self, method: Method, pattern: &'static str) -> RequestBuilder<'_> {
        RequestBuilder::new(self, method, pattern)
    }

    /// Returns a new builder for a GET request.
    pub fn get(&self, pattern: &'static str) -> RequestBuilder<'_> {
        self.request(Method::GET, pattern)
    }

    /// Returns a new builder for a POST request.
    pub fn post(&self, pattern: &'static str) -> RequestBuilder<'_> {
        self.request(Method::POST, pattern)
    }

    /// Returns a new builder for a PUT request.
    pub fn put(&self, pattern: &'static str) -> RequestBuilder<'_> {
        self.request(Method::PUT, pattern)
    }

    /// Returns a new builder for a DELETE request.
    pub fn delete(&self, pattern: &'static str) -> RequestBuilder<'_> {
        self.request(Method::DELETE, pattern)
    }

    /// Returns a new builder for a PATCH request.
    pub fn patch(&self, pattern: &'static str) -> RequestBuilder<'_> {
        self.request(Method::PATCH, pattern)
    }

    pub(crate) async fn send(&self, request: Request<'_>) -> Result<Response, Error> {
        self.state.load().send(request).await
    }
}
