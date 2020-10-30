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
use crate::raw::BuildRawClient;
use crate::raw::RawBody;
use crate::service::proxy::{ProxyConfig, ProxyConnectorLayer, ProxyConnectorService};
use crate::service::tls_metrics::{TlsMetricsLayer, TlsMetricsService};
use crate::Builder;
use bytes::Bytes;
use conjure_error::Error;
use futures::ready;
use http::{HeaderMap, Request, Response};
use http_body::{Body, SizeHint};
use hyper::client::{HttpConnector, ResponseFuture};
use hyper::Client;
use hyper_openssl::{HttpsConnector, HttpsLayer};
use openssl::ssl::{SslConnector, SslMethod};
use pin_project::pin_project;
use std::error;
use std::fmt;
use std::future::Future;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tower::Service;
use tower::ServiceBuilder;

// This is pretty arbitrary - I just grabbed it from some Cloudflare blog post.
const TCP_KEEPALIVE: Duration = Duration::from_secs(3 * 60);
// Most servers time out idle connections after 60 seconds, so we'll set the client timeout a bit below that.
const HTTP_KEEPALIVE: Duration = Duration::from_secs(55);

type ConjureConnector = TlsMetricsService<HttpsConnector<ProxyConnectorService<HttpConnector>>>;

/// The default raw client builder used by `conjure-rust`.
#[derive(Copy, Clone)]
pub struct DefaultRawClientBuilder;

impl BuildRawClient for DefaultRawClientBuilder {
    type RawClient = DefaultRawClient;

    fn build_raw_client(&self, builder: &Builder<Self>) -> Result<Self::RawClient, Error> {
        let service = builder
            .get_service()
            .ok_or_else(|| Error::internal_safe("service missing from builder"))?;

        let mut connector = HttpConnector::new();
        connector.enforce_http(false);
        connector.set_nodelay(true);
        connector.set_keepalive(Some(TCP_KEEPALIVE));
        connector.set_connect_timeout(Some(builder.get_connect_timeout()));

        let mut ssl = SslConnector::builder(SslMethod::tls()).map_err(Error::internal_safe)?;
        ssl.set_alpn_protos(b"\x02h2\x08http/1.1")
            .map_err(Error::internal_safe)?;

        if let Some(ca_file) = builder.get_security().ca_file() {
            ssl.set_ca_file(ca_file).map_err(Error::internal_safe)?;
        }

        let proxy = ProxyConfig::from_config(&builder.get_proxy())?;

        let connector = ServiceBuilder::new()
            .layer(TlsMetricsLayer::new(&service, builder))
            .layer(HttpsLayer::with_connector(ssl).map_err(Error::internal_safe)?)
            .layer(ProxyConnectorLayer::new(&proxy))
            .service(connector);

        let client = hyper::Client::builder()
            .pool_idle_timeout(HTTP_KEEPALIVE)
            .build(connector);

        Ok(DefaultRawClient(client))
    }
}

/// The default raw client implementation used by `conjure-rust`.
///
/// This is currently implemented with `hyper`, but that is subject to change at any time.
#[derive(Clone)]
pub struct DefaultRawClient(Client<ConjureConnector, RawBody>);

impl Service<Request<RawBody>> for DefaultRawClient {
    type Response = Response<DefaultRawBody>;
    type Error = DefaultRawError;
    type Future = DefaultRawFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx).map_err(DefaultRawError)
    }

    fn call(&mut self, req: Request<RawBody>) -> Self::Future {
        DefaultRawFuture {
            future: self.0.call(req),
            _p: PhantomPinned,
        }
    }
}

/// The body type used by `DefaultRawClient`.
#[pin_project]
pub struct DefaultRawBody {
    #[pin]
    inner: hyper::Body,
    #[pin]
    _p: PhantomPinned,
}

impl Body for DefaultRawBody {
    type Data = Bytes;
    type Error = DefaultRawError;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        self.project()
            .inner
            .poll_data(cx)
            .map(|o| o.map(|r| r.map_err(DefaultRawError)))
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        self.project()
            .inner
            .poll_trailers(cx)
            .map_err(DefaultRawError)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

/// The future type used by `DefaultRawClient`.
#[pin_project]
pub struct DefaultRawFuture {
    #[pin]
    future: ResponseFuture,
    #[pin]
    _p: PhantomPinned,
}

impl Future for DefaultRawFuture {
    type Output = Result<Response<DefaultRawBody>, DefaultRawError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let response = ready!(self.project().future.poll(cx)).map_err(DefaultRawError)?;
        let response = response.map(|inner| DefaultRawBody {
            inner,
            _p: PhantomPinned,
        });

        Poll::Ready(Ok(response))
    }
}

/// The error type used by `DefaultRawClient`.
#[derive(Debug)]
pub struct DefaultRawError(hyper::Error);

impl fmt::Display for DefaultRawError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, fmt)
    }
}

impl error::Error for DefaultRawError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        error::Error::source(&self.0)
    }
}
