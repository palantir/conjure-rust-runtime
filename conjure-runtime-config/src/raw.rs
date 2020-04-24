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
use serde::de::{Deserializer, Error, Unexpected};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

#[derive(Deserialize, Default)]
#[serde(rename_all = "kebab-case", default)]
pub struct ServicesConfig {
    pub services: HashMap<String, ServiceConfig>,
    pub security: Option<SecurityConfig>,
    pub proxy: Option<ProxyConfig>,
    #[serde(deserialize_with = "de_opt_duration")]
    pub connect_timeout: Option<Duration>,
    #[serde(deserialize_with = "de_opt_duration")]
    pub request_timeout: Option<Duration>,
    #[serde(deserialize_with = "de_opt_duration")]
    pub backoff_slot_size: Option<Duration>,
    #[serde(deserialize_with = "de_opt_duration")]
    pub failed_url_cooldown: Option<Duration>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SecurityConfig {
    pub ca_file: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum ProxyConfig {
    #[serde(rename_all = "kebab-case")]
    Http {
        host_and_port: HostAndPort,
        credentials: Option<BasicCredentials>,
    },
    #[serde(rename_all = "kebab-case")]
    Mesh {
        host_and_port: HostAndPort,
    },
    Direct {},
}

pub struct HostAndPort {
    pub host: String,
    pub port: u16,
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

#[derive(Deserialize)]
pub struct BasicCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ServiceConfig {
    #[serde(default)]
    pub security: Option<SecurityConfig>,
    pub uris: Vec<Url>,
    #[serde(deserialize_with = "de_opt_duration", default)]
    pub connect_timeout: Option<Duration>,
    #[serde(deserialize_with = "de_opt_duration", default)]
    pub request_timeout: Option<Duration>,
    pub max_num_retries: Option<u32>,
    #[serde(deserialize_with = "de_opt_duration", default)]
    pub backoff_slot_size: Option<Duration>,
    #[serde(deserialize_with = "de_opt_duration", default)]
    pub failed_url_cooldown: Option<Duration>,
    #[serde(default)]
    pub proxy: Option<ProxyConfig>,
}

fn de_opt_duration<'de, D>(d: D) -> Result<Option<Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    serde_humantime::De::deserialize(d).map(serde_humantime::De::into_inner)
}
