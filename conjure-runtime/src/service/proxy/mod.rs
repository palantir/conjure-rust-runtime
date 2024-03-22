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
use crate::config;
pub use crate::service::proxy::connector::ProxyConnectorService;
pub use crate::service::proxy::request::ProxyLayer;
use base64::display::Base64Display;
use base64::engine::general_purpose::STANDARD;
use conjure_error::Error;
use http::{HeaderValue, Uri};
use std::convert::TryFrom;

pub mod connector;
pub mod request;

#[derive(Clone)]
pub enum ProxyConfig {
    Http(HttpProxyConfig),
    Direct,
}

#[derive(Clone)]
pub struct HttpProxyConfig {
    uri: Uri,
    credentials: Option<HeaderValue>,
}

impl ProxyConfig {
    pub fn from_config(config: &config::ProxyConfig) -> Result<ProxyConfig, Error> {
        match config {
            config::ProxyConfig::Direct => Ok(ProxyConfig::Direct),
            config::ProxyConfig::Http(config) => {
                let uri = format!(
                    "http://{}:{}",
                    config.host_and_port().host(),
                    config.host_and_port().port(),
                )
                .parse::<Uri>()
                .map_err(Error::internal_safe)?;

                let credentials = config.credentials().map(|c| {
                    let auth = format!("{}:{}", c.username(), c.password());
                    let header =
                        format!("Basic {}", Base64Display::new(auth.as_bytes(), &STANDARD));
                    HeaderValue::try_from(header).unwrap()
                });

                Ok(ProxyConfig::Http(HttpProxyConfig { uri, credentials }))
            }
            _ => Err(Error::internal_safe("unknown proxy type")),
        }
    }
}
