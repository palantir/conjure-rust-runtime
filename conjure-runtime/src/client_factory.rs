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
//! The client factory.
use crate::builder::{CachedConfig, UncachedConfig};
use crate::config::{ServiceConfig, ServicesConfig};
use crate::raw::{DefaultRawClient, DefaultRawClientBuilder};
use crate::weak_cache::WeakCache;
use crate::{blocking, Builder, ClientState};
use crate::{
    Client, ClientQos, HostMetricsRegistry, Idempotency, NodeSelectionStrategy, ServerQos,
    ServiceError, UserAgent,
};
use arc_swap::ArcSwap;
use conjure_error::Error;
use conjure_http::client::{AsyncService, Service};
use refreshable::Refreshable;
use std::sync::Arc;
use tokio::runtime::Handle;
use witchcraft_metrics::MetricRegistry;

const STATE_CACHE_CAPACITY: usize = 10_000;

/// A factory type which can create clients that will live-reload in response to configuration updates.
#[derive(Clone)]
pub struct ClientFactory<T = Complete>(T);

/// The config builder stage.
pub struct ConfigStage(());

/// The user agent builder stage.
pub struct UserAgentStage {
    config: Arc<Refreshable<ServicesConfig, Error>>,
}

#[derive(Clone)]
struct CacheManager {
    uncached_inner: UncachedConfig<DefaultRawClientBuilder>,
    cache: WeakCache<CachedConfig, ClientState<DefaultRawClient>>,
}

impl CacheManager {
    fn uncached(&self) -> &UncachedConfig<DefaultRawClientBuilder> {
        &self.uncached_inner
    }

    fn uncached_mut(&mut self) -> &mut UncachedConfig<DefaultRawClientBuilder> {
        self.cache = WeakCache::new(STATE_CACHE_CAPACITY);
        &mut self.uncached_inner
    }
}

/// The complete builder stage.
#[derive(Clone)]
pub struct Complete {
    config: Arc<Refreshable<ServicesConfig, Error>>,
    user_agent: UserAgent,
    client_qos: ClientQos,
    server_qos: ServerQos,
    service_error: ServiceError,
    idempotency: Idempotency,
    node_selection_strategy: NodeSelectionStrategy,
    cache_manager: CacheManager,
}

impl Default for ClientFactory<ConfigStage> {
    #[inline]
    fn default() -> Self {
        ClientFactory::builder()
    }
}

impl ClientFactory<ConfigStage> {
    /// Creates a new builder to construct a client factory.
    #[inline]
    pub fn builder() -> Self {
        ClientFactory(ConfigStage(()))
    }

    /// Sets the refreshable configuration used for service clients.
    #[inline]
    pub fn config(
        self,
        config: Refreshable<ServicesConfig, Error>,
    ) -> ClientFactory<UserAgentStage> {
        ClientFactory(UserAgentStage {
            config: Arc::new(config),
        })
    }
}

impl ClientFactory<UserAgentStage> {
    /// Sets the user agent sent by clients.
    #[inline]
    pub fn user_agent(self, user_agent: UserAgent) -> ClientFactory {
        ClientFactory(Complete {
            config: self.0.config,
            user_agent,
            client_qos: ClientQos::Enabled,
            server_qos: ServerQos::AutomaticRetry,
            service_error: ServiceError::WrapInNewError,
            idempotency: Idempotency::ByMethod,
            node_selection_strategy: NodeSelectionStrategy::PinUntilError,
            cache_manager: CacheManager {
                uncached_inner: UncachedConfig {
                    metrics: None,
                    host_metrics: None,
                    blocking_handle: None,
                    raw_client_builder: DefaultRawClientBuilder,
                },
                cache: WeakCache::new(STATE_CACHE_CAPACITY),
            },
        })
    }
}

impl ClientFactory {
    /// Sets the user agent sent by clients.
    #[inline]
    pub fn user_agent(mut self, user_agent: UserAgent) -> Self {
        self.0.user_agent = user_agent;
        self
    }

    /// Returns the configured user agent.
    #[inline]
    pub fn get_user_agent(&self) -> &UserAgent {
        &self.0.user_agent
    }

    /// Sets clients' rate limiting behavior.
    ///
    /// Defaults to `ClientQos::Enabled`.
    #[inline]
    pub fn client_qos(mut self, client_qos: ClientQos) -> Self {
        self.0.client_qos = client_qos;
        self
    }

    /// Returns the configured rate limiting behavior
    #[inline]
    pub fn get_client_qos(&self) -> ClientQos {
        self.0.client_qos
    }

    /// Sets clients' behavior in response to a QoS error from the server.
    ///
    /// Defaults to `ServerQos::AutomaticRetry`.
    #[inline]
    pub fn server_qos(mut self, server_qos: ServerQos) -> Self {
        self.0.server_qos = server_qos;
        self
    }

    /// Returns the configured QoS behavior.
    #[inline]
    pub fn get_server_qos(&self) -> ServerQos {
        self.0.server_qos
    }

    /// Sets clients' behavior in response to a service error from the server.
    ///
    /// Defaults to `ServiceError::WrapInNewError`.
    #[inline]
    pub fn service_error(mut self, service_error: ServiceError) -> Self {
        self.0.service_error = service_error;
        self
    }

    /// Returns the configured service error behavior.
    #[inline]
    pub fn get_service_error(&self) -> ServiceError {
        self.0.service_error
    }

    /// Sets clients' behavior to determine if a request is idempotent or not.
    ///
    /// Only idempotent requests will be retried.
    ///
    /// Defaults to `Idempotency::ByMethod`.
    #[inline]
    pub fn idempotency(mut self, idempotency: Idempotency) -> Self {
        self.0.idempotency = idempotency;
        self
    }

    /// Returns the configured idempotency behavior.
    #[inline]
    pub fn get_idempotency(&self) -> Idempotency {
        self.0.idempotency
    }

    /// Sets the clients' strategy for selecting a node for a request.
    ///
    /// Defaults to `NodeSelectionStrategy::PinUntilError`.
    #[inline]
    pub fn node_selection_strategy(
        mut self,
        node_selection_strategy: NodeSelectionStrategy,
    ) -> Self {
        self.0.node_selection_strategy = node_selection_strategy;
        self
    }

    /// Returns the configured node selection strategy.
    #[inline]
    pub fn get_node_selection_strategy(&self) -> NodeSelectionStrategy {
        self.0.node_selection_strategy
    }

    /// Sets the metric registry used to register client metrics.
    ///
    /// Defaults to no registry.
    #[inline]
    pub fn metrics(mut self, metrics: Arc<MetricRegistry>) -> Self {
        self.0.cache_manager.uncached_mut().metrics = Some(metrics);
        self
    }

    /// Returns the configured metrics registry.
    #[inline]
    pub fn get_metrics(&self) -> Option<&Arc<MetricRegistry>> {
        self.0.cache_manager.uncached().metrics.as_ref()
    }

    /// Sets the host metrics registry used to track host performance.
    ///
    /// Defaults to no registry.
    #[inline]
    pub fn host_metrics(mut self, host_metrics: Arc<HostMetricsRegistry>) -> Self {
        self.0.cache_manager.uncached_mut().host_metrics = Some(host_metrics);
        self
    }

    /// Returns the configured host metrics registry.
    #[inline]
    pub fn get_host_metrics(&self) -> Option<&Arc<HostMetricsRegistry>> {
        self.0.cache_manager.uncached().host_metrics.as_ref()
    }

    /// Returns the `Handle` to the tokio `Runtime` to be used by blocking clients.
    ///
    /// This has no effect on async clients.
    ///
    /// Defaults to a `conjure-runtime` internal `Runtime`.
    #[inline]
    pub fn blocking_handle(mut self, blocking_handle: Handle) -> Self {
        self.0.cache_manager.uncached_mut().blocking_handle = Some(blocking_handle);
        self
    }

    /// Returns the configured blocking handle.
    #[inline]
    pub fn get_blocking_handle(&self) -> Option<&Handle> {
        self.0.cache_manager.uncached().blocking_handle.as_ref()
    }

    /// Creates a new client for the specified service.
    ///
    /// The client's configuration will automatically refresh to track changes in the factory's [`ServicesConfig`].
    ///
    /// If no configuration is present for the specified service in the [`ServicesConfig`], the client will
    /// immediately return an error for all requests.
    ///
    /// The method can return any type implementing the `conjure-http` [`AsyncService`] trait. This notably includes all
    /// Conjure-generated client types as well as the `conjure-runtime` [`Client`] itself.
    ///
    /// # Panics
    ///
    /// Panics if `user_agent` is not set.
    pub fn client<T>(&self, service: &str) -> Result<T, Error>
    where
        T: AsyncService<Client>,
    {
        self.client_inner(service).map(T::new)
    }

    fn client_inner(&self, service: &str) -> Result<Client, Error> {
        let service_config = self.0.config.map({
            let service = service.to_string();
            move |c| c.merged_service(&service).unwrap_or_default()
        });

        let service = service.to_string();
        let user_agent = self.0.user_agent.clone();
        let metrics = self.0.cache_manager.uncached().metrics.clone();
        let host_metrics = self.0.cache_manager.uncached().host_metrics.clone();
        let client_qos = self.0.client_qos;
        let server_qos = self.0.server_qos;
        let service_error = self.0.service_error;
        let idempotency = self.0.idempotency;
        let node_selection_strategy = self.0.node_selection_strategy;
        let cache = self.0.cache_manager.cache.clone();

        let make_state = move |config: &ServiceConfig| {
            let mut builder = Client::builder()
                .service(&service)
                .user_agent(user_agent.clone())
                .from_config(config)
                .client_qos(client_qos)
                .server_qos(server_qos)
                .service_error(service_error)
                .idempotency(idempotency)
                .node_selection_strategy(node_selection_strategy);

            if let Some(metrics) = metrics.clone() {
                builder = builder.metrics(metrics);
            }

            if let Some(host_metrics) = host_metrics.clone() {
                builder = builder.host_metrics(host_metrics);
            }

            cache.get(&builder, Builder::cached_config, ClientState::new)
        };

        let state = make_state(&service_config.get())?;
        let state = Arc::new(ArcSwap::new(state));

        let subscription = service_config.subscribe({
            let state = state.clone();
            move |config| {
                let new_state = make_state(config)?;
                state.store(new_state);
                Ok(())
            }
        })?;

        Ok(Client::new(state, Some(subscription)))
    }

    /// Creates a new blocking client for the specified service.
    ///
    /// The client's configuration will automatically refresh to track changes in the factory's [`ServicesConfig`].
    ///
    /// If no configuration is present for the specified service in the [`ServicesConfig`], the client will
    /// immediately return an error for all requests.
    ///
    /// The method can return any type implementing the `conjure-http` [`Service`] trait. This notably includes all
    /// Conjure-generated client types as well as the `conjure-runtime` [`blocking::Client`] itself.
    ///
    /// # Panics
    ///
    /// Panics if `user_agent` is not set.
    pub fn blocking_client<T>(&self, service: &str) -> Result<T, Error>
    where
        T: Service<blocking::Client>,
    {
        self.blocking_client_inner(service).map(T::new)
    }

    fn blocking_client_inner(&self, service: &str) -> Result<blocking::Client, Error> {
        self.client_inner(service).map(|client| blocking::Client {
            client,
            handle: self.0.cache_manager.uncached().blocking_handle.clone(),
        })
    }
}
