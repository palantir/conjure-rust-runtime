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
use crate::service::proxy::ProxyConfig;
use http::header::PROXY_AUTHORIZATION;
use http::uri::Scheme;
use http::Request;
use std::task::{Context, Poll};
use tower::layer::Layer;
use tower::Service;

/// A layer which adjusts an HTTP request as necessary to respect proxy settings.
///
/// This is paired with the `ProxyConnectorLayer` which handles the socket-level proxy configuration.
///
/// For http requests over an HTTP proxy, we inject the `Proxy-Authorization` header if necessary. For https requests
/// authorization is handled in the `CONNECT` request made by the `ProxyConnectorLayer`.
pub struct ProxyLayer {
    config: ProxyConfig,
}

impl ProxyLayer {
    pub fn new(config: &ProxyConfig) -> ProxyLayer {
        ProxyLayer {
            config: config.clone(),
        }
    }
}

impl<S> Layer<S> for ProxyLayer {
    type Service = ProxyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ProxyService {
            inner,
            config: self.config.clone(),
        }
    }
}

pub struct ProxyService<S> {
    inner: S,
    config: ProxyConfig,
}

impl<S, B> Service<Request<B>> for ProxyService<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        match &self.config {
            ProxyConfig::Http(config) => {
                if req.uri().scheme() == Some(&Scheme::HTTP) {
                    if let Some(credentials) = &config.credentials {
                        req.headers_mut()
                            .insert(PROXY_AUTHORIZATION, credentials.clone());
                    }
                }
            }
            ProxyConfig::Direct => {}
        }

        self.inner.call(req)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config::{self, BasicCredentials, HostAndPort, HttpProxyConfig};
    use http::{HeaderMap, HeaderValue};
    use tower::ServiceExt;

    #[tokio::test]
    async fn http_proxied_http() {
        let config = ProxyConfig::from_config(&config::ProxyConfig::Http(
            HttpProxyConfig::builder()
                .host_and_port(HostAndPort::new("proxy", 1234))
                .credentials(Some(BasicCredentials::new("admin", "hunter2")))
                .build(),
        ))
        .unwrap();

        let service =
            ProxyLayer::new(&config).layer(tower::service_fn(|req| async { Ok::<_, ()>(req) }));

        let req = Request::builder()
            .uri("http://foobar.com/fizz/buzz")
            .body(())
            .unwrap();
        let out = service.oneshot(req).await.unwrap();

        assert_eq!(out.uri(), "http://foobar.com/fizz/buzz");

        let mut headers = HeaderMap::new();
        headers.insert(
            PROXY_AUTHORIZATION,
            HeaderValue::from_static("Basic YWRtaW46aHVudGVyMg=="),
        );
        assert_eq!(*out.headers(), headers);
    }

    #[tokio::test]
    async fn http_proxied_https() {
        let config = ProxyConfig::from_config(&config::ProxyConfig::Http(
            HttpProxyConfig::builder()
                .host_and_port(HostAndPort::new("proxy", 1234))
                .credentials(Some(BasicCredentials::new("admin", "hunter2")))
                .build(),
        ))
        .unwrap();

        let service =
            ProxyLayer::new(&config).layer(tower::service_fn(|req| async { Ok::<_, ()>(req) }));

        let req = Request::builder()
            .uri("https://foobar.com/fizz/buzz")
            .body(())
            .unwrap();
        let out = service.oneshot(req).await.unwrap();

        assert_eq!(out.uri(), "https://foobar.com/fizz/buzz");

        let headers = HeaderMap::new();
        assert_eq!(*out.headers(), headers);
    }

    #[tokio::test]
    async fn unproxied() {
        let config = ProxyConfig::from_config(&config::ProxyConfig::Direct).unwrap();

        let service =
            ProxyLayer::new(&config).layer(tower::service_fn(|req| async { Ok::<_, ()>(req) }));

        let req = Request::builder()
            .uri("https://foobar.com/fizz/buzz")
            .body(())
            .unwrap();
        let out = service.oneshot(req).await.unwrap();

        assert_eq!(out.uri(), "https://foobar.com/fizz/buzz");

        let headers = HeaderMap::new();
        assert_eq!(*out.headers(), headers);
    }
}
