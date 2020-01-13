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
//! Deserializable configuration types for `conjure_runtime` clients.
#![doc(html_root_url = "https://docs.rs/conjure-runtime-config/0.1")]
#![warn(missing_docs, clippy::all)]

use serde::de::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

mod raw;
#[cfg(test)]
mod test;

/// Configuration for a collection of services.
///
/// This type can be constructed programmatically via the `ServicesConfigBuilder` API or deserialized from e.g. a
/// configuration file. When deserializing, default values for various configuration options can be overridden at the
/// top level in addition to being specified per-service.
///
/// # Examples
///
/// ```yaml
/// services:
///   auth-service:
///     uris:
///       - https://auth.my-network.com:1234/auth-service
///   cache-service:
///     uris:
///       - https://cache-1.my-network.com/cache-service
///       - https://cache-2.my-network.com/cache-service
///     request-timeout: 10s
/// # options set at this level will apply as defaults to all configured services
/// security:
///   ca-file: var/security/ca.pem
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServicesConfig {
    services: HashMap<String, ServiceConfig>,
}

impl<'de> Deserialize<'de> for ServicesConfig {
    fn deserialize<D>(d: D) -> Result<ServicesConfig, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = raw::ServicesConfig::deserialize(d)?;
        Ok(ServicesConfig::from_raw(raw))
    }
}

impl ServicesConfig {
    /// Returns a new builder.
    pub fn builder() -> ServicesConfigBuilder {
        ServicesConfigBuilder::default()
    }

    /// Returns the configuration for the specified service, if present.
    pub fn service(&self, service: &str) -> Option<&ServiceConfig> {
        self.services.get(service)
    }

    // FIXME: remove this check when clippy stops complaining.
    // or_fun_call in newer versions of clippy ignore common cheap calls such as as_ref.
    #[allow(clippy::or_fun_call)]
    fn from_raw(raw: raw::ServicesConfig) -> ServicesConfig {
        let mut config = ServicesConfig::builder();

        for (name, raw_service) in raw.services {
            let mut service = ServiceConfig::builder();
            service.uris(raw_service.uris);
            if let Some(security) = raw_service.security.as_ref().or(raw.security.as_ref()) {
                service.security(SecurityConfig::from_raw(security));
            }
            if let Some(proxy) = raw_service.proxy.as_ref().or(raw.proxy.as_ref()) {
                service.proxy(ProxyConfig::from_raw(proxy));
            }
            if let Some(connect_timeout) = raw_service.connect_timeout.or(raw.connect_timeout) {
                service.connect_timeout(connect_timeout);
            }
            if let Some(request_timeout) = raw_service.request_timeout.or(raw.request_timeout) {
                service.request_timeout(request_timeout);
            }
            if let Some(max_num_retries) = raw_service.max_num_retries {
                service.max_num_retries(max_num_retries);
            }
            if let Some(backoff_slot_size) = raw_service.backoff_slot_size.or(raw.backoff_slot_size)
            {
                service.backoff_slot_size(backoff_slot_size);
            }
            if let Some(failed_url_cooldown) =
                raw_service.failed_url_cooldown.or(raw.failed_url_cooldown)
            {
                service.failed_url_cooldown(failed_url_cooldown);
            }

            config.service(&name, service.build());
        }

        config.build()
    }
}

/// A builder type for `ServiceConfig`.
pub struct ServicesConfigBuilder(ServicesConfig);

impl Default for ServicesConfigBuilder {
    fn default() -> ServicesConfigBuilder {
        ServicesConfigBuilder(ServicesConfig::default())
    }
}

impl From<ServicesConfig> for ServicesConfigBuilder {
    fn from(config: ServicesConfig) -> ServicesConfigBuilder {
        ServicesConfigBuilder(config)
    }
}

impl ServicesConfigBuilder {
    /// Adds a service to the builder.
    pub fn service(&mut self, name: &str, config: ServiceConfig) -> &mut Self {
        self.0.services.insert(name.to_string(), config);
        self
    }

    /// Creates a new `ServicesConfig`.
    pub fn build(&self) -> ServicesConfig {
        self.0.clone()
    }
}

/// The configuration for an individual service.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceConfig {
    uris: Vec<Url>,
    security: SecurityConfig,
    connect_timeout: Duration,
    request_timeout: Duration,
    max_num_retries: u32,
    backoff_slot_size: Duration,
    failed_url_cooldown: Duration,
    proxy: ProxyConfig,
}

impl Default for ServiceConfig {
    fn default() -> ServiceConfig {
        ServiceConfig {
            uris: vec![],
            security: SecurityConfig::builder().build(),
            proxy: ProxyConfig::Direct,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(5 * 60),
            max_num_retries: 3,
            backoff_slot_size: Duration::from_millis(250),
            failed_url_cooldown: Duration::from_millis(250),
        }
    }
}

impl ServiceConfig {
    /// Returns a new builder.
    pub fn builder() -> ServiceConfigBuilder {
        ServiceConfigBuilder::default()
    }

    /// Returns the URIs of the service's nodes.
    ///
    /// Defaults to an empty list.
    pub fn uris(&self) -> &[Url] {
        &self.uris
    }

    /// Returns the service's security configuration.
    pub fn security(&self) -> &SecurityConfig {
        &self.security
    }

    /// Returns the timeout used when connecting to the nodes of a service.
    ///
    /// Defaults to 10 seconds.
    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    /// Returns the overall request timeout used when making a request to the service.
    ///
    /// The timeout is applied from when the request is first started to when the headers of a successful response are
    /// returned.
    ///
    /// Defaults to 5 minutes.
    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    /// Returns the number of times the client will retry a request after failed attempts before giving up.
    ///
    /// Defaults to 3.
    pub fn max_num_retries(&self) -> u32 {
        self.max_num_retries
    }

    /// Returns the base time of the exponential backoff algorithm applied between attempts.
    ///
    /// Defaults to 250 milliseconds.
    pub fn backoff_slot_size(&self) -> Duration {
        self.backoff_slot_size
    }

    /// Returns the amount of time a node of a service is blacklisted after a failed request.
    ///
    /// While blacklisted, a node will not be used for requests unless no other node is available.
    ///
    /// Defaults to 250 milliseconds.
    pub fn failed_url_cooldown(&self) -> Duration {
        self.failed_url_cooldown
    }

    /// Returns the proxy configuration used to communicate with the service.
    ///
    /// Defaults to "direct" (i.e. no proxy).
    pub fn proxy(&self) -> &ProxyConfig {
        &self.proxy
    }
}

/// A builder type for `ServiceConfig`s.
pub struct ServiceConfigBuilder(ServiceConfig);

impl Default for ServiceConfigBuilder {
    fn default() -> ServiceConfigBuilder {
        ServiceConfigBuilder(ServiceConfig::default())
    }
}

impl From<ServiceConfig> for ServiceConfigBuilder {
    fn from(config: ServiceConfig) -> ServiceConfigBuilder {
        ServiceConfigBuilder(config)
    }
}

impl ServiceConfigBuilder {
    /// Sets the URI list.
    pub fn uris(&mut self, uris: Vec<Url>) -> &mut Self {
        self.0.uris = uris;
        self
    }

    /// Sets the security configuration.
    pub fn security(&mut self, security: SecurityConfig) -> &mut Self {
        self.0.security = security;
        self
    }

    /// Sets the connect timeout.
    pub fn connect_timeout(&mut self, connect_timeout: Duration) -> &mut Self {
        self.0.connect_timeout = connect_timeout;
        self
    }

    /// Sets the request timeout.
    pub fn request_timeout(&mut self, request_timeout: Duration) -> &mut Self {
        self.0.request_timeout = request_timeout;
        self
    }

    /// Sets the maximum retry count.
    pub fn max_num_retries(&mut self, max_num_retries: u32) -> &mut Self {
        self.0.max_num_retries = max_num_retries;
        self
    }

    /// Sets the backoff slot size.
    pub fn backoff_slot_size(&mut self, backoff_slot_size: Duration) -> &mut Self {
        self.0.backoff_slot_size = backoff_slot_size;
        self
    }

    /// Sets the failed URL cooldown.
    pub fn failed_url_cooldown(&mut self, failed_url_cooldown: Duration) -> &mut Self {
        self.0.failed_url_cooldown = failed_url_cooldown;
        self
    }

    /// Sets the proxy configuration.
    pub fn proxy(&mut self, proxy: ProxyConfig) -> &mut Self {
        self.0.proxy = proxy;
        self
    }

    /// Creates a new `ServiceConfig`.
    pub fn build(&self) -> ServiceConfig {
        self.0.clone()
    }
}

/// Security configuration used to communicate with a service.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SecurityConfig {
    ca_file: Option<PathBuf>,
}

impl SecurityConfig {
    /// Returns a new builder.
    pub fn builder() -> SecurityConfigBuilder {
        SecurityConfigBuilder::default()
    }

    /// The path to a file containing PEM-formatted root certificates trusted to identify the service.
    ///
    /// These certificates are used in addition to the system's root CA list.
    ///
    /// Defaults to `None`.
    pub fn ca_file(&self) -> Option<&Path> {
        self.ca_file.as_ref().map(|p| &**p)
    }

    fn from_raw(raw: &raw::SecurityConfig) -> SecurityConfig {
        let mut config = SecurityConfig::builder();
        if let Some(ref ca_file) = raw.ca_file {
            config.ca_file(Some(ca_file.to_path_buf()));
        }
        config.build()
    }
}

/// A builder type for `SecurityConfig`s.
pub struct SecurityConfigBuilder(SecurityConfig);

impl Default for SecurityConfigBuilder {
    fn default() -> SecurityConfigBuilder {
        SecurityConfigBuilder(SecurityConfig::default())
    }
}

impl From<SecurityConfig> for SecurityConfigBuilder {
    fn from(config: SecurityConfig) -> SecurityConfigBuilder {
        SecurityConfigBuilder(config)
    }
}

impl SecurityConfigBuilder {
    /// Sets the trusted root CA file.
    pub fn ca_file(&mut self, ca_file: Option<PathBuf>) -> &mut Self {
        self.0.ca_file = ca_file;
        self
    }

    /// Creates a new `SecurityConfig`.
    pub fn build(&self) -> SecurityConfig {
        self.0.clone()
    }
}

/// Proxy configuration used to connect to a service.
#[derive(Debug, Clone, PartialEq)]
pub enum ProxyConfig {
    /// A direct connection (i.e. no proxy).
    Direct,
    /// An HTTP proxy.
    Http(HttpProxyConfig),
    /// A mesh proxy.
    Mesh(MeshProxyConfig),
    #[doc(hidden)]
    __ForExtensibility,
}

impl Default for ProxyConfig {
    fn default() -> ProxyConfig {
        ProxyConfig::Direct
    }
}

impl ProxyConfig {
    fn from_raw(raw: &raw::ProxyConfig) -> ProxyConfig {
        match *raw {
            raw::ProxyConfig::Http {
                ref host_and_port,
                ref credentials,
            } => {
                let mut builder = HttpProxyConfig::builder();
                builder.host_and_port(HostAndPort::from_raw(host_and_port));
                if let Some(ref credentials) = *credentials {
                    builder.credentials(Some(BasicCredentials::from_raw(credentials)));
                }
                ProxyConfig::Http(builder.build())
            }
            raw::ProxyConfig::Mesh { ref host_and_port } => {
                let config = MeshProxyConfig::builder()
                    .host_and_port(HostAndPort::from_raw(host_and_port))
                    .build();
                ProxyConfig::Mesh(config)
            }
            raw::ProxyConfig::Direct {} => ProxyConfig::Direct,
        }
    }
}

/// Configuration for an HTTP proxy.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpProxyConfig {
    host_and_port: HostAndPort,
    credentials: Option<BasicCredentials>,
}

impl HttpProxyConfig {
    /// Returns a new builder.
    pub fn builder() -> HttpProxyConfigBuilder {
        HttpProxyConfigBuilder::default()
    }

    /// The host and port of the proxy server.
    ///
    /// Required.
    pub fn host_and_port(&self) -> &HostAndPort {
        &self.host_and_port
    }

    /// The credentials used to authenticate with the proxy.
    ///
    /// Defaults to `None`.
    pub fn credentials(&self) -> Option<&BasicCredentials> {
        self.credentials.as_ref()
    }
}

/// A builder for `HttpProxyConfig`s.
pub struct HttpProxyConfigBuilder {
    host_and_port: Option<HostAndPort>,
    credentials: Option<BasicCredentials>,
}

impl Default for HttpProxyConfigBuilder {
    fn default() -> HttpProxyConfigBuilder {
        HttpProxyConfigBuilder {
            host_and_port: None,
            credentials: None,
        }
    }
}

impl From<HttpProxyConfig> for HttpProxyConfigBuilder {
    fn from(config: HttpProxyConfig) -> HttpProxyConfigBuilder {
        HttpProxyConfigBuilder {
            host_and_port: Some(config.host_and_port),
            credentials: config.credentials,
        }
    }
}

impl HttpProxyConfigBuilder {
    /// Sets the host and port.
    pub fn host_and_port(&mut self, host_and_port: HostAndPort) -> &mut Self {
        self.host_and_port = Some(host_and_port);
        self
    }

    /// Sets the credentials.
    pub fn credentials(&mut self, credentials: Option<BasicCredentials>) -> &mut Self {
        self.credentials = credentials;
        self
    }

    /// Creates a new `HttpProxyConfig`.
    pub fn build(&self) -> HttpProxyConfig {
        HttpProxyConfig {
            host_and_port: self.host_and_port.clone().expect("host_and_port not set"),
            credentials: self.credentials.clone(),
        }
    }
}

/// A host and port identifier of a server.
#[derive(Debug, Clone, PartialEq)]
pub struct HostAndPort {
    host: String,
    port: u16,
}

impl fmt::Display for HostAndPort {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}:{}", self.host, self.port)
    }
}

impl HostAndPort {
    /// Creates a new `HostAndPort`.
    pub fn new(host: &str, port: u16) -> HostAndPort {
        HostAndPort {
            host: host.to_string(),
            port,
        }
    }

    /// Returns the host.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Returns the port.
    pub fn port(&self) -> u16 {
        self.port
    }

    fn from_raw(raw: &raw::HostAndPort) -> HostAndPort {
        HostAndPort::new(&raw.host, raw.port)
    }
}

/// Credentials used to authenticate with an HTTP proxy.
#[derive(Debug, Clone, PartialEq)]
pub struct BasicCredentials {
    username: String,
    password: String,
}

impl BasicCredentials {
    /// Creates a new `BasicCredentials.
    pub fn new(username: &str, password: &str) -> BasicCredentials {
        BasicCredentials {
            username: username.to_string(),
            password: password.to_string(),
        }
    }

    /// Returns the username.
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Returns the password.
    pub fn password(&self) -> &str {
        &self.password
    }

    fn from_raw(raw: &raw::BasicCredentials) -> BasicCredentials {
        BasicCredentials::new(&raw.username, &raw.password)
    }
}

/// Configuration for a mesh proxy.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshProxyConfig {
    host_and_port: HostAndPort,
}

impl MeshProxyConfig {
    /// Creates a new builder.
    pub fn builder() -> MeshProxyConfigBuilder {
        MeshProxyConfigBuilder::default()
    }

    /// Returns the host and port of the proxy server.
    pub fn host_and_port(&self) -> &HostAndPort {
        &self.host_and_port
    }
}

/// A builder type for `MeshProxyConfig`s.
pub struct MeshProxyConfigBuilder {
    host_and_port: Option<HostAndPort>,
}

impl Default for MeshProxyConfigBuilder {
    fn default() -> MeshProxyConfigBuilder {
        MeshProxyConfigBuilder {
            host_and_port: None,
        }
    }
}

impl From<MeshProxyConfig> for MeshProxyConfigBuilder {
    fn from(config: MeshProxyConfig) -> MeshProxyConfigBuilder {
        MeshProxyConfigBuilder {
            host_and_port: Some(config.host_and_port),
        }
    }
}

impl MeshProxyConfigBuilder {
    /// Sets the host and port.
    pub fn host_and_port(&mut self, host_and_port: HostAndPort) -> &mut Self {
        self.host_and_port = Some(host_and_port);
        self
    }

    /// Creates a new `MeshProxyBuilder`.
    pub fn build(&self) -> MeshProxyConfig {
        MeshProxyConfig {
            host_and_port: self.host_and_port.clone().expect("host_and_port not set"),
        }
    }
}
