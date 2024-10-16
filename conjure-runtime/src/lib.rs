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
//! An HTTP client for use with [Conjure] servers.
//!
//! This crate provides both asynchronous and blocking clients. The clients can either be used in Conjure-generated
//! wrapper types or standalone for use with non-Conjure servers.
//!
//! # Configuration
//!
//! While a `conjure_runtime` client can be built up programmatically, the more common approach is for the configuration
//! to be deserialized from a service's runtime-reloadable configuration file. The
//! [`ServicesConfig`](config::ServicesConfig) supports configuration for multiple downstream services, as well as
//! allowing for both global and per-service configuration overrides:
//!
//! ```yaml
//! services:
//!   auth-service:
//!     uris:
//!       - https://auth.my-network.com/auth-service
//!   cache-service:
//!     uris:
//!       - https://cache-1.my-network.com/cache-service
//!       - https://cache-2.my-network.com/cache-service
//!     request-timeout: 10s
//! # options set at this level will apply as defaults to all configured services
//! security:
//!   ca-file: var/security/ca.pem
//! ```
//!
//! Using the [`refreshable`] crate, a live-updating config file can be used with the [`ClientFactory`] type to create
//! live-updating clients for the configured services.
//!
//! # Usage
//!
//! First construct a raw [`Client`]:
//!
//! ```
//! use conjure_runtime::{UserAgent, Agent, Client};
//! use conjure_runtime::config::SecurityConfig;
//! use std::path::PathBuf;
//!
//! # fn foo() -> Result<(), conjure_error::Error> {
//! let client = Client::builder()
//!     .service("test-service")
//!     .user_agent(UserAgent::new(Agent::new("my-user-agent", "1.0.0")))
//!     .uri("https://url-to-server:1234/test-service".parse().unwrap())
//!     .security(
//!         SecurityConfig::builder()
//!             .ca_file(Some(PathBuf::from("path/to/ca_file.pem")))
//!             .build(),
//!     )
//!     .build()?;
//! # Ok(()) }
//! ```
//!
//! The client can then be used with Conjure-generated service interfaces:
//!
//! ```
//! use conjure_codegen::example_types::another::TestServiceAsyncClient;
//! use conjure_http::client::AsyncService;
//! use conjure_object::BearerToken;
//!
//! # async fn foo(client: conjure_runtime::Client) -> Result<(), conjure_error::Error> {
//! let client = TestServiceAsyncClient::new(client);
//!
//! let auth = BearerToken::new("my_auth_token").unwrap();
//! let file_systems = client.get_file_systems(&auth).await?;
//! # Ok(()) }
//! ```
//!
//! The [`blocking::Client`]'s API is identical, with the exception that you don't `.await` on methods:
//!
//! ```
//! use conjure_codegen::example_types::another::TestServiceClient;
//! use conjure_http::client::Service;
//! use conjure_object::BearerToken;
//!
//! # fn foo(client: conjure_runtime::blocking::Client) -> Result<(), conjure_error::Error> {
//! let client = TestServiceClient::new(client);
//!
//! let auth = BearerToken::new("my_auth_token").unwrap();
//! let file_systems = client.get_file_systems(&auth)?;
//! # Ok(()) }
//! ```
//!
//! The client can be used to communicate with non-Conjure APIs by directly using the low level
//! [`Client`](conjure_http::client::Client) and [`AsyncClient`](conjure_http::client::AsyncClient) traits.
//!
//! # Behavior
//!
//! `conjure_runtime` wraps the [`hyper`] HTTP library with opinionated behavior designed to more effectively
//! communicate between services in a distributed system. It is broadly designed to align with the [`dialogue`] Java
//! library, though it does differ in various ways.
//!
//! ## Error Propagation
//!
//! Servers should use the standard Conjure error format to propagate application-specific errors to callers. Non-QoS
//! (see below) errors received from the server are treated as fatal. By default, `conjure_runtime` will return a
//! [`conjure_error::Error`] that will generate a generic 500 Internal Server Error response. Its cause will be a
//! [`RemoteError`] object that contains the serialized Conjure error information sent by the server. The
//! [`Builder::service_error()`] and [`ClientFactory::service_error()`] methods can be used to change that behavior to
//! instead transparently propagate the error received from the server. Rather than producing a generic 500 response,
//! the returned [`conjure_error::Error`] will produce the same response the client received from the server.
//!
//! ## Call Tracing
//!
//! The client propagates trace information via the [`zipkin`] crate using the traditional `X-B3-*` HTTP headers. It
//! also creates local spans covering various stages of request processing:
//!
//! * `conjure-runtime: request`
//!     * `conjure-runtime: attempt`
//!         * `conjure-runtime: acquire-permit` - If client QoS is enabled and the node selection strategy is not
//!             [`Balanced`], this span covers the time spent acquiring a concurrency limiter permit.
//!         * `conjure-runtime: balanced-node-selection` - If the node selection strategy is [`Balanced`], this span
//!             covers the time spent selecting a node and (if client QoS is enabled) acquiring a concurrency limiter
//!             permit.
//!         * `conjure-runtime: wait-for-headers` - This span is sent to the server, and lasts until the server sends
//!             the headers of the response.
//!         * `conjure-runtime: wait-for-body` - This span is tracked along with the response body, and lasts until the
//!             [`ResponseBody`] object is dropped. It is "detached" from the zipkin tracer so new spans created outside
//!             of `conjure-runtime` will not be parented to it, and can outlive the parent `conjure-runtime` spans. It
//!             will not be created if an IO error occurs before headers are received.
//!     * `conjure-runtime: backoff-with-jitter` - If the request is retried, this span tracks the time spent waiting
//!         between attempts.
//!     * `conjure-runtime: attempt`
//!         * ...
//!
//! ## Quality of Service: Retry, Failover, Throttling, and Backpressure
//!
//! The client treats certain HTTP errors specially. Servers can advertise an overloaded state via the 429 Too Many
//! Requests or 503 Service Unavailable status codes. Unlike other 4xx and 5xx status codes, these responses do *not*
//! cause the request to fail. Instead, `conjure_runtime` will throttle itself and retry the request. Requests are
//! retried a fixed number of times, with an exponentially growing backoff in between attempts. If a 429 response
//! contains a `Retry-After` header, its backoff will be used rather than the default. IO errors also trigger a retry.
//!
//! A 503 response or IO error will also cause that host to be temporarily put on "cooldown" so it will not be used by
//! other requests unless there is no other option.
//!
//! Only some requests can be retried. By default, `conjure_runtime` will only retry requests with HTTP methods
//! identified as idempotent - `GET`, `PUT`, `DELETE`, `HEAD`, `TRACE`, and `OPTIONS`. Non-idempotent requests cannot be
//! safely retried to avoid the risk of unexpected behavior if the request ends up being applied twice. The
//! [`Builder::idempotency()`] and [`ClientFactory::idempotency()`] methods can be used to override this behavior and
//! have the client assume all or no requests are idempotent. In addition, requests with streaming request bodies can
//! only be retried if the body had either not started to be written when the error occurred or if it was successfully
//! reset for another attempt.
//!
//! ## Metrics
//!
//! Clients record metrics to both a standard [`MetricRegistry`] and a `conjure_runtime`-specific
//! [`HostMetricsRegistry`].
//!
//! ### Standard Metrics
//!
//! * `client.response (channel-name: <channel_name>, service-name: <service_name>, endpoint: <endpoint>, status:
//!     <status>)` - A `Timer` recording request durations per endpoint. Note that the requests timed by this metric
//!     are the user-percieved request, including any backoffs/retries/etc. It only records the time until response
//!     headers are received, not until the entire response body is read. The `status` tag will be `success` if the
//!     response status was 2xx and will be `failure` otherwise (QoS failure, internal server error, IO error, etc).
//! * `tls.handshake (context: <service_name>, protocol: <protocol_version>, cipher: <cipher_name>)` - A `Meter`
//!     tracking the rate of TLS handshakes, tagged by the service, TLS protocol version (e.g. `TLSv1.3`), and cipher
//!     name (e.g. `TLS_CHACHA20_POLY1305_SHA256`).
//! * `conjure-runtime.concurrencylimiter.max (service: <service_name>, hostIndex: <host_index>)` - A `Gauge` reporting
//!     the maximum number of concurrent requests which are currently permitted to be made to a specific host.
//! * `conjure-runtime.concurrencylimiter.in-flight (service: <service_name>, hostIndex: <host_index>)` - A `Gauge`
//!     reporting the current number of requests being made to a specific host.
//!
//! ### Host Metrics
//!
//! The [`HostMetricsRegistry`] contains metrics for every host of every service being actively used by a
//! `conjure_runtime` client.
//!
//! [Conjure]: https://github.com/palantir/conjure
//! [`hyper`]: https://docs.rs/hyper
//! [`RemoteError`]: errors::RemoteError
//! [`dialogue`]: https://github.com/palantir/dialogue
//! [`zipkin`]: https://docs.rs/zipkin
//! [`Balanced`]: NodeSelectionStrategy::Balanced
//! [`MetricRegistry`]: witchcraft_metrics::MetricRegistry
#![warn(missing_docs, clippy::all)]

pub use crate::body::*;
#[doc(inline)]
pub use crate::builder::{
    Builder, ClientQos, Idempotency, NodeSelectionStrategy, ServerQos, ServiceError,
};
pub use crate::client::*;
#[doc(inline)]
pub use crate::client_factory::ClientFactory;
pub use crate::host_metrics::*;
#[doc(inline)]
pub use crate::per_host_clients::{Host, PerHostClients};
pub use crate::user_agent::*;

pub mod blocking;
mod body;
pub mod builder;
mod client;
pub mod client_factory;
pub mod errors;
mod host_metrics;
pub mod per_host_clients;
pub mod raw;
mod rng;
mod service;
#[cfg(test)]
mod test;
mod user_agent;
mod util;
mod weak_cache;

/// Client configuration.
///
/// This is just a reexport of the `conjure_runtime_config` crate for convenience.
pub mod config {
    #[doc(inline)]
    pub use conjure_runtime_config::*;
}
