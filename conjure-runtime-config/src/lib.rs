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
#![warn(missing_docs, clippy::all)]
// reserve the right to add non-eq config in the future
#![allow(clippy::derive_partial_eq_without_eq)]

use serde::de::{Deserializer, Error as _, Unexpected};
use serde::Deserialize;
use staged_builder::staged_builder;
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
#[staged_builder]
#[builder(update)]
pub struct ServicesConfig {
    #[builder(map(key(type = String, into), value(type = ServiceConfig)))]
    services: HashMap<String, ServiceConfig>,
    #[builder(default, into)]
    security: Option<SecurityConfig>,
    #[builder(default, into)]
    proxy: Option<ProxyConfig>,
    #[builder(default, into)]
    #[serde(deserialize_with = "de_opt_duration")]
    connect_timeout: Option<Duration>,
    #[builder(default, into)]
    #[serde(deserialize_with = "de_opt_duration")]
    read_timeout: Option<Duration>,
    #[builder(default, into)]
    #[serde(deserialize_with = "de_opt_duration")]
    write_timeout: Option<Duration>,
    #[builder(default, into)]
    #[serde(deserialize_with = "de_opt_duration")]
    backoff_slot_size: Option<Duration>,
}

impl ServicesConfig {
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

        if service.read_timeout.is_none() {
            service.read_timeout = self.read_timeout;
        }

        if service.write_timeout.is_none() {
            service.write_timeout = self.write_timeout;
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

    /// Returns the read timeout.
    pub fn read_timeout(&self) -> Option<Duration> {
        self.read_timeout
    }

    /// Returns the write timeout.
    pub fn write_timeout(&self) -> Option<Duration> {
        self.write_timeout
    }

    /// Returns the backoff slot size.
    pub fn backoff_slot_size(&self) -> Option<Duration> {
        self.backoff_slot_size
    }
}

/// The configuration for an individual service.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
#[staged_builder]
#[builder(update)]
pub struct ServiceConfig {
    #[builder(list(item(type = Url)))]
    uris: Vec<Url>,
    #[builder(default, into)]
    security: Option<SecurityConfig>,
    #[builder(default, into)]
    proxy: Option<ProxyConfig>,
    #[builder(default, into)]
    #[serde(deserialize_with = "de_opt_duration")]
    connect_timeout: Option<Duration>,
    #[builder(default, into)]
    #[serde(deserialize_with = "de_opt_duration")]
    read_timeout: Option<Duration>,
    #[builder(default, into)]
    #[serde(deserialize_with = "de_opt_duration")]
    write_timeout: Option<Duration>,
    #[builder(default, into)]
    #[serde(deserialize_with = "de_opt_duration")]
    backoff_slot_size: Option<Duration>,
    #[builder(default, into)]
    max_num_retries: Option<u32>,
}

impl ServiceConfig {
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

    /// Returns the read timeout.
    pub fn read_timeout(&self) -> Option<Duration> {
        self.read_timeout
    }

    /// Returns the write timeout.
    pub fn write_timeout(&self) -> Option<Duration> {
        self.write_timeout
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

/// Security configuration used to communicate with a service.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[staged_builder]
#[builder(update)]
pub struct SecurityConfig {
    #[builder(default, into)]
    ca_file: Option<PathBuf>,
    #[builder(default, into)]
    key_file: Option<PathBuf>,
    #[builder(default, into)]
    cert_file: Option<PathBuf>,
}

impl SecurityConfig {
    /// The path to a file containing PEM-formatted root certificates trusted to identify the service.
    ///
    /// These certificates are used in addition to the bundled root CA list.
    pub fn ca_file(&self) -> Option<&Path> {
        self.ca_file.as_deref()
    }

    /// The path to a file containing a PEM-formatted private key used for client certificate authentication.
    ///
    /// This key is expected to match the leaf certificate in [`Self::cert_file`].
    pub fn key_file(&self) -> Option<&Path> {
        self.key_file.as_deref()
    }

    /// The path to a file containing PEM-formatted certificates used for client certificate authentication.
    ///
    /// The file should start with the leaf certificate corresponding to the key in [`Self::key_file`], and the contain
    /// the remainder of the certificate chain to a trusted root.
    pub fn cert_file(&self) -> Option<&Path> {
        self.cert_file.as_deref()
    }
}

/// Proxy configuration used to connect to a service.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
#[non_exhaustive]
pub enum ProxyConfig {
    /// A direct connection (i.e. no proxy).
    Direct,
    /// An HTTP proxy.
    Http(HttpProxyConfig),
}

#[allow(clippy::derivable_impls)]
impl Default for ProxyConfig {
    fn default() -> ProxyConfig {
        ProxyConfig::Direct
    }
}

/// Configuration for an HTTP proxy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[staged_builder]
#[builder(update)]
pub struct HttpProxyConfig {
    host_and_port: HostAndPort,
    #[builder(default, into)]
    credentials: Option<BasicCredentials>,
}

impl HttpProxyConfig {
    /// The host and port of the proxy server.
    pub fn host_and_port(&self) -> &HostAndPort {
        &self.host_and_port
    }

    /// The credentials used to authenticate with the proxy.
    pub fn credentials(&self) -> Option<&BasicCredentials> {
        self.credentials.as_ref()
    }
}

/// A host and port identifier of a server.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
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

fn de_opt_duration<'de, D>(d: D) -> Result<Option<Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    humantime_serde::Serde::deserialize(d).map(humantime_serde::Serde::into_inner)
}
