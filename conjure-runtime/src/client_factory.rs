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
use crate::config::{ServiceConfig, ServicesConfig};
use crate::{
    Client, ClientQos, HostMetricsRegistry, Idempotency, NodeSelectionStrategy, ServerQos,
    ServiceError, UserAgent,
};
use arc_swap::ArcSwap;
use conjure_error::Error;
use refreshable::Refreshable;
use std::sync::Arc;
use tokio::runtime::Handle;
use witchcraft_metrics::MetricRegistry;

/// A factory type which can create clients that will live-reload in response to configuration updates.
#[derive(Clone)]
pub struct ClientFactory {
    config: Arc<Refreshable<ServicesConfig, Error>>,
    user_agent: Option<UserAgent>,
    metrics: Option<Arc<MetricRegistry>>,
    host_metrics: Option<Arc<HostMetricsRegistry>>,
    client_qos: ClientQos,
    server_qos: ServerQos,
    service_error: ServiceError,
    idempotency: Idempotency,
    node_selection_strategy: NodeSelectionStrategy,
    blocking_handle: Option<Handle>,
}

impl ClientFactory {
    /// Creates a new client factory based off of a refreshable `ServicesConfig`.
    pub fn new(config: Refreshable<ServicesConfig, Error>) -> ClientFactory {
        ClientFactory {
            config: Arc::new(config),
            user_agent: None,
            metrics: None,
            host_metrics: None,
            client_qos: ClientQos::Enabled,
            server_qos: ServerQos::AutomaticRetry,
            service_error: ServiceError::WrapInNewError,
            idempotency: Idempotency::ByMethod,
            node_selection_strategy: NodeSelectionStrategy::PinUntilError,
            blocking_handle: None,
        }
    }

    /// Sets the user agent sent by clients.
    ///
    /// Required.
    pub fn user_agent(&mut self, user_agent: UserAgent) -> &mut Self {
        self.user_agent = Some(user_agent);
        self
    }

    /// Returns the configured user agent.
    pub fn get_user_agent(&self) -> Option<&UserAgent> {
        self.user_agent.as_ref()
    }

    /// Sets clients' rate limiting behavior.
    ///
    /// Defaults to `ClientQos::Enabled`.
    pub fn client_qos(&mut self, client_qos: ClientQos) -> &mut Self {
        self.client_qos = client_qos;
        self
    }

    /// Returns the configured rate limiting behavior
    pub fn get_client_qos(&self) -> ClientQos {
        self.client_qos
    }

    /// Sets clients' behavior in response to a QoS error from the server.
    ///
    /// Defaults to `ServerQos::AutomaticRetry`.
    pub fn server_qos(&mut self, server_qos: ServerQos) -> &mut Self {
        self.server_qos = server_qos;
        self
    }

    /// Returns the configured QoS behavior.
    pub fn get_server_qos(&self) -> ServerQos {
        self.server_qos
    }

    /// Sets clients' behavior in response to a service error from the server.
    ///
    /// Defaults to `ServiceError::WrapInNewError`.
    pub fn service_error(&mut self, service_error: ServiceError) -> &mut Self {
        self.service_error = service_error;
        self
    }

    /// Returns the configured service error behavior.
    pub fn get_service_error(&self) -> ServiceError {
        self.service_error
    }

    /// Sets clients' behavior to determine if a request is idempotent or not.
    ///
    /// Only idempotent requests will be retried.
    ///
    /// Defaults to `Idempotency::ByMethod`.
    pub fn idempotency(&mut self, idempotency: Idempotency) -> &mut Self {
        self.idempotency = idempotency;
        self
    }

    /// Returns the configured idempotency behavior.
    pub fn get_idempotency(&self) -> Idempotency {
        self.idempotency
    }

    /// Sets the clients' strategy for selecting a node for a request.
    ///
    /// Defaults to `NodeSelectionStrategy::PinUntilError`.
    pub fn node_selection_strategy(
        &mut self,
        node_selection_strategy: NodeSelectionStrategy,
    ) -> &mut Self {
        self.node_selection_strategy = node_selection_strategy;
        self
    }

    /// Returns the configured node selection strategy.
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

    /// Returns the configured metrics registry.
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

    /// Returns the configured host metrics registry.
    pub fn get_host_metrics(&self) -> Option<&Arc<HostMetricsRegistry>> {
        self.host_metrics.as_ref()
    }

    /// Creates a new client for the specified service.
    ///
    /// The client's configuration will automatically refresh to track changes in the factory's `ServicesConfig`.
    ///
    /// If no configuration is present for the specified service in the `ServicesConfig`, the client will
    /// immediately return an error for all requests.
    ///
    /// # Panics
    ///
    /// Panics if `user_agent` is not set.
    pub fn client(&self, service: &str) -> Result<Client, Error> {
        let service_config = self.config.map({
            let service = service.to_string();
            move |c| c.merged_service(&service).unwrap_or_default()
        });

        let service = service.to_string();
        let user_agent = self.user_agent.clone();
        let metrics = self.metrics.clone();
        let host_metrics = self.host_metrics.clone();
        let client_qos = self.client_qos;
        let server_qos = self.server_qos;
        let service_error = self.service_error;
        let idempotency = self.idempotency;
        let node_selection_strategy = self.node_selection_strategy;

        let make_state = move |config: &ServiceConfig| {
            let mut builder = Client::builder();
            builder
                .from_config(config)
                .service(&service)
                .client_qos(client_qos)
                .server_qos(server_qos)
                .service_error(service_error)
                .idempotency(idempotency)
                .node_selection_strategy(node_selection_strategy);

            if let Some(user_agent) = user_agent.clone() {
                builder.user_agent(user_agent);
            }

            if let Some(metrics) = metrics.clone() {
                builder.metrics(metrics);
            }

            if let Some(host_metrics) = host_metrics.clone() {
                builder.host_metrics(host_metrics);
            }

            ClientState::new(&builder)
        };

        let state = make_state(&service_config.get())?;
        let state = Arc::new(ArcSwap::new(Arc::new(state)));

        let subscription = service_config.subscribe({
            let state = state.clone();
            move |config| {
                let new_state = make_state(config)?;
                state.store(Arc::new(new_state));
                Ok(())
            }
        })?;

        Ok(Client::new(state, Some(subscription)))
    }

    /// Creates a new blocking client for the specified service.
    ///
    /// The client's configuration will automatically refresh to track changes in the factory's `ServicesConfig`.
    ///
    /// If no configuration is present for the specified service in the `ServicesConfig`, the client will
    /// immediately return an error for all requests.
    ///
    /// # Panics
    ///
    /// Panics if `user_agent` is not set.
    pub fn blocking_client(&self, service: &str) -> Result<blocking::Client, Error> {
        self.client(service).map(|client| blocking::Client {
            client,
            handle: self.blocking_handle.clone(),
        })
    }
}
