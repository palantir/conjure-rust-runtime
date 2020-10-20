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
use super::*;

#[test]
fn empty() {
    let config = "{}";
    let config = serde_json::from_str::<ServicesConfig>(config).unwrap();
    let expected = ServicesConfig::default();
    assert_eq!(config, expected);
}

#[test]
fn minimal() {
    let config = r#"
        {
            "services": {
                "foo": {
                    "uris": [
                        "http://foo1.com"
                    ]
                }
            }
        }
    "#;
    let config = serde_json::from_str::<ServicesConfig>(config).unwrap();
    let expected = ServiceConfig::builder()
        .uris(vec!["http://foo1.com".parse().unwrap()])
        .build();
    assert_eq!(config.merged_service("foo"), Some(expected));
}

#[test]
fn root_defaults() {
    let config = r#"
        {
            "services": {
                "foo": {
                    "uris": [
                        "http://foo1.com"
                    ]
                }
            },
            "security": {
                "ca-file": "/foo/bar"
            },
            "proxy": {
                "type": "http",
                "host-and-port": "localhost:1234",
                "credentials": {
                    "username": "admin",
                    "password": "palantir"
                }
            },
            "connect-timeout": "11 seconds",
            "request-timeout": "3 minutes"
        }
    "#;
    let config = serde_json::from_str::<ServicesConfig>(config).unwrap();
    let expected = ServiceConfig::builder()
        .uris(vec!["http://foo1.com".parse().unwrap()])
        .security(
            SecurityConfig::builder()
                .ca_file(Some("/foo/bar".into()))
                .build(),
        )
        .proxy(ProxyConfig::Http(
            HttpProxyConfig::builder()
                .host_and_port(HostAndPort::new("localhost", 1234))
                .credentials(Some(BasicCredentials::new("admin", "palantir")))
                .build(),
        ))
        .connect_timeout(Duration::from_secs(11))
        .request_timeout(Duration::from_secs(3 * 60))
        .build();
    assert_eq!(config.merged_service("foo"), Some(expected));
}

#[test]
fn service_overrides() {
    let config = r#"
        {
            "services": {
                "foo": {
                    "uris": [
                        "http://foo1.com"
                    ],
                    "security": {
                        "ca-file": "/fizz/buzz"
                    },
                    "proxy": {
                        "type": "mesh",
                        "host-and-port": "localhost:5678"
                    },
                    "connect-timeout": "13 seconds",
                    "request-timeout": "2 minutes"
                }
            },
            "security": {
                "ca-file": "/foo/bar"
            },
            "proxy": {
                "type": "http",
                "host-and-port": "localhost:1234",
                "credentials": {
                    "username": "admin",
                    "password": "palantir"
                }
            },
            "connect-timeout": "11 seconds",
            "request-timeout": "3 minutes"
        }
    "#;
    let config = serde_json::from_str::<ServicesConfig>(config).unwrap();
    let expected = ServiceConfig::builder()
        .uris(vec!["http://foo1.com".parse().unwrap()])
        .security(
            SecurityConfig::builder()
                .ca_file(Some("/fizz/buzz".into()))
                .build(),
        )
        .proxy(ProxyConfig::Mesh(
            MeshProxyConfig::builder()
                .host_and_port(HostAndPort::new("localhost", 5678))
                .build(),
        ))
        .connect_timeout(Duration::from_secs(13))
        .request_timeout(Duration::from_secs(2 * 60))
        .build();
    assert_eq!(config.merged_service("foo"), Some(expected));
}
