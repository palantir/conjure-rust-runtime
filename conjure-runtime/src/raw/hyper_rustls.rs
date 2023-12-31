// Copyright 2023 Palantir Technologies, Inc.
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
use std::{
    convert::TryFrom,
    error::Error,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::future::BoxFuture;
use http::{uri::Scheme, Uri};
use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::{
    client::legacy::connect::{Connected, Connection},
    rt::TokioIo,
};
use pin_project::pin_project;
use rustls::{pki_types::ServerName, ClientConfig};
use tokio_rustls::{client::TlsStream, TlsConnector};
use tower_service::Service;

#[pin_project(project = MaybeHttpsStreamProj)]
#[allow(clippy::large_enum_variant)]
pub enum MaybeHttpsStream<T> {
    Http(#[pin] T),
    Https(#[pin] TokioIo<TlsStream<TokioIo<T>>>),
}

impl<T> Read for MaybeHttpsStream<T>
where
    T: Read + Write + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        match self.project() {
            MaybeHttpsStreamProj::Http(s) => s.poll_read(cx, buf),
            MaybeHttpsStreamProj::Https(s) => s.poll_read(cx, buf),
        }
    }
}

impl<T> Write for MaybeHttpsStream<T>
where
    T: Read + Write + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            MaybeHttpsStreamProj::Http(s) => s.poll_write(cx, buf),
            MaybeHttpsStreamProj::Https(s) => s.poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.project() {
            MaybeHttpsStreamProj::Http(s) => s.poll_flush(cx),
            MaybeHttpsStreamProj::Https(s) => s.poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.project() {
            MaybeHttpsStreamProj::Http(s) => s.poll_shutdown(cx),
            MaybeHttpsStreamProj::Https(s) => s.poll_shutdown(cx),
        }
    }
}

impl<T> Connection for MaybeHttpsStream<T>
where
    T: Connection,
{
    fn connected(&self) -> Connected {
        match self {
            MaybeHttpsStream::Http(s) => s.connected(),
            MaybeHttpsStream::Https(s) => {
                let mut connected = s.inner().get_ref().0.inner().connected();
                if s.inner().get_ref().1.alpn_protocol() == Some(b"h2") {
                    connected = connected.negotiated_h2();
                }
                connected
            }
        }
    }
}

#[derive(Clone)]
pub struct HttpsConnector<T> {
    inner: T,
    tls_connector: TlsConnector,
}

impl<T> HttpsConnector<T> {
    pub fn new(inner: T, client_config: ClientConfig) -> Self {
        HttpsConnector {
            inner,
            tls_connector: TlsConnector::from(Arc::new(client_config)),
        }
    }
}

impl<T> Service<Uri> for HttpsConnector<T>
where
    T: Service<Uri> + Clone + Sync + Send + 'static,
    T::Response: Read + Write + Unpin + Send,
    T::Error: Into<Box<dyn Error + Sync + Send>>,
    T::Future: Send,
{
    type Response = MaybeHttpsStream<T::Response>;

    type Error = Box<dyn Error + Sync + Send>;

    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: Uri) -> Self::Future {
        Box::pin({
            let mut this = self.clone();
            async move {
                let domain = if req.scheme() == Some(&Scheme::HTTPS) {
                    Some(ServerName::try_from(req.host().ok_or("URI missing host")?)?.to_owned())
                } else {
                    None
                };
                let s = this.inner.call(req).await.map_err(Into::into)?;
                match domain {
                    Some(domain) => {
                        let s = this.tls_connector.connect(domain, TokioIo::new(s)).await?;
                        Ok(MaybeHttpsStream::Https(TokioIo::new(s)))
                    }
                    None => Ok(MaybeHttpsStream::Http(s)),
                }
            }
        })
    }
}
