// Copyright 2025 Palantir Technologies, Inc.
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
use crate::service::proxy::connector::ProxyConnectorLayer;
use crate::service::proxy::ProxyConnectorService;
use crate::service::raw::RawRequestBody;
use crate::service::timeout::{TimeoutLayer, TimeoutService};
use crate::service::tls_metrics::{TlsMetricsLayer, TlsMetricsService};
use crate::service::Service;
use crate::{builder, Builder};
use bytes::Bytes;
use conjure_error::Error;
use http::{Request, Response};
use http_body::{Body, Frame, SizeHint};
use hyper::body::Incoming;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::{self, Client};
use hyper_util::rt::{TokioExecutor, TokioTimer};
use pin_project::pin_project;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{self, ring, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use rustls_pemfile::Item;
use std::fs::File;
use std::io::BufReader;
use std::marker::PhantomPinned;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use subtle::{Choice, ConstantTimeEq};
use tower_layer::Layer;
use webpki_roots::TLS_SERVER_ROOTS;

// This is pretty arbitrary - I just grabbed it from some Cloudflare blog post.
const TCP_KEEPALIVE: Duration = Duration::from_secs(3 * 60);
// Most servers time out idle connections after 60 seconds, so we'll set the client timeout a bit below that.
const HTTP_KEEPALIVE: Duration = Duration::from_secs(55);

type ConjureConnector =
    TlsMetricsService<HttpsConnector<ProxyConnectorService<TimeoutService<HttpConnector>>>>;

pub struct RawClient(Client<ConjureConnector, RawRequestBody>);

impl RawClient {
    pub fn new(builder: &Builder<builder::Complete>) -> Result<Self, Error> {
        let mut connector = HttpConnector::new();
        connector.enforce_http(false);
        connector.set_nodelay(true);
        connector.set_keepalive(Some(TCP_KEEPALIVE));
        connector.set_connect_timeout(Some(builder.get_connect_timeout()));

        let connector = TimeoutLayer::new(builder).layer(connector);
        let connector = ProxyConnectorLayer::new(builder)?.layer(connector);

        let provider = Arc::new(ring::default_provider());

        let builder_stage = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(Error::internal_safe)?;

        let unauthed_config = if builder.get_security().pinned_certs().is_empty() {
            let mut roots = RootCertStore::empty();
            roots.extend(TLS_SERVER_ROOTS.iter().cloned());

            if let Some(ca_file) = builder.get_security().ca_file() {
                let certs = load_certs_file(ca_file)?;
                roots.add_parsable_certificates(certs);
            }
            builder_stage.with_root_certificates(roots)
        } else {
            let mut pinned = Vec::with_capacity(builder.get_security().pinned_certs().len());
            for (idx, pem) in builder.get_security().pinned_certs().iter().enumerate() {
                let mut reader = BufReader::new(pem.as_bytes());
                let mut certs = rustls_pemfile::certs(&mut reader)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(Error::internal_safe)?;
                if certs.len() != 1 {
                    return Err(Error::internal_safe(
                        "pinned-certs entry must contain exactly one PEM-encoded certificate",
                    )
                    .with_safe_param("index", idx)
                    .with_safe_param("count", certs.len()));
                }
                pinned.push(certs.pop().unwrap());
            }
            let verifier = Arc::new(PinnedLeafVerifier::new(pinned, provider.clone()));
            builder_stage
                .dangerous()
                .with_custom_certificate_verifier(verifier)
        };

        let client_config = match (
            builder.get_security().cert_file(),
            builder.get_security().key_file(),
        ) {
            (Some(cert_file), Some(key_file)) => {
                let cert_chain = load_certs_file(cert_file)?;
                let private_key = load_private_key(key_file)?;

                unauthed_config
                    .with_client_auth_cert(cert_chain, private_key)
                    .map_err(Error::internal_safe)?
            }
            (None, None) => unauthed_config.with_no_client_auth(),
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

        Ok(RawClient(client))
    }
}

impl Service<Request<RawRequestBody>> for RawClient {
    type Response = Response<RawResponseBody>;
    type Error = legacy::Error;

    async fn call(&self, req: Request<RawRequestBody>) -> Result<Self::Response, Self::Error> {
        self.0.request(req).await.map(|r| {
            r.map(|inner| RawResponseBody {
                inner,
                _p: PhantomPinned,
            })
        })
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

/// A `ServerCertVerifier` that accepts a connection iff the server's end-entity certificate exactly
/// matches one of a configured set of pinned leaf certificates.
///
/// The certificate chain is *not* validated. Pinning the leaf is sufficient to identify the server,
/// since rustls separately verifies the handshake signature against the leaf's public key (see
/// `verify_tls12_signature` / `verify_tls13_signature`), which proves the server holds the matching
/// private key.
///
/// This is intended as an escape hatch for environments whose CA hierarchy uses X.509 features
/// the underlying TLS library does not support (e.g. `directoryName` name constraints).
#[derive(Debug)]
struct PinnedLeafVerifier {
    pinned: Vec<CertificateDer<'static>>,
    provider: Arc<CryptoProvider>,
}

impl PinnedLeafVerifier {
    fn new(pinned: Vec<CertificateDer<'static>>, provider: Arc<CryptoProvider>) -> Self {
        PinnedLeafVerifier { pinned, provider }
    }
}

impl ServerCertVerifier for PinnedLeafVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let mut matched = Choice::from(0);
        for pin in &self.pinned {
            matched |= pin.as_ref().ct_eq(end_entity.as_ref());
        }
        if bool::from(matched) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::UnknownIssuer,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[pin_project]
pub struct RawResponseBody {
    #[pin]
    inner: Incoming,
    #[pin]
    _p: PhantomPinned,
}

impl Body for RawResponseBody {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.project().inner.poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}
