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
use crate::raw::{BuildRawClient, RawBody, Service};
use crate::service::proxy::connector::ProxyConnectorLayer;
use crate::service::proxy::{ProxyConfig, ProxyConnectorService};
use crate::service::timeout::{TimeoutLayer, TimeoutService};
use crate::service::tls_metrics::{TlsMetricsLayer, TlsMetricsService};
use crate::{builder, Builder};
use bytes::Bytes;
use conjure_error::Error;
use http::{Request, Response};
use http_body::{Body, Frame, SizeHint};
use hyper::body::Incoming;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioTimer};
use pin_project::pin_project;
use rustls::crypto::aws_lc_rs;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};
use rustls_pemfile::Item;
use std::error;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::marker::PhantomPinned;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tower_layer::Layer;
use webpki_roots::TLS_SERVER_ROOTS;

// This is pretty arbitrary - I just grabbed it from some Cloudflare blog post.
const TCP_KEEPALIVE: Duration = Duration::from_secs(3 * 60);
// Most servers time out idle connections after 60 seconds, so we'll set the client timeout a bit below that.
const HTTP_KEEPALIVE: Duration = Duration::from_secs(55);

type ConjureConnector =
    TlsMetricsService<HttpsConnector<ProxyConnectorService<TimeoutService<HttpConnector>>>>;

/// The default raw client builder used by `conjure_runtime`.
#[derive(Copy, Clone)]
pub struct DefaultRawClientBuilder;

impl BuildRawClient for DefaultRawClientBuilder {
    type RawClient = DefaultRawClient;

    fn build_raw_client(
        &self,
        builder: &Builder<builder::Complete<Self>>,
    ) -> Result<Self::RawClient, Error> {
        let mut connector = HttpConnector::new();
        connector.enforce_http(false);
        connector.set_nodelay(true);
        connector.set_keepalive(Some(TCP_KEEPALIVE));
        connector.set_connect_timeout(Some(builder.get_connect_timeout()));

        let proxy = ProxyConfig::from_config(builder.get_proxy())?;

        let connector = TimeoutLayer::new(builder).layer(connector);
        let connector = ProxyConnectorLayer::new(&proxy).layer(connector);

        let mut roots = RootCertStore::empty();
        roots.extend(TLS_SERVER_ROOTS.iter().cloned());

        if let Some(ca_file) = builder.get_security().ca_file() {
            let certs = load_certs_file(ca_file)?;
            roots.add_parsable_certificates(certs);
        }

        let client_config =
            ClientConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(Error::internal_safe)?
                .with_root_certificates(roots);

        let client_config = match (
            builder.get_security().cert_file(),
            builder.get_security().key_file(),
        ) {
            (Some(cert_file), Some(key_file)) => {
                let cert_chain = load_certs_file(cert_file)?;
                let private_key = load_private_key(key_file)?;

                client_config
                    .with_client_auth_cert(cert_chain, private_key)
                    .map_err(Error::internal_safe)?
            }
            (None, None) => client_config.with_no_client_auth(),
            _ => {
                return Err(Error::internal_safe(
                    "neither or both of key-file and cert-file must be set in the client \
                    security config",
                ));
            }
        };

        let connector = HttpsConnectorBuilder::new()
            .with_tls_config(client_config)
            .https_or_http()
            .enable_all_versions()
            .wrap_connector(connector);
        let connector = TlsMetricsLayer::new(builder).layer(connector);

        let client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(HTTP_KEEPALIVE)
            .pool_timer(TokioTimer::new())
            .timer(TokioTimer::new())
            .build(connector);

        Ok(DefaultRawClient(client))
    }
}

fn load_certs_file(path: &Path) -> Result<Vec<CertificateDer<'static>>, Error> {
    let file = File::open(path).map_err(Error::internal_safe)?;
    let mut file = BufReader::new(file);
    rustls_pemfile::certs(&mut file)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Error::internal_safe)
}

fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, Error> {
    let file = File::open(path).map_err(Error::internal_safe)?;
    let mut reader = BufReader::new(file);

    let mut items = rustls_pemfile::read_all(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Error::internal_safe)?;

    if items.len() != 1 {
        return Err(Error::internal_safe(
            "expected exactly one private key in key file",
        ));
    }

    match items.pop().unwrap() {
        Item::Pkcs1Key(key) => Ok(key.into()),
        Item::Pkcs8Key(key) => Ok(key.into()),
        Item::Sec1Key(key) => Ok(key.into()),
        _ => Err(Error::internal_safe(
            "expected a PKCS#1, PKCS#8, or Sec1 private key",
        )),
    }
}

/// The default raw client implementation used by `conjure_runtime`.
///
/// This is currently implemented with `hyper` and `rustls`, but that is subject to change at any time.
pub struct DefaultRawClient(Client<ConjureConnector, RawBody>);

impl Service<Request<RawBody>> for DefaultRawClient {
    type Response = Response<DefaultRawBody>;
    type Error = DefaultRawError;

    async fn call(&self, req: Request<RawBody>) -> Result<Self::Response, Self::Error> {
        self.0
            .request(req)
            .await
            .map(|r| {
                r.map(|inner| DefaultRawBody {
                    inner,
                    _p: PhantomPinned,
                })
            })
            .map_err(DefaultRawError::new)
    }
}

/// The body type used by `DefaultRawClient`.
#[pin_project]
pub struct DefaultRawBody {
    #[pin]
    inner: Incoming,
    #[pin]
    _p: PhantomPinned,
}

impl Body for DefaultRawBody {
    type Data = Bytes;
    type Error = DefaultRawError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.project()
            .inner
            .poll_frame(cx)
            .map(|o| o.map(|r| r.map_err(DefaultRawError::new)))
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

/// The error type used by `DefaultRawClient`.
#[derive(Debug)]
pub struct DefaultRawError(Box<dyn error::Error + Sync + Send>);

impl DefaultRawError {
    fn new<T>(e: T) -> Self
    where
        T: Into<Box<dyn error::Error + Sync + Send>>,
    {
        DefaultRawError(e.into())
    }
}

impl fmt::Display for DefaultRawError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, fmt)
    }
}

impl error::Error for DefaultRawError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        error::Error::source(&*self.0)
    }
}
