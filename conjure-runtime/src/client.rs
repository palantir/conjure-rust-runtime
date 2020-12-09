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
use crate::connect::metrics::MetricsConnector;
use crate::connect::proxy::{ProxyConfig, ProxyConnector};
use crate::node_selector::NodeSelector;
use crate::{
    send, Agent, HostMetricsRegistry, HyperBody, Request, RequestBuilder, Response, UserAgent,
};
use arc_swap::ArcSwap;
use conjure_error::Error;
use conjure_runtime_config::{ServiceConfig, ServiceConfigBuilder};
use hyper::client::HttpConnector;
use hyper::header::HeaderValue;
use hyper::Method;
use hyper_openssl::HttpsConnector;
use openssl::ssl::{SslConnector, SslMethod};
use std::borrow::Cow;
use std::sync::{Arc, Weak};
use std::time::Duration;
use witchcraft_log::info;
use witchcraft_metrics::{Meter, MetricId, MetricRegistry, Timer};

// This is pretty arbitrary - I just grabbed it from some Cloudflare blog post.
const TCP_KEEPALIVE: Duration = Duration::from_secs(3 * 60);
// Most servers time out idle connections after 60 seconds, so we'll set the client timeout a bit below that.
const HTTP_KEEPALIVE: Duration = Duration::from_secs(55);

type ConjureConnector = MetricsConnector<HttpsConnector<ProxyConnector<HttpConnector>>>;

pub(crate) struct ClientState {
    pub(crate) client: hyper::Client<ConjureConnector, HyperBody>,
    pub(crate) node_selector: NodeSelector,
    pub(crate) max_num_retries: u32,
    pub(crate) backoff_slot_size: Duration,
    pub(crate) request_timeout: Duration,
    pub(crate) proxy: ProxyConfig,
}

impl ClientState {
    fn from_config(
        service: &str,
        metrics: &Arc<MetricRegistry>,
        host_metrics: &HostMetricsRegistry,
        service_config: &ServiceConfig,
    ) -> Result<ClientState, Error> {
        let service_config = Self::rewrite_for_mesh(service_config)?;

        let mut connector = HttpConnector::new();
        connector.enforce_http(false);
        connector.set_nodelay(true);
        connector.set_keepalive(Some(TCP_KEEPALIVE));
        connector.set_connect_timeout(Some(service_config.connect_timeout()));

        let proxy = ProxyConfig::from_config(service_config.proxy())?;
        let connector = ProxyConnector::new(connector, &proxy);

        let mut ssl = SslConnector::builder(SslMethod::tls()).map_err(Error::internal_safe)?;
        ssl.set_alpn_protos(b"\x02h2\x08http/1.1")
            .map_err(Error::internal_safe)?;

        if let Some(ca_file) = service_config.security().ca_file() {
            ssl.set_ca_file(ca_file).map_err(Error::internal_safe)?;
        }

        let connector =
            HttpsConnector::with_connector(connector, ssl).map_err(Error::internal_safe)?;

        let connector = MetricsConnector::new(connector, metrics, service);

        let client = hyper::Client::builder()
            .pool_idle_timeout(HTTP_KEEPALIVE)
            .build(connector);

        let node_selector = NodeSelector::new(service, host_metrics, &service_config);

        Ok(ClientState {
            client,
            node_selector,
            max_num_retries: service_config.max_num_retries(),
            backoff_slot_size: service_config.backoff_slot_size(),
            request_timeout: service_config.request_timeout(),
            proxy,
        })
    }

    fn rewrite_for_mesh(service_config: &ServiceConfig) -> Result<Cow<ServiceConfig>, Error> {
        let prefix = "mesh-";

        let mesh_uris = service_config
            .uris()
            .iter()
            .filter(|uri| uri.scheme().starts_with(prefix))
            .count();

        if mesh_uris == 0 {
            return Ok(Cow::Borrowed(service_config));
        }

        if service_config.uris().len() != 1 {
            return Err(
                Error::internal_safe("exactly one URI must be present in mesh mode")
                    .with_safe_param("uris", service_config.uris()),
            );
        }

        let new_uri = service_config.uris()[0].as_str()[prefix.len()..]
            .parse()
            .unwrap();

        let config = ServiceConfigBuilder::from(service_config.clone())
            .uris(vec![new_uri])
            .max_num_retries(0)
            .build();

        Ok(Cow::Owned(config))
    }
}

pub(crate) struct SharedClient {
    pub(crate) service: String,
    pub(crate) user_agent: HeaderValue,
    pub(crate) state: ArcSwap<ClientState>,
    pub(crate) metrics: Arc<MetricRegistry>,
    pub(crate) host_metrics: Arc<HostMetricsRegistry>,
    pub(crate) response_timer: Arc<Timer>,
    pub(crate) error_meter: Arc<Meter>,
}

/// An asynchronous HTTP client to a remote service.
///
/// It implements the Conjure `AsyncClient` trait, but also offers a "raw" request interface for use with services that
/// don't provide Conjure service definitions.
#[derive(Clone)]
pub struct Client {
    pub(crate) shared: Arc<SharedClient>,
    assume_idempotent: bool,
    propagate_qos_errors: bool,
    propagate_service_errors: bool,
}

impl Client {
    /// Creates a new client.
    ///
    /// The user agent is extended with an agent identifying the name and version of this crate.
    pub fn new(
        service: &str,
        mut user_agent: UserAgent,
        host_metrics: &Arc<HostMetricsRegistry>,
        metrics: &Arc<MetricRegistry>,
        config: &ServiceConfig,
    ) -> Result<Client, Error> {
        user_agent.push_agent(Agent::new("conjure-runtime", env!("CARGO_PKG_VERSION")));

        let state = ClientState::from_config(service, metrics, host_metrics, config)?;

        let response_timer = metrics
            .timer(MetricId::new("client.response").with_tag("service-name", service.to_string()));
        let error_meter = metrics.meter(
            MetricId::new("client.response.error")
                .with_tag("service-name", service.to_string())
                .with_tag("reason", "IOException"),
        );

        Ok(Client {
            shared: Arc::new(SharedClient {
                service: service.to_string(),
                user_agent: HeaderValue::from_str(&user_agent.to_string()).unwrap(),
                state: ArcSwap::new(Arc::new(state)),
                metrics: metrics.clone(),
                host_metrics: host_metrics.clone(),
                response_timer,
                error_meter,
            }),
            assume_idempotent: false,
            propagate_qos_errors: false,
            propagate_service_errors: false,
        })
    }

    /// Configures the client to assume that all requests are idempotent.
    ///
    /// Idempotent operations can be rerun without changing the result of the operation, which allows the client to
    /// safely retry failed requests. By default, GET, HEAD, PUT, and DELETE requests are assumed to be idempotent, but
    /// this method can be used to override that behavior.
    pub fn set_assume_idempotent(&mut self, assume_idempotent: bool) {
        self.assume_idempotent = assume_idempotent;
    }

    /// Returns true if the client is configured to assume all requests are idempotent.
    pub fn assume_idempotent(&self) -> bool {
        self.assume_idempotent
    }

    /// Configures transparent propagation of QoS errors (i.e. 429 and 503 responses).
    ///
    /// By default, the client will automatically retry in response to QoS errors, but if this option is enabled it will
    /// instead immediately return an error which will cause the same response. This is designed for contexts where one
    /// service is proxying a request to another and the developer wants to avoid nested retry loops.
    pub fn set_propagate_qos_errors(&mut self, propagate_qos_errors: bool) {
        self.propagate_qos_errors = propagate_qos_errors;
    }

    /// Returns true if the client will propagate QoS errors.
    pub fn propagate_qos_errors(&self) -> bool {
        self.propagate_qos_errors
    }

    /// Configures transparent propagation of service errors.
    ///
    /// By default, the client will turn service errors returned by the remote server into an internal server error, but
    /// if this option is enabled it will instead return the same service error it received. This is designed for
    /// contexts where one service is proxying a request to another and the developer wants the upstream client to see
    /// downstream errors.
    pub fn set_propagate_service_errors(&mut self, propagate_service_errors: bool) {
        self.propagate_service_errors = propagate_service_errors;
    }

    /// Returns true if the client will propagate service errors.
    pub fn propagate_service_errors(&self) -> bool {
        self.propagate_service_errors
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

    /// Returns a new handle which can be used to dynamically refresh the client's configuration.
    pub fn refresh_handle(&self) -> RefreshHandle {
        RefreshHandle(Arc::downgrade(&self.shared))
    }

    pub(crate) async fn send(&self, request: Request<'_>) -> Result<Response, Error> {
        send::send(self, request).await
    }
}

/// A handle used to update the configuration of a `Client`.
pub struct RefreshHandle(Weak<SharedClient>);

impl RefreshHandle {
    /// Refreshes the client's configuration with a new one.
    ///
    /// If the client has already dropped, this is a no-op.
    pub fn refresh(&self, config: &ServiceConfig) -> Result<(), Error> {
        let client = match self.0.upgrade() {
            Some(client) => client,
            None => return Ok(()),
        };

        let state = ClientState::from_config(
            &client.service,
            &client.metrics,
            &client.host_metrics,
            config,
        )?;
        client.state.store(Arc::new(state));
        info!("reloaded client", safe: { service: client.service });

        Ok(())
    }

    /// Returns `true` if the client associated with the handle has dropped.
    pub fn has_dropped(&self) -> bool {
        // FIXME use strong_count when it stabilizes
        self.0.upgrade().is_none()
    }
}
