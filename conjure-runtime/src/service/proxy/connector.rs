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
use crate::service::proxy::{HttpProxyConfig, ProxyConfig};
use bytes::Bytes;
use futures::future::{self, BoxFuture};
use http::header::{HOST, PROXY_AUTHORIZATION};
use http::uri::Scheme;
use http::{HeaderValue, Method, Request, StatusCode, Uri, Version};
use http_body_util::Empty;
use hyper::client::conn;
use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::client::legacy::connect::{Connected, Connection};
use std::convert::TryFrom;
use std::error;
use std::fmt;
use std::future::Future;
use std::io;
use std::pin::{pin, Pin};
use std::task::{Context, Poll};
use tower_layer::Layer;
use tower_service::Service;

/// A connector layer which handles socket-level setup for HTTP proxies.
///
/// For http requests, we just connect to the proxy server and tell hyper that the connection is proxied so it can
/// adjust the HTTP request. For https requests, we create a tunnel through the proxy server to the target server via a
/// CONNECT request that is then used upstream for the TLS handshake.
pub struct ProxyConnectorLayer {
    config: Option<HttpProxyConfig>,
}

impl ProxyConnectorLayer {
    pub fn new(config: &ProxyConfig) -> ProxyConnectorLayer {
        let config = match config {
            ProxyConfig::Http(config) => Some(config.clone()),
            _ => None,
        };

        ProxyConnectorLayer { config }
    }
}

impl<S> Layer<S> for ProxyConnectorLayer {
    type Service = ProxyConnectorService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ProxyConnectorService {
            inner,
            config: self.config.clone(),
        }
    }
}

#[derive(Clone)]
pub struct ProxyConnectorService<S> {
    inner: S,
    config: Option<HttpProxyConfig>,
}

impl<S> Service<Uri> for ProxyConnectorService<S>
where
    S: Service<Uri> + Send,
    S::Response: Read + Write + Unpin + Send + 'static,
    S::Error: Into<Box<dyn error::Error + Sync + Send>>,
    S::Future: Send + 'static,
{
    type Response = ProxyConnection<S::Response>;
    type Error = Box<dyn error::Error + Sync + Send>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: Uri) -> Self::Future {
        let config = match &self.config {
            Some(proxy) => proxy.clone(),
            None => {
                let connect = self.inner.call(req);
                return Box::pin(async move {
                    let stream = connect.await.map_err(Into::into)?;
                    Ok(ProxyConnection {
                        stream,
                        proxy: false,
                    })
                });
            }
        };

        let connect = self.inner.call(config.uri.clone());
        Box::pin(async move {
            let stream = connect.await.map_err(Into::into)?;

            if req.scheme() == Some(&Scheme::HTTP) {
                Ok(ProxyConnection {
                    stream,
                    proxy: true,
                })
            } else {
                let stream = connect_https(stream, req, config).await?;
                Ok(ProxyConnection {
                    stream,
                    proxy: false,
                })
            }
        })
    }
}

async fn connect_https<T>(
    stream: T,
    uri: Uri,
    config: HttpProxyConfig,
) -> Result<T, Box<dyn error::Error + Sync + Send>>
where
    T: Read + Write + Send + Unpin + 'static,
{
    let (mut sender, mut conn) = conn::http1::handshake(stream).await?;

    let host = uri.host().ok_or("host missing from URI")?;
    let authority = format!("{}:{}", host, uri.port_u16().unwrap_or(443));
    let authority_uri = Uri::try_from(authority.clone()).unwrap();
    let host = HeaderValue::try_from(authority).unwrap();

    let mut request = Request::new(Empty::<Bytes>::new());
    *request.method_mut() = Method::CONNECT;
    *request.uri_mut() = authority_uri;
    *request.version_mut() = Version::HTTP_11;
    request.headers_mut().insert(HOST, host);
    if let Some(credentials) = config.credentials {
        request
            .headers_mut()
            .insert(PROXY_AUTHORIZATION, credentials);
    }

    let mut response = pin!(sender.send_request(request));
    let response = future::poll_fn(|cx| {
        let _ = conn.poll_without_shutdown(cx)?;
        response.as_mut().poll(cx)
    })
    .await?;

    if !response.status().is_success() {
        return Err(ProxyTunnelError {
            status: response.status(),
        }
        .into());
    }

    Ok(conn.into_parts().io)
}

#[derive(Debug)]
pub struct ProxyConnection<T> {
    stream: T,
    proxy: bool,
}

impl<T> Read for ProxyConnection<T>
where
    T: Read + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl<T> Write for ProxyConnection<T>
where
    T: Write + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

impl<T> Connection for ProxyConnection<T>
where
    T: Connection,
{
    fn connected(&self) -> Connected {
        self.stream.connected().proxy(self.proxy)
    }
}

#[derive(Debug)]
struct ProxyTunnelError {
    status: StatusCode,
}

impl fmt::Display for ProxyTunnelError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "got status {} from HTTPS proxy", self.status)
    }
}

impl error::Error for ProxyTunnelError {}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config::{self, BasicCredentials, HostAndPort};
    use hyper_util::rt::TokioIo;
    use tower_util::ServiceExt;

    struct MockConnection(TokioIo<tokio_test::io::Mock>);

    impl Read for MockConnection {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: ReadBufCursor<'_>,
        ) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_read(cx, buf)
        }
    }

    impl Write for MockConnection {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.0).poll_write(cx, buf)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_flush(cx)
        }

        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_shutdown(cx)
        }
    }

    impl Connection for MockConnection {
        fn connected(&self) -> Connected {
            Connected::new()
        }
    }

    #[tokio::test]
    async fn unproxied() {
        let service = ProxyConnectorLayer::new(&ProxyConfig::Direct).layer(tower_util::service_fn(
            |uri: Uri| async move {
                assert_eq!(uri, "http://foobar.com");
                Ok::<_, Box<dyn error::Error + Sync + Send>>(MockConnection(TokioIo::new(
                    tokio_test::io::Builder::new().build(),
                )))
            },
        ));

        let conn = service
            .oneshot("http://foobar.com".parse().unwrap())
            .await
            .unwrap();

        assert!(!conn.connected().is_proxied())
    }

    #[tokio::test]
    async fn http_proxied_http() {
        let config = ProxyConfig::from_config(&config::ProxyConfig::Http(
            config::HttpProxyConfig::builder()
                .host_and_port(HostAndPort::new("127.0.0.1", 1234))
                .build(),
        ))
        .unwrap();
        let service = ProxyConnectorLayer::new(&config).layer(tower_util::service_fn(
            |uri: Uri| async move {
                assert_eq!(uri, "http://127.0.0.1:1234");
                Ok::<_, Box<dyn error::Error + Sync + Send>>(MockConnection(TokioIo::new(
                    tokio_test::io::Builder::new().build(),
                )))
            },
        ));

        let conn = service
            .oneshot("http://foobar.com".parse().unwrap())
            .await
            .unwrap();

        assert!(conn.connected().is_proxied())
    }

    #[tokio::test]
    async fn http_proxied_https() {
        let config = ProxyConfig::from_config(&config::ProxyConfig::Http(
            config::HttpProxyConfig::builder()
                .host_and_port(HostAndPort::new("127.0.0.1", 1234))
                .credentials(Some(BasicCredentials::new("admin", "hunter2")))
                .build(),
        ))
        .unwrap();
        let service = ProxyConnectorLayer::new(&config).layer(tower_util::service_fn(
            |uri: Uri| async move {
                assert_eq!(uri, "http://127.0.0.1:1234");
                let mut builder = tokio_test::io::Builder::new();
                builder.write(
                    b"CONNECT foobar.com:443 HTTP/1.1\r\n\
                    host: foobar.com:443\r\n\
                    proxy-authorization: Basic YWRtaW46aHVudGVyMg==\r\n\r\n",
                );
                builder.read(b"HTTP/1.1 200 OK\r\n\r\n");
                Ok::<_, Box<dyn error::Error + Sync + Send>>(MockConnection(TokioIo::new(
                    builder.build(),
                )))
            },
        ));

        let conn = service
            .oneshot("https://admin:hunter2@foobar.com/fizzbuzz".parse().unwrap())
            .await
            .unwrap();

        assert!(!conn.connected().is_proxied())
    }

    #[tokio::test]
    async fn http_proxied_https_error() {
        let config = ProxyConfig::from_config(&config::ProxyConfig::Http(
            config::HttpProxyConfig::builder()
                .host_and_port(HostAndPort::new("127.0.0.1", 1234))
                .build(),
        ))
        .unwrap();
        let service = ProxyConnectorLayer::new(&config).layer(tower_util::service_fn(
            |uri: Uri| async move {
                assert_eq!(uri, "http://127.0.0.1:1234");
                let mut builder = tokio_test::io::Builder::new();
                builder.write(
                    b"CONNECT foobar.com:443 HTTP/1.1\r\n\
                    host: foobar.com:443\r\n\r\n",
                );
                builder.read(b"HTTP/1.1 401 Unauthorized\r\n\r\n");
                Ok::<_, Box<dyn error::Error + Sync + Send>>(MockConnection(TokioIo::new(
                    builder.build(),
                )))
            },
        ));

        let err = service
            .oneshot("https://admin:hunter2@foobar.com/fizzbuzz".parse().unwrap())
            .await
            .err()
            .unwrap()
            .downcast::<ProxyTunnelError>()
            .unwrap();

        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
    }
}
