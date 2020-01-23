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
use crate::errors::{RemoteError, TimeoutError};
use crate::{blocking, Agent, Body, BodyWriter, Client, HostMetricsRegistry, UserAgent};
use async_trait::async_trait;
use bytes::Bytes;
use conjure_error::NotFound;
use conjure_error::{Error, ErrorKind};
use conjure_runtime_config::{ServiceConfig, ServicesConfig};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use futures::task::Context;
use futures::{join, StreamExt, TryStreamExt};
use hyper::header::{HeaderValue, ACCEPT_ENCODING, CONTENT_ENCODING, HOST};
use hyper::http::header::RETRY_AFTER;
use hyper::server::conn::Http;
use hyper::service::Service;
use hyper::{Request, Response, StatusCode};
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::thread;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Runtime;
use tokio::time;
use witchcraft_metrics::MetricRegistry;
use zipkin::{SpanId, TraceContext, TraceId};

const STOCK_CONFIG: &str = r#"
services:
  service:
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
        .set_private_key_file(&key_file(), SslFiletype::PEM)
        .unwrap();
    acceptor.set_certificate_chain_file(&cert_file()).unwrap();
    acceptor.build()
}

async fn test<F, G>(
    config: &str,
    requests: u32,
    handler: impl FnMut(Request<hyper::Body>) -> F + 'static,
    check: impl FnOnce(Client) -> G,
) where
    F: Future<Output = Result<Response<hyper::Body>, &'static str>> + 'static + Send,
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
    handler: impl FnMut(Request<hyper::Body>) -> F + 'static + Send,
    check: impl FnOnce(blocking::Client),
) where
    F: Future<Output = Result<Response<hyper::Body>, &'static str>> + 'static + Send,
{
    let mut runtime = Runtime::new().unwrap();
    let listener = runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || runtime.block_on(server(listener, requests, handler)));

    let config = parse_config(config, port);
    let agent = UserAgent::new(Agent::new("test", "1.0"));
    let node_health = Arc::new(HostMetricsRegistry::new());
    let metrics = Arc::new(MetricRegistry::new());
    let client = blocking::Client::new("service", agent, &node_health, &metrics, &config).unwrap();

    check(client);
    server.join().unwrap();
}

async fn server<F>(
    mut listener: TcpListener,
    requests: u32,
    mut handler: impl FnMut(Request<hyper::Body>) -> F + 'static,
) where
    F: Future<Output = Result<Response<hyper::Body>, &'static str>> + 'static + Send,
{
    let acceptor = ssl_acceptor();

    for _ in 0..requests {
        let socket = listener.accept().await.unwrap().0;
        let socket = tokio_openssl::accept(&acceptor, socket).await.unwrap();
        let _ = Http::new()
            .keep_alive(false)
            .serve_connection(socket, TestService(&mut handler))
            .await;
    }
}

async fn client<F>(config: &str, port: u16, check: impl FnOnce(Client) -> F)
where
    F: Future<Output = ()>,
{
    let config = parse_config(config, port);
    let agent = UserAgent::new(Agent::new("test", "1.0"));
    let node_health = Arc::new(HostMetricsRegistry::new());
    let metrics = Arc::new(MetricRegistry::new());
    let client = Client::new("service", agent, &node_health, &metrics, &config).unwrap();

    check(client).await
}

fn parse_config(config: &str, port: u16) -> ServiceConfig {
    let config = config
        .replace("{{port}}", &port.to_string())
        .replace("{{ca_file}}", &cert_file().display().to_string());
    let config = serde_yaml::from_str::<ServicesConfig>(&config).unwrap();
    config.service("service").unwrap().clone()
}

struct TestService<'a, F>(&'a mut F);

impl<'a, F, G> Service<Request<hyper::Body>> for TestService<'a, F>
where
    F: FnMut(Request<hyper::Body>) -> G,
    G: Future<Output = Result<Response<hyper::Body>, &'static str>> + 'static + Send,
{
    type Response = Response<hyper::Body>;
    type Error = &'static str;
    type Future = G;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<hyper::Body>) -> Self::Future {
        (self.0)(req)
    }
}

#[tokio::test]
async fn mesh_proxy() {
    test(
        r#"
services:
  service:
    uris: ["https://www.google.com:1234"]
    security:
      ca-file: "{{ca_file}}"
    proxy:
      type: mesh
      host-and-port: "localhost:{{port}}"
    "#,
        1,
        |req| {
            async move {
                let host = req.headers().get(HOST).unwrap();
                assert_eq!(host, "www.google.com:1234");
                assert_eq!(req.uri(), &"/foo/bar?fizz=buzz");

                Ok(Response::new(hyper::Body::empty()))
            }
        },
        |client| {
            async move {
                client
                    .get("/foo/bar")
                    .param("fizz", "buzz")
                    .send()
                    .await
                    .unwrap();
            }
        },
    )
    .await;
}

#[tokio::test]
async fn retry_after_503() {
    let mut first = true;
    test(
        STOCK_CONFIG,
        2,
        move |_| {
            let inner_first = first;
            first = false;
            async move {
                if inner_first {
                    Ok(Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .body(hyper::Body::empty())
                        .unwrap())
                } else {
                    Ok(Response::new(hyper::Body::empty()))
                }
            }
        },
        |client| {
            async move {
                let response = client.get("/").send().await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }
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
        |_| {
            async move {
                Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(hyper::Body::empty())
                    .unwrap())
            }
        },
        |client| {
            async move {
                let error = client.get("/").send().await.err().unwrap();
                assert_eq!(
                    error
                        .cause()
                        .downcast_ref::<RemoteError>()
                        .unwrap()
                        .status(),
                    &StatusCode::NOT_FOUND,
                );
            }
        },
    )
    .await;
}

#[tokio::test]
async fn retry_after_overrides() {
    let mut first = true;
    test(
        STOCK_CONFIG,
        2,
        move |_| {
            let inner_first = first;
            first = false;
            async move {
                if inner_first {
                    Ok(Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .header(RETRY_AFTER, "1")
                        .body(hyper::Body::empty())
                        .unwrap())
                } else {
                    Ok(Response::new(hyper::Body::empty()))
                }
            }
        },
        |client| {
            async move {
                let time = Instant::now();
                let response = client.get("/").send().await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                assert!(time.elapsed() >= Duration::from_secs(1));
            }
        },
    )
    .await;
}

#[tokio::test]
async fn connect_error_doesnt_reset_body() {
    struct TestBody(bool);

    #[async_trait]
    impl Body for TestBody {
        fn content_length(&self) -> Option<u64> {
            None
        }

        fn content_type(&self) -> HeaderValue {
            HeaderValue::from_static("text/plain")
        }

        async fn write(mut self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
            assert!(!self.0);
            self.0 = true;
            w.write_all(b"hello world").await.unwrap();
            Ok(())
        }

        async fn reset(self: Pin<&mut Self>) -> bool {
            panic!("should not reset");
        }
    }

    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = async move {
        // accept and immediately close the socket without completing the TLS handshake
        listener.accept().await.unwrap();

        server(listener, 1, |req| {
            async move {
                let body = req
                    .into_body()
                    .try_fold(vec![], |mut buf, chunk| {
                        async move {
                            buf.extend_from_slice(&chunk);
                            Ok(buf)
                        }
                    })
                    .await
                    .unwrap();
                assert_eq!(&*body, b"hello world");
                Ok(Response::new(hyper::Body::empty()))
            }
        })
        .await;
    };

    let client = client(STOCK_CONFIG, port, |client| {
        async move {
            let response = client.put("/").body(TestBody(false)).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    });

    join!(server, client);
}

#[tokio::test]
async fn propagate_429() {
    test(
        STOCK_CONFIG,
        1,
        |_| {
            async {
                Ok(Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .body(hyper::Body::empty())
                    .unwrap())
            }
        },
        |mut client| {
            async move {
                client.set_propagate_qos_errors(true);
                let error = client.get("/").send().await.err().unwrap();
                match error.kind() {
                    ErrorKind::Throttle(e) => assert_eq!(e.duration(), None),
                    _ => panic!("wrong error kind"),
                }
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
        |_| {
            async {
                Ok(Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .header(RETRY_AFTER, "100")
                    .body(hyper::Body::empty())
                    .unwrap())
            }
        },
        |mut client| {
            async move {
                client.set_propagate_qos_errors(true);
                let error = client.get("/").send().await.err().unwrap();
                match error.kind() {
                    ErrorKind::Throttle(e) => {
                        assert_eq!(e.duration(), Some(Duration::from_secs(100)))
                    }
                    _ => panic!("wrong error kind"),
                }
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
        |_| {
            async {
                Ok(Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .body(hyper::Body::empty())
                    .unwrap())
            }
        },
        |mut client| {
            async move {
                client.set_propagate_qos_errors(true);
                let error = client.get("/").send().await.err().unwrap();
                match error.kind() {
                    ErrorKind::Unavailable(_) => {}
                    _ => panic!("wrong error kind"),
                }
            }
        },
    )
    .await;
}

#[tokio::test]
async fn dont_propagate_protocol_errors() {
    let mut first = true;
    test(
        STOCK_CONFIG,
        2,
        move |_| {
            let inner_first = first;
            first = false;
            async move {
                if inner_first {
                    Err("")
                } else {
                    Ok(Response::new(hyper::Body::empty()))
                }
            }
        },
        |mut client| {
            async move {
                client.set_propagate_qos_errors(true);
                let response = client.get("/").send().await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }
        },
    )
    .await;
}

#[tokio::test]
async fn dont_bail_when_all_timed_out() {
    let mut first = true;
    test(
        r#"
services:
  service:
    uris: ["https://localhost:{{port}}"]
    security:
      ca-file: "{{ca_file}}"
    failed-url-cooldown: 1h
    "#,
        2,
        move |_| {
            let inner_first = first;
            first = false;
            async move {
                if inner_first {
                    Err("")
                } else {
                    Ok(Response::new(hyper::Body::empty()))
                }
            }
        },
        |client| {
            async move {
                let response = client.get("/").send().await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }
        },
    )
    .await;
}

#[tokio::test]
async fn slow_headers() {
    test(
        r#"
services:
  service:
    uris: ["https://localhost:{{port}}"]
    security:
      ca-file: "{{ca_file}}"
    request-timeout: 1s
    "#,
        1,
        |_| {
            async {
                time::delay_for(Duration::from_secs(2)).await;
                Ok(Response::new(hyper::Body::empty()))
            }
        },
        |client| {
            async move {
                let start = Instant::now();
                let error = client.get("/").send().await.err().unwrap();
                assert!(start.elapsed() < Duration::from_secs(2));
                assert!(error.cause().is::<TimeoutError>());
            }
        },
    )
    .await;
}

#[tokio::test]
async fn slow_request_body() {
    test(
        r#"
services:
  service:
    uris: ["https://localhost:{{port}}"]
    security:
      ca-file: "{{ca_file}}"
    request-timeout: 1s
    "#,
        1,
        |req| {
            async {
                req.into_body().for_each(|_| async {}).await;
                Ok(Response::new(hyper::Body::empty()))
            }
        },
        |client| {
            async move {
                let start = Instant::now();
                let error = client
                    .post("/")
                    .body(InfiniteBody)
                    .send()
                    .await
                    .err()
                    .unwrap();
                assert!(start.elapsed() < Duration::from_secs(2));
                assert!(error.cause().is::<TimeoutError>());
            }
        },
    )
    .await;
}

#[tokio::test]
async fn slow_response_body() {
    test(
        r#"
services:
  service:
    uris: ["https://localhost:{{port}}"]
    security:
      ca-file: "{{ca_file}}"
    request-timeout: 1s
    "#,
        1,
        |_| {
            async {
                let (mut sender, body) = hyper::Body::channel();
                tokio::spawn(async move {
                    time::delay_for(Duration::from_secs(2)).await;
                    let _ = sender.send_data(Bytes::from("hi")).await;
                });
                Ok(Response::new(body))
            }
        },
        |client| {
            async move {
                let start = Instant::now();
                let response = client.get("/").send().await.unwrap();
                let error = response
                    .into_body()
                    .read_to_end(&mut vec![])
                    .await
                    .err()
                    .unwrap();
                assert!(start.elapsed() < Duration::from_secs(2));
                assert!(error.get_ref().unwrap().is::<TimeoutError>());
            }
        },
    )
    .await;
}

#[tokio::test]
async fn body_write_ends_after_error() {
    test(
        STOCK_CONFIG,
        1,
        |_| async { Ok(Response::new(hyper::Body::empty())) },
        |client| {
            async move {
                client.post("/").body(InfiniteBody).send().await.unwrap();
            }
        },
    )
    .await;
}

#[tokio::test]
async fn streaming_write_error_reporting() {
    struct TestBody;

    #[async_trait]
    impl Body for TestBody {
        fn content_length(&self) -> Option<u64> {
            None
        }

        fn content_type(&self) -> HeaderValue {
            HeaderValue::from_static("test/plain")
        }

        async fn write(self: Pin<&mut Self>, _: Pin<&mut BodyWriter>) -> Result<(), Error> {
            Err(Error::internal_safe("foobar"))
        }

        async fn reset(self: Pin<&mut Self>) -> bool {
            panic!("should not reset")
        }
    }

    test(
        STOCK_CONFIG,
        1,
        |req| {
            async move {
                req.into_body().for_each(|_| async {}).await;
                Ok(Response::new(hyper::Body::empty()))
            }
        },
        |client| {
            async move {
                let error = client.post("/").body(TestBody).send().await.err().unwrap();
                assert_eq!(error.cause().to_string(), "foobar");
            }
        },
    )
    .await;
}

#[tokio::test]
async fn service_error_propagation() {
    test(
        STOCK_CONFIG,
        1,
        |_| {
            async {
                let body = conjure_error::encode(&NotFound::new());
                let body = conjure_serde::json::to_vec(&body).unwrap();
                Ok(Response::builder()
                    .status(404)
                    .header("Content-Type", "application/json")
                    .body(hyper::Body::from(body))
                    .unwrap())
            }
        },
        |mut client| {
            async move {
                client.set_propagate_service_errors(true);
                let error = client.get("/").send().await.err().unwrap();
                match error.kind() {
                    ErrorKind::Service(e) => assert_eq!(e.error_name(), "Default:NotFound"),
                    _ => panic!("invalid error kind"),
                }
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
        |req| {
            async move {
                assert_eq!(req.headers().get(ACCEPT_ENCODING).unwrap(), "gzip, deflate");
                let mut body = GzEncoder::new(vec![], Compression::default());
                body.write_all(b"hello world").unwrap();
                let body = body.finish().unwrap();
                Ok(Response::builder()
                    .header(CONTENT_ENCODING, "gzip")
                    .body(hyper::Body::from(body))
                    .unwrap())
            }
        },
        |client| {
            async move {
                let response = client.get("/").send().await.unwrap();
                let mut body = vec![];
                response.into_body().read_to_end(&mut body).await.unwrap();
                assert_eq!(body, b"hello world");
            }
        },
    )
    .await;
}

#[tokio::test]
async fn deflate_body() {
    test(
        STOCK_CONFIG,
        1,
        |req| {
            async move {
                assert_eq!(req.headers().get(ACCEPT_ENCODING).unwrap(), "gzip, deflate");
                let mut body = ZlibEncoder::new(vec![], Compression::default());
                body.write_all(b"hello world").unwrap();
                let body = body.finish().unwrap();
                Ok(Response::builder()
                    .header(CONTENT_ENCODING, "deflate")
                    .body(hyper::Body::from(body))
                    .unwrap())
            }
        },
        |client| {
            async move {
                let response = client.get("/").send().await.unwrap();
                let mut body = vec![];
                response.into_body().read_to_end(&mut body).await.unwrap();
                assert_eq!(body, b"hello world");
            }
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
        |req| {
            async move {
                assert_eq!(
                    req.headers().get("X-B3-TraceId").unwrap(),
                    "0001020304050607"
                );
                Ok(Response::new(hyper::Body::empty()))
            }
        },
        |client| {
            let context = TraceContext::builder()
                .trace_id(trace_id)
                .span_id(SpanId::from(*b"abcdefgh"))
                .build();
            zipkin::join_trace(context).detach().bind(async move {
                client.get("/").send().await.unwrap();
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
        |req| {
            async move {
                assert_eq!(
                    req.headers().get("X-B3-TraceId").unwrap(),
                    "0001020304050607"
                );
                Ok(Response::new(hyper::Body::empty()))
            }
        },
        |client| {
            let context = TraceContext::builder()
                .trace_id(trace_id)
                .span_id(SpanId::from(*b"abcdefgh"))
                .build();
            let _guard = zipkin::set_current(context);
            client.get("/").send().unwrap();
        },
    );
}

struct InfiniteBody;

#[async_trait]
impl Body for InfiniteBody {
    fn content_length(&self) -> Option<u64> {
        None
    }

    fn content_type(&self) -> HeaderValue {
        HeaderValue::from_static("test/plain")
    }

    async fn write(self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
        let buf = [b'a'; 1024];
        loop {
            w.write_all(&buf).await.map_err(Error::internal_safe)?;
        }
    }

    async fn reset(self: Pin<&mut Self>) -> bool {
        panic!("should not reset");
    }
}
