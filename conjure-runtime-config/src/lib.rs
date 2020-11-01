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
#![doc(html_root_url = "https://docs.rs/conjure-runtime-config/0.2")]
#![warn(missing_docs, clippy::all)]

use serde::de::{Deserializer, Error as _, Unexpected};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

#[cfg(test)]
mod test;

/// Configuration for a collection of services.
///
/// This type can be constructed programmatically via the `ServicesConfigBuilder` API or deserialized from e.g. a
/// configuration file. Default values for various configuration options can be set at the top level in addition to
/// being specified per-service.
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
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ServicesConfig {
    services: HashMap<String, ServiceConfig>,
    security: Option<SecurityConfig>,
    proxy: Option<ProxyConfig>,
    #[serde(deserialize_with = "de_opt_duration")]
    connect_timeout: Option<Duration>,
    #[serde(deserialize_with = "de_opt_duration")]
    request_timeout: Option<Duration>,
    #[serde(deserialize_with = "de_opt_duration")]
    backoff_slot_size: Option<Duration>,
}

impl ServicesConfig {
    /// Returns a new builder.
    pub fn builder() -> ServicesConfigBuilder {
        ServicesConfigBuilder::default()
    }

    /// Returns the configuration for the specified service with top-level defaults applied.
    pub fn merged_service(&self, name: &str) -> Option<ServiceConfig> {
        let mut service = self.services.get(name).cloned()?;

        if service.security.is_none() {
            service.security = self.security.clone();
        }

        if service.proxy.is_none() {
            service.proxy = self.proxy.clone();
        }

        if service.connect_timeout.is_none() {
            service.connect_timeout = self.connect_timeout;
        }

        if service.request_timeout.is_none() {
            service.request_timeout = self.request_timeout;
        }

        if service.backoff_slot_size.is_none() {
            service.backoff_slot_size = self.backoff_slot_size;
        }

        Some(service)
    }

    /// Returns the security configuration.
    pub fn security(&self) -> Option<&SecurityConfig> {
        self.security.as_ref()
    }

    /// Returns the proxy configuration.
    pub fn proxy(&self) -> Option<&ProxyConfig> {
        self.proxy.as_ref()
    }

    /// Returns the connection timeout.
    pub fn connect_timeout(&self) -> Option<Duration> {
        self.connect_timeout
    }

    /// Returns the request timeout.
    pub fn request_timeout(&self) -> Option<Duration> {
        self.request_timeout
    }

    /// Returns the backoff slot size.
    pub fn backoff_slot_size(&self) -> Option<Duration> {
        self.backoff_slot_size
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

    /// Sets the security configuration.
    pub fn security(&mut self, security: SecurityConfig) -> &mut Self {
        self.0.security = Some(security);
        self
    }

    /// Sets the proxy configuration.
    pub fn proxy(&mut self, proxy: ProxyConfig) -> &mut Self {
        self.0.proxy = Some(proxy);
        self
    }

    /// Sets the connect timeout.
    pub fn connect_timeout(&mut self, connect_timeout: Duration) -> &mut Self {
        self.0.connect_timeout = Some(connect_timeout);
        self
    }

    /// Sets the request timeout.
    pub fn request_timeout(&mut self, request_timeout: Duration) -> &mut Self {
        self.0.request_timeout = Some(request_timeout);
        self
    }

    /// Sets the backoff slot size.
    pub fn backoff_slot_size(&mut self, backoff_slot_size: Duration) -> &mut Self {
        self.0.backoff_slot_size = Some(backoff_slot_size);
        self
    }

    /// Creates a new `ServicesConfig`.
    pub fn build(&self) -> ServicesConfig {
        self.0.clone()
    }
}

/// The configuration for an individual service.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ServiceConfig {
    uris: Vec<Url>,
    security: Option<SecurityConfig>,
    proxy: Option<ProxyConfig>,
    #[serde(deserialize_with = "de_opt_duration")]
    connect_timeout: Option<Duration>,
    #[serde(deserialize_with = "de_opt_duration")]
    request_timeout: Option<Duration>,
    #[serde(deserialize_with = "de_opt_duration")]
    backoff_slot_size: Option<Duration>,
    max_num_retries: Option<u32>,
}

impl ServiceConfig {
    /// Returns a new builder.
    pub fn builder() -> ServiceConfigBuilder {
        ServiceConfigBuilder::default()
    }

    /// Returns the URIs of the service's nodes.
    pub fn uris(&self) -> &[Url] {
        &self.uris
    }

    /// Returns the security configuration.
    pub fn security(&self) -> Option<&SecurityConfig> {
        self.security.as_ref()
    }

    /// Returns the proxy configuration.
    pub fn proxy(&self) -> Option<&ProxyConfig> {
        self.proxy.as_ref()
    }

    /// Returns the connection timeout.
    pub fn connect_timeout(&self) -> Option<Duration> {
        self.connect_timeout
    }

    /// Returns the request timeout.
    pub fn request_timeout(&self) -> Option<Duration> {
        self.request_timeout
    }

    /// Returns the backoff slot size.
    pub fn backoff_slot_size(&self) -> Option<Duration> {
        self.backoff_slot_size
    }

    /// Returns the maximum number of retries for failed RPCs.
    pub fn max_num_retries(&self) -> Option<u32> {
        self.max_num_retries
    }
}

/// A builder type for `ServiceConfig`s.
#[derive(Default)]
pub struct ServiceConfigBuilder(ServiceConfig);

impl From<ServiceConfig> for ServiceConfigBuilder {
    fn from(config: ServiceConfig) -> ServiceConfigBuilder {
        ServiceConfigBuilder(config)
    }
}

impl ServiceConfigBuilder {
    /// Appends a URI to the URIs list.
    pub fn uri(&mut self, uri: Url) -> &mut Self {
        self.0.uris.push(uri);
        self
    }

    /// Sets the URIs list.
    pub fn uris(&mut self, uris: Vec<Url>) -> &mut Self {
        self.0.uris = uris;
        self
    }

    /// Sets the security configuration.
    pub fn security(&mut self, security: SecurityConfig) -> &mut Self {
        self.0.security = Some(security);
        self
    }

    /// Sets the proxy configuration.
    pub fn proxy(&mut self, proxy: ProxyConfig) -> &mut Self {
        self.0.proxy = Some(proxy);
        self
    }

    /// Sets the connect timeout.
    pub fn connect_timeout(&mut self, connect_timeout: Duration) -> &mut Self {
        self.0.connect_timeout = Some(connect_timeout);
        self
    }

    /// Sets the request timeout.
    pub fn request_timeout(&mut self, request_timeout: Duration) -> &mut Self {
        self.0.request_timeout = Some(request_timeout);
        self
    }

    /// Sets the backoff slot size.
    pub fn backoff_slot_size(&mut self, backoff_slot_size: Duration) -> &mut Self {
        self.0.backoff_slot_size = Some(backoff_slot_size);
        self
    }

    /// Sets the maximum number of retries for failed RPCs.
    pub fn max_num_retries(&mut self, max_num_retries: u32) -> &mut Self {
        self.0.max_num_retries = Some(max_num_retries);
        self
    }

    /// Creates a new `ServiceConfig`.
    pub fn build(&self) -> ServiceConfig {
        self.0.clone()
    }
}

/// Security configuration used to communicate with a service.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
    pub fn ca_file(&self) -> Option<&Path> {
        self.ca_file.as_deref()
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
#[non_exhaustive]
pub enum ProxyConfig {
    /// A direct connection (i.e. no proxy).
    Direct,
    /// An HTTP proxy.
    Http(HttpProxyConfig),
    /// A mesh proxy.
    Mesh(MeshProxyConfig),
}

impl Default for ProxyConfig {
    fn default() -> ProxyConfig {
        ProxyConfig::Direct
    }
}

/// Configuration for an HTTP proxy.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
    pub fn host_and_port(&self) -> &HostAndPort {
        &self.host_and_port
    }

    /// The credentials used to authenticate with the proxy.
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
    ///
    /// # Panics
    ///
    /// Panics if `host_and_port` is not set.
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

impl<'de> Deserialize<'de> for HostAndPort {
    fn deserialize<D>(deserializer: D) -> Result<HostAndPort, D::Error>
    where
        D: Deserializer<'de>,
    {
        let expected = "a host:port identifier";

        let mut s = String::deserialize(deserializer)?;

        match s.find(':') {
            Some(idx) => {
                let port = s[idx + 1..]
                    .parse()
                    .map_err(|_| D::Error::invalid_value(Unexpected::Str(&s), &expected))?;
                s.truncate(idx);
                Ok(HostAndPort { host: s, port })
            }
            None => Err(D::Error::invalid_value(Unexpected::Str(&s), &expected)),
        }
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
}

/// Credentials used to authenticate with an HTTP proxy.
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
}

/// Configuration for a mesh proxy.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
    ///
    /// # Panics
    ///
    /// Panics if `host_and_port` is not set.
    pub fn build(&self) -> MeshProxyConfig {
        MeshProxyConfig {
            host_and_port: self.host_and_port.clone().expect("host_and_port not set"),
        }
    }
}

fn de_opt_duration<'de, D>(d: D) -> Result<Option<Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    serde_humantime::De::deserialize(d).map(serde_humantime::De::into_inner)
}
