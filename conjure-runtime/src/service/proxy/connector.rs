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
use bytes::{Buf, BufMut};
use futures::future::{self, BoxFuture};
use futures::FutureExt;
use http::header::{HOST, PROXY_AUTHORIZATION};
use http::uri::Scheme;
use http::{HeaderValue, Method, Request, StatusCode, Uri, Version};
use hyper::client::conn;
use hyper::client::connect::{Connected, Connection};
use hyper::Body;
use std::convert::TryFrom;
use std::error;
use std::fmt;
use std::io;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};
use tower::layer::Layer;
use tower::Service;

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
    S::Error: Into<Box<dyn error::Error + Sync + Send>>,
    S::Future: Send + 'static,
    S::Response: AsyncRead + AsyncWrite + Unpin + 'static + Send,
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
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (mut sender, mut conn) = conn::handshake(stream).await?;

    let host = uri.host().ok_or("host missing from URI")?;
    let authority = format!("{}:{}", host, uri.port_u16().unwrap_or(443));
    let authority_uri = Uri::try_from(authority.clone()).unwrap();
    let host = HeaderValue::try_from(authority).unwrap();

    let mut request = Request::new(Body::empty());
    *request.method_mut() = Method::CONNECT;
    *request.uri_mut() = authority_uri;
    *request.version_mut() = Version::HTTP_11;
    request.headers_mut().insert(HOST, host);
    if let Some(credentials) = config.credentials {
        request
            .headers_mut()
            .insert(PROXY_AUTHORIZATION, credentials);
    }

    let mut response = sender.send_request(request);
    let response = future::poll_fn(|cx| {
        let _ = conn.poll_without_shutdown(cx)?;
        response.poll_unpin(cx)
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

impl<T> AsyncRead for ProxyConnection<T>
where
    T: AsyncRead + Unpin,
{
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [MaybeUninit<u8>]) -> bool {
        self.stream.prepare_uninitialized_buffer(buf)
    }

    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }

    fn poll_read_buf<B>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut B,
    ) -> Poll<io::Result<usize>>
    where
        Self: Sized,
        B: BufMut,
    {
        Pin::new(&mut self.stream).poll_read_buf(cx, buf)
    }
}

impl<T> AsyncWrite for ProxyConnection<T>
where
    T: AsyncWrite + Unpin,
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

    fn poll_write_buf<B>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut B,
    ) -> Poll<io::Result<usize>>
    where
        Self: Sized,
        B: Buf,
    {
        Pin::new(&mut self.stream).poll_write_buf(cx, buf)
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
    use tower::ServiceExt;

    struct MockConnection(tokio_test::io::Mock);

    impl AsyncRead for MockConnection {
        unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [MaybeUninit<u8>]) -> bool {
            self.0.prepare_uninitialized_buffer(buf)
        }

        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.0).poll_read(cx, buf)
        }

        fn poll_read_buf<B>(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut B,
        ) -> Poll<io::Result<usize>>
        where
            Self: Sized,
            B: BufMut,
        {
            Pin::new(&mut self.0).poll_read_buf(cx, buf)
        }
    }

    impl AsyncWrite for MockConnection {
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

        fn poll_write_buf<B>(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut B,
        ) -> Poll<io::Result<usize>>
        where
            Self: Sized,
            B: Buf,
        {
            Pin::new(&mut self.0).poll_write_buf(cx, buf)
        }
    }

    impl Connection for MockConnection {
        fn connected(&self) -> Connected {
            Connected::new()
        }
    }

    #[tokio::test]
    async fn unproxied() {
        let service = ProxyConnectorLayer::new(&ProxyConfig::Direct).layer(tower::service_fn(
            |uri: Uri| async move {
                assert_eq!(uri, "http://foobar.com");
                Ok::<_, Box<dyn error::Error + Sync + Send>>(MockConnection(
                    tokio_test::io::Builder::new().build(),
                ))
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
        let service =
            ProxyConnectorLayer::new(&config).layer(tower::service_fn(|uri: Uri| async move {
                assert_eq!(uri, "http://127.0.0.1:1234");
                Ok::<_, Box<dyn error::Error + Sync + Send>>(MockConnection(
                    tokio_test::io::Builder::new().build(),
                ))
            }));

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
        let service =
            ProxyConnectorLayer::new(&config).layer(tower::service_fn(|uri: Uri| async move {
                assert_eq!(uri, "http://127.0.0.1:1234");
                let mut builder = tokio_test::io::Builder::new();
                builder.write(
                    b"CONNECT foobar.com:443 HTTP/1.1\r\n\
                    host: foobar.com:443\r\n\
                    proxy-authorization: Basic YWRtaW46aHVudGVyMg==\r\n\r\n",
                );
                builder.read(b"HTTP/1.1 200 OK\r\n\r\n");
                Ok::<_, Box<dyn error::Error + Sync + Send>>(MockConnection(builder.build()))
            }));

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
        let service =
            ProxyConnectorLayer::new(&config).layer(tower::service_fn(|uri: Uri| async move {
                assert_eq!(uri, "http://127.0.0.1:1234");
                let mut builder = tokio_test::io::Builder::new();
                builder.write(
                    b"CONNECT foobar.com:443 HTTP/1.1\r\n\
                    host: foobar.com:443\r\n\r\n",
                );
                builder.read(b"HTTP/1.1 401 Unauthorized\r\n\r\n");
                Ok::<_, Box<dyn error::Error + Sync + Send>>(MockConnection(builder.build()))
            }));

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
