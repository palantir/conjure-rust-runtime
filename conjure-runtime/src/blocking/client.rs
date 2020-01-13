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
use crate::blocking::RequestBuilder;
use crate::{HostMetricsRegistry, RefreshHandle, UserAgent};
use conjure_error::Error;
use conjure_runtime_config::ServiceConfig;
use hyper::Method;
use std::sync::Arc;
use witchcraft_metrics::MetricRegistry;

/// A blocking HTTP client to a remote service.
///
/// It implements the Conjure `Client` trait, but also offers a "raw" request interface for use with services that don't
/// provide Conjure service definitions.
#[derive(Clone)]
pub struct Client(pub(crate) crate::Client);

impl Client {
    /// Creates a new client.
    ///
    /// The user agent is extended with an agent identifying the name and version of this crate.
    pub fn new(
        service: &str,
        user_agent: UserAgent,
        node_health: &Arc<HostMetricsRegistry>,
        metrics: &Arc<MetricRegistry>,
        config: &ServiceConfig,
    ) -> Result<Client, Error> {
        crate::Client::new(service, user_agent, node_health, metrics, config).map(Client)
    }

    /// Configures the client to assume that all requests are idempotent.
    ///
    /// Idempotent operations can be rerun without changing the result of the operation, which allows the client to
    /// safely retry failed requests. By default, GET, HEAD, PUT, and DELETE requests are assumed to be idempotent, but
    /// this method can be used to override that behavior.
    pub fn set_assume_idempotent(&mut self, assume_idempotent: bool) {
        self.0.set_assume_idempotent(assume_idempotent);
    }

    /// Returns true if the client is configured to assume all requests are idempotent.
    pub fn assume_idempotent(&self) -> bool {
        self.0.assume_idempotent()
    }

    /// Configures transparent propagation of QoS errors (i.e. 429 and 503 responses).
    ///
    /// By default, the client will automatically retry in response to QoS errors, but if this option is enabled it will
    /// instead immediately return an error which will cause the same response. This is designed for contexts where one
    /// service is proxying a request to another and the developer wants to avoid nested retry loops.
    pub fn set_propagate_qos_errors(&mut self, propagate_qos_errors: bool) {
        self.0.set_propagate_qos_errors(propagate_qos_errors);
    }

    /// Returns true if the client will propagate QoS errors.
    pub fn propagate_qos_errors(&self) -> bool {
        self.0.propagate_qos_errors()
    }

    /// Configures transparent propagation of service errors.
    ///
    /// By default, the client will turn service errors returned by the remote server into an internal server error, but
    /// if this option is enabled it will instead return the same service error it received. This is designed for
    /// contexts where one service is proxying a request to another and the developer wants the upstream client to see
    /// downstream errors.
    pub fn set_propagate_service_errors(&mut self, propagate_service_errors: bool) {
        self.0
            .set_propagate_service_errors(propagate_service_errors);
    }

    /// Returns true if the client will propagate service errors.
    pub fn propagate_service_errors(&self) -> bool {
        self.0.propagate_service_errors()
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
        self.0.refresh_handle()
    }
}
