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
use crate::errors::RemoteError;
use crate::{blocking, Agent, BodyWriter, Builder, Client, ServerQos, ServiceError, UserAgent};
use bytes::Bytes;
use conjure_error::NotFound;
use conjure_error::{Error, ErrorKind};
use conjure_http::client::{
    AsyncClient, AsyncRequestBody, AsyncWriteBody, BoxAsyncWriteBody, Client as _, Endpoint,
    RequestBody,
};
use conjure_runtime_config::ServiceConfig;
use flate2::write::GzEncoder;
use flate2::Compression;
use futures::channel::mpsc;
use futures::{join, pin_mut, SinkExt};
use http::header::{CONTENT_LENGTH, TRANSFER_ENCODING};
use http::{request, Method};
use http_body::Frame;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty, Full, StreamBody};
use hyper::body::Incoming;
use hyper::header::{ACCEPT_ENCODING, CONTENT_ENCODING};
use hyper::http::header::RETRY_AFTER;
use hyper::server::conn::http1;
use hyper::service::Service;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use openssl::ssl::{Ssl, SslAcceptor, SslFiletype, SslMethod};
use std::convert::Infallible;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Runtime;
use tokio_openssl::SslStream;
use zipkin::{SpanId, TraceContext, TraceId};

const STOCK_CONFIG: &str = r#"
uris: ["https://localhost:{{port}}"]
security:
  ca-file: "{{ca_file}}"
    "#;

fn test_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../test")
}

fn key_file() -> PathBuf {
    test_dir().join("key.pem")
}

fn cert_file() -> PathBuf {
    test_dir().join("cert.cer")
}

fn ssl_acceptor() -> SslAcceptor {
    let mut acceptor = SslAcceptor::mozilla_modern(SslMethod::tls()).unwrap();
    acceptor
        .set_private_key_file(key_file(), SslFiletype::PEM)
        .unwrap();
    acceptor.set_certificate_chain_file(cert_file()).unwrap();
    acceptor.build()
}

async fn test<F, G>(
    config: &str,
    requests: u32,
    handler: impl Fn(Request<Incoming>) -> F + 'static,
    check: impl FnOnce(Builder) -> G,
) where
    F: Future<Output = Result<Response<BoxBody<Bytes, Infallible>>, &'static str>> + 'static + Send,
    G: Future<Output = ()>,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    join!(
        server(listener, requests, handler),
        client(config, port, check)
    );
}

fn blocking_test<F>(
    config: &str,
    requests: u32,
    handler: impl Fn(Request<Incoming>) -> F + 'static + Send,
    check: impl FnOnce(blocking::Client),
) where
    F: Future<Output = Result<Response<BoxBody<Bytes, Infallible>>, &'static str>> + 'static + Send,
{
    let runtime = Runtime::new().unwrap();
    let listener = runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || runtime.block_on(server(listener, requests, handler)));

    let client = Client::builder()
        .service("service")
        .user_agent(UserAgent::new(Agent::new("test", "1.0")))
        .from_config(&parse_config(config, port))
        .build_blocking()
        .unwrap();

    check(client);
    server.join().unwrap();
}

async fn server<F>(
    listener: TcpListener,
    requests: u32,
    handler: impl Fn(Request<Incoming>) -> F + 'static,
) where
    F: Future<Output = Result<Response<BoxBody<Bytes, Infallible>>, &'static str>> + 'static + Send,
{
    let acceptor = ssl_acceptor();

    for _ in 0..requests {
        let socket = listener.accept().await.unwrap().0;

        let ssl = Ssl::new(acceptor.context()).unwrap();
        let mut socket = SslStream::new(ssl, socket).unwrap();
        Pin::new(&mut socket).accept().await.unwrap();

        let _ = http1::Builder::new()
            .keep_alive(false)
            .serve_connection(TokioIo::new(socket), TestService(&handler))
            .await;
    }
}

async fn client<F>(config: &str, port: u16, check: impl FnOnce(Builder) -> F)
where
    F: Future<Output = ()>,
{
    let builder = Client::builder()
        .service("service")
        .user_agent(UserAgent::new(Agent::new("test", "1.0")))
        .from_config(&parse_config(config, port));
    check(builder).await
}

fn parse_config(config: &str, port: u16) -> ServiceConfig {
    let config = config
        .replace("{{port}}", &port.to_string())
        .replace("{{ca_file}}", &cert_file().display().to_string());
    serde_yaml::from_str(&config).unwrap()
}

struct TestService<'a, F>(&'a F);

impl<F, G> Service<Request<Incoming>> for TestService<'_, F>
where
    F: Fn(Request<Incoming>) -> G,
    G: Future<Output = Result<Response<BoxBody<Bytes, Infallible>>, &'static str>> + 'static + Send,
{
    type Response = Response<BoxBody<Bytes, Infallible>>;
    type Error = &'static str;
    type Future = G;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        (self.0)(req)
    }
}

fn req() -> request::Builder {
    Request::builder().extension(Endpoint::new("service", None, "endpoint", "/"))
}

#[tokio::test]
async fn retry_after_503() {
    let first = AtomicBool::new(true);
    test(
        STOCK_CONFIG,
        2,
        move |_| {
            let inner_first = first.swap(false, Ordering::Relaxed);
            async move {
                if inner_first {
                    Ok(Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .body(Empty::new().boxed())
                        .unwrap())
                } else {
                    Ok(Response::new(Empty::new().boxed()))
                }
            }
        },
        |builder| async move {
            let response = builder
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        },
    )
    .await;
}

#[tokio::test]
async fn no_retry_after_404() {
    // the server's only alive for 1 request, so retries will hit a network error
    test(
        STOCK_CONFIG,
        1,
        |_| async move {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Empty::new().boxed())
                .unwrap())
        },
        |builder| async move {
            let error = builder
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .err()
                .unwrap();
            assert_eq!(
                error
                    .cause()
                    .downcast_ref::<RemoteError>()
                    .unwrap()
                    .status(),
                &StatusCode::NOT_FOUND,
            );
        },
    )
    .await;
}

#[tokio::test]
async fn retry_after_overrides() {
    let first = AtomicBool::new(true);
    test(
        STOCK_CONFIG,
        2,
        move |_| {
            let inner_first = first.swap(false, Ordering::Relaxed);
            async move {
                if inner_first {
                    Ok(Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .header(RETRY_AFTER, "1")
                        .body(Empty::new().boxed())
                        .unwrap())
                } else {
                    Ok(Response::new(Empty::new().boxed()))
                }
            }
        },
        |builder| async move {
            let time = Instant::now();
            let response = builder
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(time.elapsed() >= Duration::from_secs(1));
        },
    )
    .await;
}

#[tokio::test]
async fn connect_error_doesnt_reset_body() {
    struct TestBody(bool);

    impl AsyncWriteBody<BodyWriter> for TestBody {
        async fn write_body(
            mut self: Pin<&mut Self>,
            mut w: Pin<&mut BodyWriter>,
        ) -> Result<(), Error> {
            assert!(!self.0);
            self.0 = true;
            w.write_all(b"hello world").await.unwrap();
            Ok(())
        }

        async fn reset(self: Pin<&mut Self>) -> bool {
            panic!("should not reset");
        }
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = async move {
        // accept and immediately close the socket without completing the TLS handshake
        listener.accept().await.unwrap();

        server(listener, 1, |req| async move {
            let body = req.into_body().collect().await.unwrap();
            assert_eq!(&*body.to_bytes(), b"hello world");
            Ok(Response::new(Empty::new().boxed()))
        })
        .await;
    };

    let client = client(STOCK_CONFIG, port, |builder| async move {
        let response = builder
            .build()
            .unwrap()
            .send(
                req()
                    .method(Method::PUT)
                    .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                        TestBody(false),
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    });

    join!(server, client);
}

#[tokio::test]
async fn propagate_429() {
    test(
        STOCK_CONFIG,
        1,
        |_| async {
            Ok(Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .body(Empty::new().boxed())
                .unwrap())
        },
        |builder| async move {
            let error = builder
                .server_qos(ServerQos::Propagate429And503ToCaller)
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .err()
                .unwrap();
            match error.kind() {
                ErrorKind::Throttle(e) => assert_eq!(e.duration(), None),
                _ => panic!("wrong error kind"),
            }
        },
    )
    .await;
}

#[tokio::test]
async fn propagate_429_with_retry_after() {
    test(
        STOCK_CONFIG,
        1,
        |_| async {
            Ok(Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header(RETRY_AFTER, "100")
                .body(Empty::new().boxed())
                .unwrap())
        },
        |builder| async move {
            let error = builder
                .server_qos(ServerQos::Propagate429And503ToCaller)
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .err()
                .unwrap();
            match error.kind() {
                ErrorKind::Throttle(e) => assert_eq!(e.duration(), Some(Duration::from_secs(100))),
                _ => panic!("wrong error kind"),
            }
        },
    )
    .await;
}

#[tokio::test]
async fn propagate_503() {
    test(
        STOCK_CONFIG,
        1,
        |_| async {
            Ok(Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Empty::new().boxed())
                .unwrap())
        },
        |builder| async move {
            let error = builder
                .server_qos(ServerQos::Propagate429And503ToCaller)
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .err()
                .unwrap();
            match error.kind() {
                ErrorKind::Unavailable(_) => {}
                _ => panic!("wrong error kind"),
            }
        },
    )
    .await;
}

#[tokio::test]
async fn dont_propagate_protocol_errors() {
    let first = AtomicBool::new(true);
    test(
        STOCK_CONFIG,
        2,
        move |_| {
            let inner_first = first.swap(false, Ordering::Relaxed);
            async move {
                if inner_first {
                    Err("")
                } else {
                    Ok(Response::new(Empty::new().boxed()))
                }
            }
        },
        |builder| async move {
            let response = builder
                .server_qos(ServerQos::Propagate429And503ToCaller)
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        },
    )
    .await;
}

#[tokio::test]
async fn dont_bail_when_all_timed_out() {
    let first = AtomicBool::new(true);
    test(
        r#"
uris: ["https://localhost:{{port}}"]
security:
  ca-file: "{{ca_file}}"
failed-url-cooldown: 1h
    "#,
        2,
        move |_| {
            let inner_first = first.swap(false, Ordering::Relaxed);
            async move {
                if inner_first {
                    Err("")
                } else {
                    Ok(Response::new(Empty::new().boxed()))
                }
            }
        },
        |builder| async move {
            let response = builder
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        },
    )
    .await;
}

#[tokio::test]
async fn body_write_ends_after_error() {
    test(
        STOCK_CONFIG,
        1,
        |_| async { Ok(Response::new(Empty::new().boxed())) },
        |builder| {
            async move {
                // This could succeed or fail depending on if we get an EPIPE or the response. The important thing is
                // that we don't deadlock.
                let _ = builder
                    .build()
                    .unwrap()
                    .send(
                        req()
                            .method(Method::POST)
                            .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                                InfiniteBody,
                            )))
                            .unwrap(),
                    )
                    .await;
            }
        },
    )
    .await;
}

#[tokio::test]
async fn streaming_write_error_reporting() {
    struct TestBody;

    impl AsyncWriteBody<BodyWriter> for TestBody {
        async fn write_body(self: Pin<&mut Self>, _: Pin<&mut BodyWriter>) -> Result<(), Error> {
            Err(Error::internal_safe("foobar"))
        }

        async fn reset(self: Pin<&mut Self>) -> bool {
            panic!("should not reset")
        }
    }

    test(
        STOCK_CONFIG,
        1,
        |req| async move {
            let _ = req.into_body().collect().await;
            Ok(Response::new(Empty::new().boxed()))
        },
        |builder| async move {
            let error = builder
                .build()
                .unwrap()
                .send(
                    req()
                        .method(Method::POST)
                        .body(AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(
                            TestBody,
                        )))
                        .unwrap(),
                )
                .await
                .err()
                .unwrap();
            assert_eq!(error.cause().to_string(), "foobar");
        },
    )
    .await;
}

#[tokio::test]
async fn service_error_propagation() {
    test(
        STOCK_CONFIG,
        1,
        |_| async {
            let body = conjure_error::encode(&NotFound::new());
            let body = conjure_serde::json::to_vec(&body).unwrap();
            Ok(Response::builder()
                .status(404)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(body)).boxed())
                .unwrap())
        },
        |builder| async move {
            let error = builder
                .service_error(ServiceError::PropagateToCaller)
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .err()
                .unwrap();
            match error.kind() {
                ErrorKind::Service(e) => assert_eq!(e.error_name(), "Default:NotFound"),
                _ => panic!("invalid error kind"),
            }
        },
    )
    .await;
}

#[tokio::test]
async fn gzip_body() {
    test(
        STOCK_CONFIG,
        1,
        |req| async move {
            assert_eq!(req.headers().get(ACCEPT_ENCODING).unwrap(), "gzip");
            let mut body = GzEncoder::new(vec![], Compression::default());
            body.write_all(b"hello world").unwrap();
            let body = body.finish().unwrap();
            Ok(Response::builder()
                .header(CONTENT_ENCODING, "gzip")
                .body(Full::new(Bytes::from(body)).boxed())
                .unwrap())
        },
        |builder| async move {
            let body = builder
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .unwrap()
                .into_body();
            pin_mut!(body);
            let mut buf = vec![];
            body.read_to_end(&mut buf).await.unwrap();
            assert_eq!(buf, b"hello world");
        },
    )
    .await;
}

#[tokio::test]
async fn zipkin_propagation() {
    let trace_id = TraceId::from([0, 1, 2, 3, 4, 5, 6, 7]);
    test(
        STOCK_CONFIG,
        1,
        |req| async move {
            assert_eq!(
                req.headers().get("X-B3-TraceId").unwrap(),
                "0001020304050607"
            );
            Ok(Response::new(Empty::new().boxed()))
        },
        |builder| {
            let context = TraceContext::builder()
                .trace_id(trace_id)
                .span_id(SpanId::from(*b"abcdefgh"))
                .build();
            zipkin::join_trace(context).detach().bind(async move {
                builder
                    .build()
                    .unwrap()
                    .send(req().body(AsyncRequestBody::Empty).unwrap())
                    .await
                    .unwrap();
            })
        },
    )
    .await;
}

#[test]
fn blocking_zipkin_propagation() {
    let trace_id = TraceId::from([0, 1, 2, 3, 4, 5, 6, 7]);
    blocking_test(
        STOCK_CONFIG,
        1,
        |req| async move {
            assert_eq!(
                req.headers().get("X-B3-TraceId").unwrap(),
                "0001020304050607"
            );
            Ok(Response::new(Empty::new().boxed()))
        },
        |client| {
            let context = TraceContext::builder()
                .trace_id(trace_id)
                .span_id(SpanId::from(*b"abcdefgh"))
                .build();
            let _guard = zipkin::set_current(context);
            client
                .send(req().body(RequestBody::Empty).unwrap())
                .unwrap();
        },
    );
}

#[tokio::test]
async fn read_past_eof() {
    test(
        STOCK_CONFIG,
        1,
        |_| async move {
            let (mut tx, rx) = mpsc::channel(1);
            tokio::spawn(async move {
                tx.send(Ok(Frame::data(Bytes::from("hello"))))
                    .await
                    .unwrap();
                tx.send(Ok(Frame::data(Bytes::from(" world"))))
                    .await
                    .unwrap();
            });
            Ok(Response::new(StreamBody::new(rx).boxed()))
        },
        |builder| async move {
            let body = builder
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .unwrap()
                .into_body();
            pin_mut!(body);
            let mut buf = vec![];
            body.read_to_end(&mut buf).await.unwrap();
            assert_eq!(buf, b"hello world");
            assert_eq!(body.read(&mut [0]).await.unwrap(), 0);
        },
    )
    .await
}

struct InfiniteBody;

impl AsyncWriteBody<BodyWriter> for InfiniteBody {
    async fn write_body(self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
        let buf = [b'a'; 1024];
        loop {
            w.write_all(&buf).await.map_err(Error::internal_safe)?;
        }
    }

    async fn reset(self: Pin<&mut Self>) -> bool {
        panic!("should not reset");
    }
}

#[tokio::test]
async fn mesh_mode() {
    test(
        r#"
uris: ["mesh-https://localhost:{{port}}"]
security:
  ca-file: "{{ca_file}}"
        "#,
        1,
        |_| async move { Ok(Response::new(Empty::new().boxed())) },
        |builder| async move {
            builder
                .build()
                .unwrap()
                .send(req().body(AsyncRequestBody::Empty).unwrap())
                .await
                .unwrap();
        },
    )
    .await
}

#[tokio::test]
async fn empty_body_has_no_transfer_encoding() {
    test(
        STOCK_CONFIG,
        1,
        |req| async move {
            assert_eq!(req.headers().get(CONTENT_LENGTH), None);
            assert_eq!(req.headers().get(TRANSFER_ENCODING), None);
            Ok(Response::new(http_body_util::Empty::new().boxed()))
        },
        |builder| async move {
            builder
                .build()
                .unwrap()
                .send(
                    req()
                        .method(Method::POST)
                        .body(AsyncRequestBody::Empty)
                        .unwrap(),
                )
                .await
                .unwrap();
        },
    )
    .await
}

#[tokio::test]
async fn fixed_body_has_content_length() {
    test(
        STOCK_CONFIG,
        1,
        |req| async move {
            assert_eq!(req.headers().get(CONTENT_LENGTH).unwrap(), "4");
            assert_eq!(req.headers().get(TRANSFER_ENCODING), None);
            Ok(Response::new(http_body_util::Empty::new().boxed()))
        },
        |builder| async move {
            builder
                .build()
                .unwrap()
                .send(
                    req()
                        .method(Method::POST)
                        .body(AsyncRequestBody::Fixed(Bytes::from_static(b"1234")))
                        .unwrap(),
                )
                .await
                .unwrap();
        },
    )
    .await
}

fn cert_file_pem() -> String {
    std::fs::read_to_string(cert_file()).unwrap()
}

fn alt_self_signed_cert_pem() -> String {
    use openssl::asn1::Asn1Time;
    use openssl::hash::MessageDigest;
    use openssl::pkey::PKey;
    use openssl::rsa::Rsa;
    use openssl::x509::{X509Builder, X509NameBuilder};

    let rsa = Rsa::generate(2048).unwrap();
    let pkey = PKey::from_rsa(rsa).unwrap();

    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "wrong.example.com")
        .unwrap();
    let name = name.build();

    let mut builder = X509Builder::new().unwrap();
    builder.set_version(2).unwrap();
    builder.set_subject_name(&name).unwrap();
    builder.set_issuer_name(&name).unwrap();
    builder.set_pubkey(&pkey).unwrap();
    builder
        .set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    builder
        .set_not_after(&Asn1Time::days_from_now(365).unwrap())
        .unwrap();
    builder.sign(&pkey, MessageDigest::sha256()).unwrap();
    let cert = builder.build();
    String::from_utf8(cert.to_pem().unwrap()).unwrap()
}

fn pinned_config_for(port: u16, pem: &str) -> ServiceConfig {
    ServiceConfig::builder()
        .uris(vec![format!("https://localhost:{port}").parse().unwrap()])
        .security(
            conjure_runtime_config::SecurityConfig::builder()
                .push_pinned_certs(pem)
                .build(),
        )
        .max_num_retries(0)
        .build()
}

async fn pinned_client<F>(port: u16, pem: &str, check: impl FnOnce(Builder) -> F)
where
    F: Future<Output = ()>,
{
    let builder = Client::builder()
        .service("service")
        .user_agent(UserAgent::new(Agent::new("test", "1.0")))
        .from_config(&pinned_config_for(port, pem));
    check(builder).await
}

#[tokio::test]
async fn pinned_certs_accepts_matching_leaf() {
    // Notably, this config does *not* set `ca-file`, so the server's self-signed certificate is
    // not in the trust store. The pin alone is what permits the connection.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let pem = cert_file_pem();

    let server = server(listener, 1, |_| async move {
        Ok(Response::new(Empty::new().boxed()))
    });

    let client = pinned_client(port, &pem, |builder| async move {
        let response = builder
            .build()
            .unwrap()
            .send(req().body(AsyncRequestBody::Empty).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    });

    join!(server, client);
}

#[tokio::test]
async fn pinned_certs_rejects_wrong_leaf() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let wrong_pem = alt_self_signed_cert_pem();

    let server = async move {
        // We expect the client to fail the TLS handshake. We still want a listener up so the
        // failure is the cert pin mismatch and not a connection-refused.
        let _ = listener.accept().await;
    };

    let client = pinned_client(port, &wrong_pem, |builder| async move {
        let err = builder
            .build()
            .unwrap()
            .send(req().body(AsyncRequestBody::Empty).unwrap())
            .await
            .err()
            .expect("expected pinned-leaf mismatch to fail the request");
        // The error should be a transport-layer error, not a remote (status code) error.
        assert!(err.cause().downcast_ref::<RemoteError>().is_none());
    });

    join!(server, client);
}

#[tokio::test]
async fn pinned_certs_rejects_correct_cert_with_wrong_key() {
    // Defense-in-depth: pinning the leaf is only safe if the server actually proves it holds the
    // matching private key. This test verifies that a server presenting the pinned cert but
    // signing the TLS handshake with an unrelated key is rejected, even though the pin check
    // (byte-comparison of the leaf) passes.
    use openssl::pkey::PKey;
    use openssl::rsa::Rsa;
    use rustls::crypto::ring::sign::any_supported_type;
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::server::{ClientHello, ResolvesServerCert, ServerConfig};
    use rustls::sign::CertifiedKey;
    use std::fmt;
    use std::sync::Arc;
    use tokio_rustls::TlsAcceptor;

    let pem = cert_file_pem();
    let cert_der = CertificateDer::from_pem_slice(pem.as_bytes()).unwrap();

    // Fresh, unrelated keypair.
    let rsa = Rsa::generate(2048).unwrap();
    let pkey = PKey::from_rsa(rsa).unwrap();
    let wrong_key_pem = pkey.private_key_to_pem_pkcs8().unwrap();
    let wrong_key_der = PrivatePkcs8KeyDer::from_pem_slice(&wrong_key_pem).unwrap();
    let signing_key = any_supported_type(&PrivateKeyDer::Pkcs8(wrong_key_der)).unwrap();

    let certified = Arc::new(CertifiedKey::new(vec![cert_der], signing_key));

    struct LyingResolver(Arc<CertifiedKey>);
    impl fmt::Debug for LyingResolver {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("LyingResolver").finish()
        }
    }
    impl ResolvesServerCert for LyingResolver {
        fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
            Some(self.0.clone())
        }
    }

    let server_config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(LyingResolver(certified)));
    let acceptor: TlsAcceptor = Arc::new(server_config).into();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = async move {
        if let Ok((tcp, _)) = listener.accept().await {
            // The handshake is expected to fail when rustls verifies the signature against the
            // (mismatched) public key in the presented leaf.
            let _ = acceptor.accept(tcp).await;
        }
    };

    let client = pinned_client(port, &pem, |builder| async move {
        let err = builder
            .build()
            .unwrap()
            .send(req().body(AsyncRequestBody::Empty).unwrap())
            .await
            .err()
            .expect("expected handshake signature failure");
        assert!(err.cause().downcast_ref::<RemoteError>().is_none());
    });

    join!(server, client);
}
