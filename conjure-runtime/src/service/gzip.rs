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
use async_compression::stream::GzipDecoder;
use bytes::Bytes;
use futures::{ready, Stream};
use http::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH};
use http::{HeaderMap, HeaderValue, Request, Response};
use http_body::{Body, SizeHint};
use once_cell::sync::Lazy;
use pin_project::pin_project;
use std::error::Error;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::layer::Layer;
use tower::Service;

static GZIP: Lazy<HeaderValue> = Lazy::new(|| HeaderValue::from_static("gzip"));

/// A layer which transparently handles gzip content-encodings.
///
/// It will add an `Accept-Encoding: gzip` header to requests that do not already set `Accept-Encoding`, and decode the
/// response body if the server responds with `Content-Encoding: gzip`.
pub struct GzipLayer;

impl<S> Layer<S> for GzipLayer {
    type Service = GzipService<S>;

    fn layer(&self, inner: S) -> GzipService<S> {
        GzipService { inner }
    }
}

pub struct GzipService<S> {
    inner: S,
}

impl<S, B1, B2, E> Service<Request<B1>> for GzipService<S>
where
    S: Service<Request<B1>, Response = Response<B2>, Error = E>,
    B2: Body<Data = Bytes, Error = E> + 'static + Sync + Send,
    E: Into<Box<dyn Error + Sync + Send>>,
{
    type Response = Response<DecodedBody>;
    type Error = E;
    type Future = GzipFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B1>) -> Self::Future {
        if !req.headers().contains_key(ACCEPT_ENCODING) {
            req.headers_mut().insert(ACCEPT_ENCODING, GZIP.clone());
        }

        GzipFuture {
            future: self.inner.call(req),
        }
    }
}

#[pin_project]
pub struct GzipFuture<F> {
    #[pin]
    future: F,
}

impl<F, E, B> Future for GzipFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
    B: Body<Data = Bytes, Error = E> + 'static + Sync + Send,
    E: Into<Box<dyn Error + Sync + Send>>,
{
    type Output = Result<Response<DecodedBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let response = ready!(self.project().future.poll(cx))?;
        let (mut parts, body) = response.into_parts();

        let body = IdentityBody { body };

        let body: Pin<Box<dyn Body<Data = Bytes, Error = io::Error> + Sync + Send>> =
            match parts.headers.get(CONTENT_ENCODING) {
                Some(v) if v == *GZIP => {
                    parts.headers.remove(CONTENT_ENCODING);
                    parts.headers.remove(CONTENT_LENGTH);
                    Box::pin(GzipBody {
                        stream: GzipDecoder::new(ShimStream { body }),
                    })
                }
                _ => Box::pin(body),
            };
        let body = DecodedBody { body };

        Poll::Ready(Ok(Response::from_parts(parts, body)))
    }
}

pub struct DecodedBody {
    body: Pin<Box<dyn Body<Data = Bytes, Error = io::Error> + Sync + Send>>,
}

impl Body for DecodedBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        Pin::new(&mut self.body).poll_data(cx)
    }

    fn poll_trailers(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        Pin::new(&mut self.body).poll_trailers(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

impl Stream for DecodedBody {
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_data(cx)
    }
}

#[pin_project]
struct IdentityBody<T> {
    #[pin]
    body: T,
}

impl<T, E> Body for IdentityBody<T>
where
    T: Body<Data = Bytes, Error = E>,
    E: Into<Box<dyn Error + Sync + Send>>,
{
    type Data = Bytes;
    type Error = io::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let item = ready!(self.project().body.poll_data(cx));
        Poll::Ready(item.map(|r| r.map_err(|e| io::Error::new(io::ErrorKind::Other, e))))
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        self.project()
            .body
            .poll_trailers(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        Body::size_hint(&self.body)
    }
}

#[pin_project]
struct ShimStream<T> {
    #[pin]
    body: IdentityBody<T>,
}

impl<T, E> Stream for ShimStream<T>
where
    T: Body<Data = Bytes, Error = E>,
    E: Into<Box<dyn Error + Sync + Send>>,
{
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().body.poll_data(cx)
    }
}

#[pin_project]
struct GzipBody<T> {
    #[pin]
    stream: GzipDecoder<ShimStream<T>>,
}

// NB: We can't override is_end_stream since we may get an error out of the gzip decoder at eof.
impl<T, E> Body for GzipBody<T>
where
    T: Body<Data = Bytes, Error = E>,
    E: Into<Box<dyn Error + Sync + Send>>,
{
    type Data = Bytes;
    type Error = io::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        self.project().stream.poll_next(cx)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        self.project()
            .stream
            .get_pin_mut()
            .project()
            .body
            .poll_trailers(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use hyper::body;
    use std::io::Write;
    use tower::ServiceExt;

    #[tokio::test]
    async fn uncompressed() {
        let body = "hello world";

        let service = GzipLayer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(req.headers().get(ACCEPT_ENCODING).unwrap(), "gzip");

            let response = Response::builder()
                .header(CONTENT_LENGTH, body.len().to_string())
                .body(hyper::Body::from(body))
                .unwrap();
            Ok::<_, hyper::Error>(response)
        }));

        let response = service.oneshot(Request::new(())).await.unwrap();

        assert_eq!(
            response.headers().get(CONTENT_LENGTH).unwrap(),
            &*body.len().to_string(),
        );
        assert_eq!(response.headers().get(CONTENT_ENCODING), None);

        let actual = body::to_bytes(response.into_body()).await.unwrap();
        assert_eq!(actual, body.as_bytes());
    }

    #[tokio::test]
    async fn compressed() {
        let body = "hello world";

        let service = GzipLayer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(req.headers().get(ACCEPT_ENCODING).unwrap(), "gzip");

            let mut writer = GzEncoder::new(vec![], Compression::default());
            writer.write_all(body.as_bytes()).unwrap();
            let body = writer.finish().unwrap();

            let response = Response::builder()
                .header(CONTENT_LENGTH, body.len().to_string())
                .header(CONTENT_ENCODING, "gzip")
                .body(hyper::Body::from(body))
                .unwrap();
            Ok::<_, hyper::Error>(response)
        }));

        let response = service.oneshot(Request::new(())).await.unwrap();

        assert_eq!(response.headers().get(CONTENT_LENGTH), None);
        assert_eq!(response.headers().get(CONTENT_ENCODING), None);

        let actual = body::to_bytes(response.into_body()).await.unwrap();
        assert_eq!(actual, body.as_bytes());
    }

    #[tokio::test]
    async fn custom_accept_encoding() {
        let body = "hello world";
        let encoding = "br";

        let service = GzipLayer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(req.headers().get(ACCEPT_ENCODING).unwrap(), encoding);

            let response = Response::builder()
                .header(CONTENT_LENGTH, body.len().to_string())
                .header(CONTENT_ENCODING, encoding)
                .body(hyper::Body::from(body))
                .unwrap();
            Ok::<_, hyper::Error>(response)
        }));

        let request = Request::builder()
            .header(ACCEPT_ENCODING, encoding)
            .body(())
            .unwrap();
        let response = service.oneshot(request).await.unwrap();

        assert_eq!(
            response.headers().get(CONTENT_LENGTH).unwrap(),
            &*body.len().to_string(),
        );
        assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), encoding);

        let actual = body::to_bytes(response.into_body()).await.unwrap();
        assert_eq!(actual, body.as_bytes());
    }
}
