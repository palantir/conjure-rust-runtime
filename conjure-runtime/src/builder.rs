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
//! The client builder.
use crate::blocking;
use crate::client::ClientState;
use crate::config::{ProxyConfig, SecurityConfig, ServiceConfig};
use crate::raw::{BuildRawClient, DefaultRawClientBuilder};
use crate::weak_cache::Cached;
use crate::{Client, HostMetricsRegistry, UserAgent};
use arc_swap::ArcSwap;
use conjure_error::Error;
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use url::Url;
use witchcraft_metrics::MetricRegistry;

const MESH_PREFIX: &str = "mesh-";

/// A builder to construct [`Client`]s and [`blocking::Client`]s.
pub struct Builder<T = Complete>(T);

/// The service builder stage.
pub struct ServiceStage(());

/// The user agent builder stage.
pub struct UserAgentStage {
    service: String,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct CachedConfig {
    service: String,
    user_agent: UserAgent,
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
    rng_seed: Option<u64>,
    override_host_index: Option<usize>,
}

#[derive(Clone)]
pub(crate) struct UncachedConfig<T> {
    pub(crate) metrics: Option<Arc<MetricRegistry>>,
    pub(crate) host_metrics: Option<Arc<HostMetricsRegistry>>,
    pub(crate) blocking_handle: Option<Handle>,
    pub(crate) raw_client_builder: T,
}

/// The complete builder stage.
pub struct Complete<T = DefaultRawClientBuilder> {
    cached: CachedConfig,
    uncached: UncachedConfig<T>,
}

impl Default for Builder<ServiceStage> {
    #[inline]
    fn default() -> Self {
        Builder::new()
    }
}

impl Builder<ServiceStage> {
    /// Creates a new builder with default settings.
    #[inline]
    pub fn new() -> Self {
        Builder(ServiceStage(()))
    }

    /// Sets the name of the service this client will communicate with.
    ///
    /// This is used in logging and metrics to allow differentiation between different clients.
    #[inline]
    pub fn service(self, service: &str) -> Builder<UserAgentStage> {
        Builder(UserAgentStage {
            service: service.to_string(),
        })
    }
}

impl Builder<UserAgentStage> {
    /// Sets the user agent sent by this client.
    #[inline]
    pub fn user_agent(self, user_agent: UserAgent) -> Builder {
        Builder(Complete {
            cached: CachedConfig {
                service: self.0.service,
                user_agent,
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
                rng_seed: None,
                override_host_index: None,
            },
            uncached: UncachedConfig {
                metrics: None,
                host_metrics: None,
                blocking_handle: None,
                raw_client_builder: DefaultRawClientBuilder,
            },
        })
    }
}

#[cfg(test)]
impl Builder {
    pub(crate) fn for_test() -> Self {
        use crate::Agent;

        Builder::new()
            .service("test")
            .user_agent(UserAgent::new(Agent::new("test", "0.0.0")))
    }
}

impl<T> Builder<Complete<T>> {
    pub(crate) fn cached_config(&self) -> &CachedConfig {
        &self.0.cached
    }

    /// Applies configuration settings from a `ServiceConfig` to the builder.
    #[inline]
    pub fn from_config(mut self, config: &ServiceConfig) -> Self {
        self = self.uris(config.uris().to_vec());

        if let Some(security) = config.security() {
            self = self.security(security.clone());
        }

        if let Some(proxy) = config.proxy() {
            self = self.proxy(proxy.clone());
        }

        if let Some(connect_timeout) = config.connect_timeout() {
            self = self.connect_timeout(connect_timeout);
        }

        if let Some(read_timeout) = config.read_timeout() {
            self = self.read_timeout(read_timeout);
        }

        if let Some(write_timeout) = config.write_timeout() {
            self = self.write_timeout(write_timeout);
        }

        if let Some(backoff_slot_size) = config.backoff_slot_size() {
            self = self.backoff_slot_size(backoff_slot_size);
        }

        if let Some(max_num_retries) = config.max_num_retries() {
            self = self.max_num_retries(max_num_retries);
        }

        self
    }

    /// Returns the builder's configured service name.
    #[inline]
    pub fn get_service(&self) -> &str {
        &self.0.cached.service
    }

    /// Returns the builder's configured user agent.
    #[inline]
    pub fn get_user_agent(&self) -> &UserAgent {
        &self.0.cached.user_agent
    }

    /// Appends a URI to the URIs list.
    ///
    /// Defaults to an empty list.
    #[inline]
    pub fn uri(mut self, uri: Url) -> Self {
        self.0.cached.uris.push(uri);
        self
    }

    /// Sets the URIs list.
    ///
    /// Defaults to an empty list.
    #[inline]
    pub fn uris(mut self, uris: Vec<Url>) -> Self {
        self.0.cached.uris = uris;
        self
    }

    /// Returns the builder's configured URIs list.
    #[inline]
    pub fn get_uris(&self) -> &[Url] {
        &self.0.cached.uris
    }

    /// Sets the security configuration.
    ///
    /// Defaults to an empty configuration.
    #[inline]
    pub fn security(mut self, security: SecurityConfig) -> Self {
        self.0.cached.security = security;
        self
    }

    /// Returns the builder's configured security configuration.
    #[inline]
    pub fn get_security(&self) -> &SecurityConfig {
        &self.0.cached.security
    }

    /// Sets the proxy configuration.
    ///
    /// Defaults to `ProxyConfig::Direct` (i.e. no proxy).
    #[inline]
    pub fn proxy(mut self, proxy: ProxyConfig) -> Self {
        self.0.cached.proxy = proxy;
        self
    }

    /// Returns the builder's configured proxy configuration.
    #[inline]
    pub fn get_proxy(&self) -> &ProxyConfig {
        &self.0.cached.proxy
    }

    /// Sets the connect timeout.
    ///
    /// Defaults to 10 seconds.
    #[inline]
    pub fn connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.0.cached.connect_timeout = connect_timeout;
        self
    }

    /// Returns the builder's configured connect timeout.
    #[inline]
    pub fn get_connect_timeout(&self) -> Duration {
        self.0.cached.connect_timeout
    }

    /// Sets the read timeout.
    ///
    /// This timeout applies to socket-level read attempts.
    ///
    /// Defaults to 5 minutes.
    #[inline]
    pub fn read_timeout(mut self, read_timeout: Duration) -> Self {
        self.0.cached.read_timeout = read_timeout;
        self
    }

    /// Returns the builder's configured read timeout.
    #[inline]
    pub fn get_read_timeout(&self) -> Duration {
        self.0.cached.read_timeout
    }

    /// Sets the write timeout.
    ///
    /// This timeout applies to socket-level write attempts.
    ///
    /// Defaults to 5 minutes.
    #[inline]
    pub fn write_timeout(mut self, write_timeout: Duration) -> Self {
        self.0.cached.write_timeout = write_timeout;
        self
    }

    /// Returns the builder's configured write timeout.
    #[inline]
    pub fn get_write_timeout(&self) -> Duration {
        self.0.cached.write_timeout
    }

    /// Sets the backoff slot size.
    ///
    /// This is the upper bound on the initial delay before retrying a request. It grows exponentially as additional
    /// attempts are made for a given request.
    ///
    /// Defaults to 250 milliseconds.
    #[inline]
    pub fn backoff_slot_size(mut self, backoff_slot_size: Duration) -> Self {
        self.0.cached.backoff_slot_size = backoff_slot_size;
        self
    }

    /// Returns the builder's configured backoff slot size.
    #[inline]
    pub fn get_backoff_slot_size(&self) -> Duration {
        self.0.cached.backoff_slot_size
    }

    /// Sets the maximum number of times a request attempt will be retried before giving up.
    ///
    /// Defaults to 4.
    #[inline]
    pub fn max_num_retries(mut self, max_num_retries: u32) -> Self {
        self.0.cached.max_num_retries = max_num_retries;
        self
    }

    /// Returns the builder's configured maximum number of retries.
    #[inline]
    pub fn get_max_num_retries(&self) -> u32 {
        self.0.cached.max_num_retries
    }

    /// Sets the client's internal rate limiting behavior.
    ///
    /// Defaults to `ClientQos::Enabled`.
    #[inline]
    pub fn client_qos(mut self, client_qos: ClientQos) -> Self {
        self.0.cached.client_qos = client_qos;
        self
    }

    /// Returns the builder's configured internal rate limiting behavior.
    #[inline]
    pub fn get_client_qos(&self) -> ClientQos {
        self.0.cached.client_qos
    }

    /// Sets the client's behavior in response to a QoS error from the server.
    ///
    /// Defaults to `ServerQos::AutomaticRetry`.
    #[inline]
    pub fn server_qos(mut self, server_qos: ServerQos) -> Self {
        self.0.cached.server_qos = server_qos;
        self
    }

    /// Returns the builder's configured server QoS behavior.
    #[inline]
    pub fn get_server_qos(&self) -> ServerQos {
        self.0.cached.server_qos
    }

    /// Sets the client's behavior in response to a service error from the server.
    ///
    /// Defaults to `ServiceError::WrapInNewError`.
    #[inline]
    pub fn service_error(mut self, service_error: ServiceError) -> Self {
        self.0.cached.service_error = service_error;
        self
    }

    /// Returns the builder's configured service error handling behavior.
    #[inline]
    pub fn get_service_error(&self) -> ServiceError {
        self.0.cached.service_error
    }

    /// Sets the client's behavior to determine if a request is idempotent or not.
    ///
    /// Only idempotent requests will be retried.
    ///
    /// Defaults to `Idempotency::ByMethod`.
    #[inline]
    pub fn idempotency(mut self, idempotency: Idempotency) -> Self {
        self.0.cached.idempotency = idempotency;
        self
    }

    /// Returns the builder's configured idempotency handling behavior.
    #[inline]
    pub fn get_idempotency(&self) -> Idempotency {
        self.0.cached.idempotency
    }

    /// Sets the client's strategy for selecting a node for a request.
    ///
    /// Defaults to `NodeSelectionStrategy::PinUntilError`.
    #[inline]
    pub fn node_selection_strategy(
        mut self,
        node_selection_strategy: NodeSelectionStrategy,
    ) -> Self {
        self.0.cached.node_selection_strategy = node_selection_strategy;
        self
    }

    /// Returns the builder's configured node selection strategy.
    #[inline]
    pub fn get_node_selection_strategy(&self) -> NodeSelectionStrategy {
        self.0.cached.node_selection_strategy
    }

    /// Sets the metric registry used to register client metrics.
    ///
    /// Defaults to no registry.
    #[inline]
    pub fn metrics(mut self, metrics: Arc<MetricRegistry>) -> Self {
        self.0.uncached.metrics = Some(metrics);
        self
    }

    /// Returns the builder's configured metric registry.
    #[inline]
    pub fn get_metrics(&self) -> Option<&Arc<MetricRegistry>> {
        self.0.uncached.metrics.as_ref()
    }

    /// Sets the host metrics registry used to track host performance.
    ///
    /// Defaults to no registry.
    #[inline]
    pub fn host_metrics(mut self, host_metrics: Arc<HostMetricsRegistry>) -> Self {
        self.0.uncached.host_metrics = Some(host_metrics);
        self
    }

    /// Returns the builder's configured host metrics registry.
    #[inline]
    pub fn get_host_metrics(&self) -> Option<&Arc<HostMetricsRegistry>> {
        self.0.uncached.host_metrics.as_ref()
    }

    /// Sets a seed used to initialize the client's random number generators.
    ///
    /// Several components of the client rely on entropy. If set, the client will use the seed to initialize its
    /// internal random number generators such that clients created with the same configuration will produce the same
    /// behavior.
    ///
    /// Defaults to no seed.
    #[inline]
    pub fn rng_seed(mut self, rng_seed: u64) -> Self {
        self.0.cached.rng_seed = Some(rng_seed);
        self
    }

    /// Returns the builder's configured RNG seed.
    #[inline]
    pub fn get_rng_seed(&self) -> Option<u64> {
        self.0.cached.rng_seed
    }

    /// Returns the `Handle` to the tokio `Runtime` to be used by blocking clients.
    ///
    /// This has no effect on async clients.
    ///
    /// Defaults to a `conjure-runtime` internal `Runtime`.
    #[inline]
    pub fn blocking_handle(mut self, blocking_handle: Handle) -> Self {
        self.0.uncached.blocking_handle = Some(blocking_handle);
        self
    }

    /// Returns the builder's configured blocking handle.
    #[inline]
    pub fn get_blocking_handle(&self) -> Option<&Handle> {
        self.0.uncached.blocking_handle.as_ref()
    }

    /// Overrides the `hostIndex` field included in metrics.
    #[inline]
    pub fn override_host_index(mut self, override_host_index: usize) -> Self {
        self.0.cached.override_host_index = Some(override_host_index);
        self
    }

    /// Returns the builder's `hostIndex` override.
    #[inline]
    pub fn get_override_host_index(&self) -> Option<usize> {
        self.0.cached.override_host_index
    }

    /// Sets the raw client builder.
    ///
    /// Defaults to `DefaultRawClientBuilder`.
    #[inline]
    pub fn raw_client_builder<U>(self, raw_client_builder: U) -> Builder<Complete<U>> {
        Builder(Complete {
            cached: self.0.cached,
            uncached: UncachedConfig {
                metrics: self.0.uncached.metrics,
                host_metrics: self.0.uncached.host_metrics,
                blocking_handle: self.0.uncached.blocking_handle,
                raw_client_builder,
            },
        })
    }

    /// Returns the builder's configured raw client builder.
    #[inline]
    pub fn get_raw_client_builder(&self) -> &T {
        &self.0.uncached.raw_client_builder
    }

    pub(crate) fn mesh_mode(&self) -> bool {
        self.0
            .cached
            .uris
            .iter()
            .any(|uri| uri.scheme().starts_with(MESH_PREFIX))
    }

    pub(crate) fn postprocessed_uris(&self) -> Result<Cow<'_, [Url]>, Error> {
        if self.mesh_mode() {
            if self.0.cached.uris.len() != 1 {
                return Err(Error::internal_safe("mesh mode expects exactly one URI")
                    .with_safe_param("uris", &self.0.cached.uris));
            }

            let uri = self.0.cached.uris[0]
                .as_str()
                .strip_prefix(MESH_PREFIX)
                .unwrap()
                .parse()
                .unwrap();

            Ok(Cow::Owned(vec![uri]))
        } else {
            Ok(Cow::Borrowed(&self.0.cached.uris))
        }
    }
}

impl<T> Builder<Complete<T>>
where
    T: BuildRawClient,
{
    /// Creates a new `Client`.
    pub fn build(&self) -> Result<Client<T::RawClient>, Error> {
        let state = ClientState::new(self)?;
        Ok(Client::new(
            Arc::new(ArcSwap::new(Arc::new(Cached::uncached(state)))),
            None,
        ))
    }

    /// Creates a new `blocking::Client`.
    pub fn build_blocking(&self) -> Result<blocking::Client<T::RawClient>, Error> {
        self.build().map(|client| blocking::Client {
            client,
            handle: self.0.uncached.blocking_handle.clone(),
        })
    }
}

/// Specifies the beahavior of client-side sympathetic rate limiting.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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
