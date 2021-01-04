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
use crate::raw::{BuildRawClient, DefaultRawClientBuilder};
use crate::{Client, HostMetricsRegistry, UserAgent};
use arc_swap::ArcSwap;
use conjure_error::Error;
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use witchcraft_metrics::MetricRegistry;

const MESH_PREFIX: &str = "mesh-";

/// A builder to construct `Client`s and `blocking::Client`s.
pub struct Builder<T = DefaultRawClientBuilder> {
    service: Option<String>,
    user_agent: Option<UserAgent>,
    uris: Vec<Url>,
    security: SecurityConfig,
    proxy: ProxyConfig,
    connect_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
    backoff_slot_size: Duration,
    max_num_retries: u32,
    client_qos: ClientQos,
    server_qos: ServerQos,
    service_error: ServiceError,
    idempotency: Idempotency,
    node_selection_strategy: NodeSelectionStrategy,
    metrics: Option<Arc<MetricRegistry>>,
    host_metrics: Option<Arc<HostMetricsRegistry>>,
    rng_seed: Option<u64>,
    raw_client_builder: T,
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
            read_timeout: Duration::from_secs(5 * 60),
            write_timeout: Duration::from_secs(5 * 60),
            backoff_slot_size: Duration::from_millis(250),
            max_num_retries: 4,
            client_qos: ClientQos::Enabled,
            server_qos: ServerQos::AutomaticRetry,
            service_error: ServiceError::WrapInNewError,
            idempotency: Idempotency::ByMethod,
            node_selection_strategy: NodeSelectionStrategy::PinUntilError,
            metrics: None,
            host_metrics: None,
            rng_seed: None,
            raw_client_builder: DefaultRawClientBuilder,
        }
    }
}

impl<T> Builder<T> {
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

        if let Some(read_timeout) = config.read_timeout() {
            self.read_timeout(read_timeout);
        }

        if let Some(write_timeout) = config.write_timeout() {
            self.write_timeout(write_timeout);
        }

        if let Some(backoff_slot_size) = config.backoff_slot_size() {
            self.backoff_slot_size(backoff_slot_size);
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

    /// Returns the builder's configured service name.
    pub fn get_service(&self) -> Option<&str> {
        self.service.as_deref()
    }

    /// Sets the user agent sent by this client.
    ///
    /// Required.
    pub fn user_agent(&mut self, user_agent: UserAgent) -> &mut Self {
        self.user_agent = Some(user_agent);
        self
    }

    /// Returns the builder's configured user agent.
    pub fn get_user_agent(&self) -> Option<&UserAgent> {
        self.user_agent.as_ref()
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

    /// Returns the builder's configured URIs list.
    pub fn get_uris(&self) -> &[Url] {
        &self.uris
    }

    /// Sets the security configuration.
    ///
    /// Defaults to an empty configuration.
    pub fn security(&mut self, security: SecurityConfig) -> &mut Self {
        self.security = security;
        self
    }

    /// Returns the builder's configured security configuration.
    pub fn get_security(&self) -> &SecurityConfig {
        &self.security
    }

    /// Sets the proxy configuration.
    ///
    /// Defaults to `ProxyConfig::Direct` (i.e. no proxy).
    pub fn proxy(&mut self, proxy: ProxyConfig) -> &mut Self {
        self.proxy = proxy;
        self
    }

    /// Returns the builder's configured proxy configuration.
    pub fn get_proxy(&self) -> &ProxyConfig {
        &self.proxy
    }

    /// Sets the connect timeout.
    ///
    /// Defaults to 10 seconds.
    pub fn connect_timeout(&mut self, connect_timeout: Duration) -> &mut Self {
        self.connect_timeout = connect_timeout;
        self
    }

    /// Returns the builder's configured connect timeout.
    pub fn get_connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    /// Sets the read timeout.
    ///
    /// This timeout applies to socket-level read attempts.
    ///
    /// Defaults to 5 minutes.
    pub fn read_timeout(&mut self, read_timeout: Duration) -> &mut Self {
        self.read_timeout = read_timeout;
        self
    }

    /// Returns the builder's configured read timeout.
    pub fn get_read_timeout(&self) -> Duration {
        self.read_timeout
    }

    /// Sets the write timeout.
    ///
    /// This timeout applies to socket-level write attempts.
    ///
    /// Defaults to 5 minutes.
    pub fn write_timeout(&mut self, write_timeout: Duration) -> &mut Self {
        self.write_timeout = write_timeout;
        self
    }

    /// Returns the builder's configured write timeout.
    pub fn get_write_timeout(&self) -> Duration {
        self.write_timeout
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

    /// Returns the builder's configured backoff slot size.
    pub fn get_backoff_slot_size(&self) -> Duration {
        self.backoff_slot_size
    }

    /// Sets the maximum number of times a request attempt will be retried before giving up.
    ///
    /// Defaults to 4.
    pub fn max_num_retries(&mut self, max_num_retries: u32) -> &mut Self {
        self.max_num_retries = max_num_retries;
        self
    }

    /// Returns the builder's configured maximum number of retries.
    pub fn get_max_num_retries(&self) -> u32 {
        self.max_num_retries
    }

    /// Sets the client's internal rate limiting behavior.
    ///
    /// Defaults to `ClientQos::Enabled`.
    pub fn client_qos(&mut self, client_qos: ClientQos) -> &mut Self {
        self.client_qos = client_qos;
        self
    }

    /// Returns the builder's configured internal rate limiting behavior.
    pub fn get_client_qos(&self) -> ClientQos {
        self.client_qos
    }

    /// Sets the client's behavior in response to a QoS error from the server.
    ///
    /// Defaults to `ServerQos::AutomaticRetry`.
    pub fn server_qos(&mut self, server_qos: ServerQos) -> &mut Self {
        self.server_qos = server_qos;
        self
    }

    /// Returns the builder's configured server QoS behavior.
    pub fn get_server_qos(&self) -> ServerQos {
        self.server_qos
    }

    /// Sets the client's behavior in response to a service error from the server.
    ///
    /// Defaults to `ServiceError::WrapInNewError`.
    pub fn service_error(&mut self, service_error: ServiceError) -> &mut Self {
        self.service_error = service_error;
        self
    }

    /// Returns the builder's configured service error handling behavior.
    pub fn get_service_error(&self) -> ServiceError {
        self.service_error
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

    /// Returns the builder's configured idempotency handling behavior.
    pub fn get_idempotency(&self) -> Idempotency {
        self.idempotency
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

    /// Returns the builder's configured node selection strategy.
    pub fn get_node_selection_strategy(&self) -> NodeSelectionStrategy {
        self.node_selection_strategy
    }

    /// Sets the metric registry used to register client metrics.
    ///
    /// Defaults to no registry.
    pub fn metrics(&mut self, metrics: Arc<MetricRegistry>) -> &mut Self {
        self.metrics = Some(metrics);
        self
    }

    /// Returns the builder's configured metric registry.
    pub fn get_metrics(&self) -> Option<&Arc<MetricRegistry>> {
        self.metrics.as_ref()
    }

    /// Sets the host metrics registry used to track host performance.
    ///
    /// Defaults to no registry.
    pub fn host_metrics(&mut self, host_metrics: Arc<HostMetricsRegistry>) -> &mut Self {
        self.host_metrics = Some(host_metrics);
        self
    }

    /// Returns the builder's configured host metrics registry.
    pub fn get_host_metrics(&self) -> Option<&Arc<HostMetricsRegistry>> {
        self.host_metrics.as_ref()
    }

    /// Sets a seed used to initialize the client's random number generators.
    ///
    /// Several components of the client rely on entropy. If set, the client will use the seed to initialize its
    /// internal random number generators such that clients created with the same configuration will produce the same
    /// behavior.
    ///
    /// Defaults to no seed.
    pub fn rng_seed(&mut self, rng_seed: u64) -> &mut Self {
        self.rng_seed = Some(rng_seed);
        self
    }

    /// Returns the builder's configured RNG seed.
    pub fn get_rng_seed(&self) -> Option<u64> {
        self.rng_seed
    }

    /// Sets the raw client builder.
    ///
    /// Defaults to `DefaultRawClientBuilder`.
    pub fn with_raw_client_builder<U>(self, raw_client_builder: U) -> Builder<U> {
        Builder {
            service: self.service,
            user_agent: self.user_agent,
            uris: self.uris,
            security: self.security,
            proxy: self.proxy,
            connect_timeout: self.connect_timeout,
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
            backoff_slot_size: self.backoff_slot_size,
            max_num_retries: self.max_num_retries,
            client_qos: self.client_qos,
            server_qos: self.server_qos,
            service_error: self.service_error,
            idempotency: self.idempotency,
            node_selection_strategy: self.node_selection_strategy,
            metrics: self.metrics,
            host_metrics: self.host_metrics,
            rng_seed: self.rng_seed,
            raw_client_builder,
        }
    }

    /// Returns the builder's configured raw client builder.
    pub fn get_raw_client_builder(&self) -> &T {
        &self.raw_client_builder
    }

    pub(crate) fn mesh_mode(&self) -> bool {
        self.uris
            .iter()
            .any(|uri| uri.scheme().starts_with(MESH_PREFIX))
    }

    pub(crate) fn postprocessed_uris(&self) -> Result<Cow<'_, [Url]>, Error> {
        if self.mesh_mode() {
            if self.uris.len() != 1 {
                return Err(Error::internal_safe("mesh mode expects exactly one URI")
                    .with_safe_param("uris", &self.uris));
            }

            let uri = self.uris[0]
                .as_str()
                .strip_prefix(MESH_PREFIX)
                .unwrap()
                .parse()
                .unwrap();

            Ok(Cow::Owned(vec![uri]))
        } else {
            Ok(Cow::Borrowed(&self.uris))
        }
    }
}

impl<T> Builder<T>
where
    T: BuildRawClient,
{
    /// Creates a new `Client`.
    ///
    /// # Panics
    ///
    /// Panics if `service` or `user_agent` is not set.
    pub fn build(&self) -> Result<Client<T::RawClient>, Error> {
        let state = ClientState::new(self)?;
        Ok(Client::new(Arc::new(ArcSwap::new(Arc::new(state))), None))
    }

    /// Creates a new `blocking::Client`.
    ///
    /// # Panics
    ///
    /// Panics if `service` or `user_agent` is not set.
    pub fn build_blocking(&self) -> Result<blocking::Client<T::RawClient>, Error> {
        self.build().map(blocking::Client)
    }
}

/// Specifies the beahavior of client-side sympathetic rate limiting.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClientQos {
    /// Enable client side rate limiting.
    ///
    /// This is the default behavior.
    Enabled,

    /// Disables client-side rate limiting.
    ///
    /// This should only be used when there are known issues with the interaction between a service's rate limiting
    /// implementation and the client's.
    DangerousDisableSympatheticClientQos,
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

    /// For each new request, select the "next" node (in some unspecified order).
    Balanced,
}
