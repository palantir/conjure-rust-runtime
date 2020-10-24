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
use crate::blocking;
use crate::client::ClientState;
use crate::config::{ProxyConfig, SecurityConfig, ServiceConfig};
use crate::{Client, HostMetricsRegistry, UserAgent};
use arc_swap::ArcSwap;
use conjure_error::Error;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use witchcraft_metrics::MetricRegistry;

/// A builder to construct `Client`s and `blocking::Client`s.
pub struct Builder {
    pub(crate) service: Option<String>,
    pub(crate) user_agent: Option<UserAgent>,
    pub(crate) uris: Vec<Url>,
    pub(crate) security: SecurityConfig,
    pub(crate) proxy: ProxyConfig,
    pub(crate) connect_timeout: Duration,
    pub(crate) request_timeout: Duration,
    pub(crate) backoff_slot_size: Duration,
    pub(crate) failed_url_cooldown: Duration,
    pub(crate) max_num_retries: u32,
    pub(crate) server_qos: ServerQos,
    pub(crate) service_error: ServiceError,
    pub(crate) idempotency: Idempotency,
    pub(crate) node_selection_strategy: NodeSelectionStrategy,
    pub(crate) metrics: Option<Arc<MetricRegistry>>,
    pub(crate) host_metrics: Option<Arc<HostMetricsRegistry>>,
}

impl Default for Builder {
    fn default() -> Builder {
        Builder::new()
    }
}

impl Builder {
    /// Creates a new builder with default settings.
    pub fn new() -> Builder {
        Builder {
            service: None,
            user_agent: None,
            uris: vec![],
            security: SecurityConfig::builder().build(),
            proxy: ProxyConfig::Direct,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(5 * 60),
            backoff_slot_size: Duration::from_millis(250),
            failed_url_cooldown: Duration::from_millis(250),
            max_num_retries: 3,
            server_qos: ServerQos::AutomaticRetry,
            service_error: ServiceError::WrapInNewError,
            idempotency: Idempotency::ByMethod,
            node_selection_strategy: NodeSelectionStrategy::PinUntilError,
            metrics: None,
            host_metrics: None,
        }
    }

    /// Applies configuration settings from a `ServiceConfig` to the builder.
    pub fn from_config(&mut self, config: &ServiceConfig) -> &mut Self {
        self.uris(config.uris().to_vec());

        if let Some(security) = config.security() {
            self.security(security.clone());
        }

        if let Some(proxy) = config.proxy() {
            self.proxy(proxy.clone());
        }

        if let Some(connect_timeout) = config.connect_timeout() {
            self.connect_timeout(connect_timeout);
        }

        if let Some(request_timeout) = config.request_timeout() {
            self.request_timeout(request_timeout);
        }

        if let Some(backoff_slot_size) = config.backoff_slot_size() {
            self.backoff_slot_size(backoff_slot_size);
        }

        if let Some(failed_url_cooldown) = config.failed_url_cooldown() {
            self.failed_url_cooldown(failed_url_cooldown);
        }

        if let Some(max_num_retries) = config.max_num_retries() {
            self.max_num_retries(max_num_retries);
        }

        self
    }

    /// Sets the name of the service this client will communicate with.
    ///
    /// This is used in logging and metrics to allow differentiation between different clients.
    ///
    /// Required.
    pub fn service(&mut self, service: &str) -> &mut Self {
        self.service = Some(service.to_string());
        self
    }

    /// Sets the user agent sent by this client.
    ///
    /// Required.
    pub fn user_agent(&mut self, user_agent: UserAgent) -> &mut Self {
        self.user_agent = Some(user_agent);
        self
    }

    /// Appends a URI to the URIs list.
    ///
    /// Defaults to an empty list.
    pub fn uri(&mut self, uri: Url) -> &mut Self {
        self.uris.push(uri);
        self
    }

    /// Sets the URIs list.
    ///
    /// Defaults to an empty list.
    pub fn uris(&mut self, uris: Vec<Url>) -> &mut Self {
        self.uris = uris;
        self
    }

    /// Sets the security configuration.
    ///
    /// Defaults to an empty configuration.
    pub fn security(&mut self, security: SecurityConfig) -> &mut Self {
        self.security = security;
        self
    }

    /// Sets the proxy configuration.
    ///
    /// Defaults to `ProxyConfig::Direct` (i.e. no proxy).
    pub fn proxy(&mut self, proxy: ProxyConfig) -> &mut Self {
        self.proxy = proxy;
        self
    }

    /// Sets the connect timeout.
    ///
    /// Defaults to 10 seconds.
    pub fn connect_timeout(&mut self, connect_timeout: Duration) -> &mut Self {
        self.connect_timeout = connect_timeout;
        self
    }

    /// Sets the request timeout.
    ///
    /// This timeout applies to the entire duration of the request, from the first attempt to send it to the last read
    /// of the response body.
    ///
    /// Defaults to 5 minutes.
    pub fn request_timeout(&mut self, request_timeout: Duration) -> &mut Self {
        self.request_timeout = request_timeout;
        self
    }

    /// Sets the backoff slot size.
    ///
    /// This is the upper bound on the initial delay before retrying a request. It grows exponentially as additional
    /// attempts are made for a given request.
    ///
    /// Defaults to 250 milliseconds.
    pub fn backoff_slot_size(&mut self, backoff_slot_size: Duration) -> &mut Self {
        self.backoff_slot_size = backoff_slot_size;
        self
    }

    /// Sets the maximum number of times a request attempt will be retried before giving up.
    ///
    /// Defaults to 3.
    pub fn max_num_retries(&mut self, max_num_retries: u32) -> &mut Self {
        self.max_num_retries = max_num_retries;
        self
    }

    /// Sets the cooldown applied to a node after it fails to respond successfully to a request.
    ///
    /// The node will be skipped while on cooldown unless no other nodes are available.
    ///
    /// Defaults to 250 milliseconds.
    pub fn failed_url_cooldown(&mut self, failed_url_cooldown: Duration) -> &mut Self {
        self.failed_url_cooldown = failed_url_cooldown;
        self
    }

    /// Sets the client's behavior in response to a QoS error from the server.
    ///
    /// Defaults to `ServerQos::AutomaticRetry`.
    pub fn server_qos(&mut self, server_qos: ServerQos) -> &mut Self {
        self.server_qos = server_qos;
        self
    }

    /// Sets the client's behavior in response to a service error from the server.
    ///
    /// Defaults to `ServiceError::WrapInNewError`.
    pub fn service_error(&mut self, service_error: ServiceError) -> &mut Self {
        self.service_error = service_error;
        self
    }

    /// Sets the client's behavior to determine if a request is idempotent or not.
    ///
    /// Only idempotent requests will be retried.
    ///
    /// Defaults to `Idempotency::ByMethod`.
    pub fn idempotency(&mut self, idempotency: Idempotency) -> &mut Self {
        self.idempotency = idempotency;
        self
    }

    /// Sets the client's strategy for selecting a node for a request.
    ///
    /// Defaults to `NodeSelectionStrategy::PinUntilError`.
    pub fn node_selection_strategy(
        &mut self,
        node_selection_strategy: NodeSelectionStrategy,
    ) -> &mut Self {
        self.node_selection_strategy = node_selection_strategy;
        self
    }

    /// Sets the metric registry used to register client metrics.
    ///
    /// Defaults to no registry.
    pub fn metrics(&mut self, metrics: Arc<MetricRegistry>) -> &mut Self {
        self.metrics = Some(metrics);
        self
    }

    /// Sets the host metrics registry used to track host performance.
    ///
    /// Defaults to no registry.
    pub fn host_metrics(&mut self, host_metrics: Arc<HostMetricsRegistry>) -> &mut Self {
        self.host_metrics = Some(host_metrics);
        self
    }

    /// Creates a new `Client`.
    ///
    /// # Panics
    ///
    /// Panics if `service` or `user_agent` is not set.
    pub fn build(&self) -> Result<Client, Error> {
        let state = ClientState::new(self)?;
        Ok(Client::new(Arc::new(ArcSwap::new(Arc::new(state))), None))
    }

    /// Creates a new `blocking::Client`.
    ///
    /// # Panics
    ///
    /// Panics if `service` or `user_agent` is not set.
    pub fn build_blocking(&self) -> Result<blocking::Client, Error> {
        self.build().map(blocking::Client)
    }
}

/// Specifies the behavior of a client in response to a `QoS` error from a server.
///
/// QoS errors have status codes 429 or 503.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerQos {
    /// The client will automatically retry the request when possible in response to a QoS error.
    ///
    /// This is the default behavior.
    AutomaticRetry,

    /// The client will transparently propagate the QoS error without retrying.
    ///
    /// This is designed for use when an upstream service has better context on how to handle a QoS error. Propagating
    /// the error upstream to that service without retrying allows it to handle retry logic internally.
    Propagate429And503ToCaller,
}

/// Specifies the behavior of the client in response to a service error from a server.
///
/// Service errors are encoded as responses with a 4xx or 5xx response code and a body containing a `SerializableError`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceError {
    /// The service error will be propagated as a new internal service error.
    ///
    /// The error's cause will contain the information about the received service error, but the error constructed by
    /// the client will have a different error instance ID, type, etc.
    ///
    /// This is the default behavior.
    WrapInNewError,

    /// The service error will be transparently propagated without change.
    ///
    /// This is designed for use when proxying a request to another node, commonly of the same service. By preserving
    /// the original error's instance ID, type, etc, the upstream service will be able to process the error properly.
    PropagateToCaller,
}

/// Specifies the manner in which the client decides if a request is idempotent or not.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Idempotency {
    /// All requests are assumed to be idempotent.
    Always,

    /// Only requests with HTTP methods defined as idempotent (GET, HEAD, OPTIONS, TRACE, PUT, and DELETE) are assumed
    /// to be idempotent.
    ///
    /// This is the default behavior.
    ByMethod,

    /// No requests are assumed to be idempotent.
    Never,
}

/// Specifies the strategy used to select a node of a service to use for a request attempt.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeSelectionStrategy {
    /// Pin to a single host as long as it continues to successfully respond to requests.
    ///
    /// If the pinned node fails to successfully respond, the client will rotate through the other nodes until it finds
    /// one that can successfully respond and then pin to that new node. The pinned node will also be randomly rotated
    /// periodically to help spread load across the cluster.
    ///
    /// This is the default behavior.
    PinUntilError,

    /// Like `PinUntilError` except that the pinned node is never randomly shuffled.
    PinUntilErrorWithoutReshuffle,
}
