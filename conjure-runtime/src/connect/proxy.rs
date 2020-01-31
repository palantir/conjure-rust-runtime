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
use bytes::{Buf, BufMut};
use conjure_error::Error;
use conjure_runtime_config::HostAndPort;
use futures::future;
use futures::FutureExt;
use http::uri::Scheme;
use hyper::client::conn;
use hyper::client::connect::{Connected, Connection};
use hyper::header::HeaderValue;
use hyper::http::header::{HOST, PROXY_AUTHORIZATION};
use hyper::service::Service;
use hyper::{Body, Method, Request, Uri, Version};
use std::future::Future;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::{error, io};
use tokio::io::{AsyncRead, AsyncWrite};

pub enum ProxyConfig {
    Http(HttpProxyConfig),
    Mesh(MeshProxyConfig),
    Direct,
}

#[derive(Clone)]
pub struct HttpProxyConfig {
    pub uri: Uri,
    pub host: HeaderValue,
    pub credentials: Option<HeaderValue>,
}

pub struct MeshProxyConfig {
    pub host_and_port: HostAndPort,
}

impl ProxyConfig {
    pub fn from_config(config: &conjure_runtime_config::ProxyConfig) -> Result<ProxyConfig, Error> {
        match config {
            conjure_runtime_config::ProxyConfig::Http(config) => {
                let uri = format!(
                    "http://{}:{}",
                    config.host_and_port().host(),
                    config.host_and_port().port(),
                )
                .parse::<Uri>()
                .map_err(Error::internal_safe)?;

                let authority = uri.authority().unwrap().to_string();
                let host = HeaderValue::from_str(&authority).unwrap();

                let credentials = config.credentials().map(|c| {
                    let auth = format!("{}:{}", c.username(), c.password());
                    let auth = base64::encode(&auth);
                    let header = format!("Basic {}", auth);
                    HeaderValue::from_str(&header).unwrap()
                });

                Ok(ProxyConfig::Http(HttpProxyConfig {
                    uri,
                    host,
                    credentials,
                }))
            }
            conjure_runtime_config::ProxyConfig::Mesh(config) => {
                Ok(ProxyConfig::Mesh(MeshProxyConfig {
                    host_and_port: config.host_and_port().clone(),
                }))
            }
            conjure_runtime_config::ProxyConfig::Direct => Ok(ProxyConfig::Direct),
            _ => Err(Error::internal_safe("unknown proxy type")),
        }
    }
}

#[derive(Clone)]
pub struct ProxyConnector<S> {
    connector: S,
    proxy: Option<HttpProxyConfig>,
}

impl<S> ProxyConnector<S>
where
    S: Service<Uri> + Send,
    S::Error: Into<Box<dyn error::Error + Send + Sync>>,
    S::Future: Unpin + Send + 'static,
    S::Response: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    pub fn new(connector: S, config: &ProxyConfig) -> ProxyConnector<S> {
        let proxy = match config {
            ProxyConfig::Http(config) => Some(config.clone()),
            _ => None,
        };
        ProxyConnector { connector, proxy }
    }
}

impl<S> Service<Uri> for ProxyConnector<S>
where
    S: Service<Uri> + Send,
    S::Error: Into<Box<dyn error::Error + Send + Sync>> + 'static,
    S::Future: Unpin + Send + 'static,
    S::Response: AsyncRead + AsyncWrite + Connection + Unpin + Send + 'static,
{
    type Response = ProxyConnection<S::Response>;
    type Error = Box<dyn error::Error + Sync + Send>;
    #[allow(clippy::type_complexity)]
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.connector.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let proxy = match &self.proxy {
            Some(proxy) => proxy.clone(),
            None => {
                let connect = self.connector.call(uri);
                return Box::pin(async move {
                    let stream = connect.await.map_err(Into::into)?;
                    Ok(ProxyConnection {
                        stream,
                        proxy: false,
                    })
                });
            }
        };

        let connect = self.connector.call(proxy.uri.clone());

        let f = async move {
            let stream = connect.await.map_err(Into::into)?;

            if uri.scheme() == Some(&Scheme::HTTP) {
                Ok(ProxyConnection {
                    stream,
                    proxy: true,
                })
            } else {
                let stream = connect_https(stream, uri, proxy).await?;
                Ok(ProxyConnection {
                    stream,
                    proxy: false,
                })
            }
        };
        Box::pin(f)
    }
}

async fn connect_https<T>(
    stream: T,
    uri: Uri,
    proxy: HttpProxyConfig,
) -> Result<T, Box<dyn error::Error + Sync + Send>>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (mut sender, mut conn) = conn::handshake(stream).await?;

    let host = uri.host().ok_or("host missing from URI")?;
    let host = format!("{}:{}", host, uri.port_u16().unwrap_or(443))
        .parse()
        .unwrap();

    let mut request = Request::new(Body::empty());
    *request.method_mut() = Method::CONNECT;
    *request.uri_mut() = proxy.uri;
    *request.version_mut() = Version::HTTP_11;
    request.headers_mut().insert(HOST, host);
    if let Some(credentials) = proxy.credentials {
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
        return Err(format!("got status {} from HTTPS proxy", response.status()).into());
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

    fn poll_read_buf<B: BufMut>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut B,
    ) -> Poll<io::Result<usize>>
    where
        Self: Sized,
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

    fn poll_write_buf<B: Buf>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut B,
    ) -> Poll<io::Result<usize>>
    where
        Self: Sized,
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
