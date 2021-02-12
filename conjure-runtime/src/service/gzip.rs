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
use crate::raw::Service;
use crate::service::Layer;
use async_compression::tokio::bufread::GzipDecoder;
use bytes::{Buf, Bytes, BytesMut};
use futures::{ready, Stream};
use http::header::{Entry, ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH};
use http::{HeaderMap, HeaderValue, Request, Response};
use http_body::{Body, SizeHint};
use once_cell::sync::Lazy;
use pin_project::pin_project;
use std::error::Error;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncRead, ReadBuf};
use tokio_util::codec::{BytesCodec, FramedRead};

static GZIP: Lazy<HeaderValue> = Lazy::new(|| HeaderValue::from_static("gzip"));

/// A layer which transparently handles gzip content-encodings.
///
/// It will add an `Accept-Encoding: gzip` header to requests that do not already set `Accept-Encoding`, and decode the
/// response body if the server responds with `Content-Encoding: gzip`.
pub struct GzipLayer;

impl<S> Layer<S> for GzipLayer {
    type Service = GzipService<S>;

    fn layer(self, inner: S) -> GzipService<S> {
        GzipService { inner }
    }
}

pub struct GzipService<S> {
    inner: S,
}

impl<S, B1, B2> Service<Request<B1>> for GzipService<S>
where
    S: Service<Request<B1>, Response = Response<B2>>,
    B2: Body<Data = Bytes>,
    B2::Error: Into<Box<dyn Error + Sync + Send>>,
{
    type Response = Response<DecodedBody<B2>>;
    type Error = S::Error;
    type Future = GzipFuture<S::Future>;

    fn call(&self, mut req: Request<B1>) -> Self::Future {
        if let Entry::Vacant(e) = req.headers_mut().entry(ACCEPT_ENCODING) {
            e.insert(GZIP.clone());
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
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn Error + Sync + Send>>,
{
    type Output = Result<Response<DecodedBody<B>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let response = ready!(self.project().future.poll(cx))?;
        let (mut parts, body) = response.into_parts();

        let body = match parts.headers.get(CONTENT_ENCODING) {
            Some(v) if v == *GZIP => {
                parts.headers.remove(CONTENT_ENCODING);
                parts.headers.remove(CONTENT_LENGTH);
                DecodedBody::Gzip {
                    body: FramedRead::new(
                        GzipDecoder::new(ShimReader {
                            body,
                            buf: Bytes::new(),
                        }),
                        BytesCodec::new(),
                    ),
                }
            }
            _ => DecodedBody::Identity { body },
        };

        Poll::Ready(Ok(Response::from_parts(parts, body)))
    }
}

#[pin_project(project = Projection)]
pub enum DecodedBody<B> {
    Identity {
        #[pin]
        body: B,
    },
    Gzip {
        #[pin]
        body: FramedRead<GzipDecoder<ShimReader<B>>, BytesCodec>,
    },
}

impl<B> Body for DecodedBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn Error + Sync + Send>>,
{
    type Data = Bytes;
    type Error = io::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.project() {
            Projection::Identity { body } => body
                .poll_data(cx)
                .map(|o| o.map(|r| r.map_err(|e| io::Error::new(io::ErrorKind::Other, e)))),
            Projection::Gzip { body } => body
                .poll_next(cx)
                .map(|o| o.map(|r| r.map(BytesMut::freeze))),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        match self.project() {
            Projection::Identity { body } => body
                .poll_trailers(cx)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e)),
            Projection::Gzip { body, .. } => body
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e)),
        }
    }

    fn is_end_stream(&self) -> bool {
        match self {
            DecodedBody::Identity { body } => body.is_end_stream(),
            // we can't check the inner body since we may get an error out of the decoder at eof
            DecodedBody::Gzip { .. } => false,
        }
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            DecodedBody::Identity { body } => body.size_hint(),
            DecodedBody::Gzip { .. } => SizeHint::new(),
        }
    }
}

#[pin_project]
pub struct ShimReader<T> {
    #[pin]
    body: T,
    buf: Bytes,
}

impl<T> AsyncRead for ShimReader<T>
where
    T: Body<Data = Bytes>,
    T::Error: Into<Box<dyn Error + Sync + Send>>,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let in_buf = ready!(self.as_mut().poll_fill_buf(cx))?;
        let len = usize::min(in_buf.len(), buf.remaining());
        buf.put_slice(&in_buf[..len]);
        self.consume(len);

        Poll::Ready(Ok(()))
    }
}

impl<T> AsyncBufRead for ShimReader<T>
where
    T: Body<Data = Bytes>,
    T::Error: Into<Box<dyn Error + Sync + Send>>,
{
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let mut this = self.project();

        while !this.buf.has_remaining() {
            *this.buf = match ready!(this.body.as_mut().poll_data(cx)) {
                Some(Ok(buf)) => buf,
                Some(Err(e)) => return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
                None => break,
            }
        }

        Poll::Ready(Ok(&*this.buf))
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.project().buf.advance(amt);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use hyper::body;
    use std::io::Write;

    #[tokio::test]
    async fn uncompressed() {
        let body = "hello world";

        let service = GzipLayer.layer(service::service_fn(|req: Request<()>| async move {
            assert_eq!(req.headers().get(ACCEPT_ENCODING).unwrap(), "gzip");

            let response = Response::builder()
                .header(CONTENT_LENGTH, body.len().to_string())
                .body(hyper::Body::from(body))
                .unwrap();
            Ok::<_, hyper::Error>(response)
        }));

        let response = service.call(Request::new(())).await.unwrap();

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

        let service = GzipLayer.layer(service::service_fn(|req: Request<()>| async move {
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

        let response = service.call(Request::new(())).await.unwrap();

        assert_eq!(response.headers().get(CONTENT_LENGTH), None);
        assert_eq!(response.headers().get(CONTENT_ENCODING), None);

        let actual = body::to_bytes(response.into_body()).await.unwrap();
        assert_eq!(actual, body.as_bytes());
    }

    #[tokio::test]
    async fn custom_accept_encoding() {
        let body = "hello world";
        let encoding = "br";

        let service = GzipLayer.layer(service::service_fn(|req: Request<()>| async move {
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
        let response = service.call(request).await.unwrap();

        assert_eq!(
            response.headers().get(CONTENT_LENGTH).unwrap(),
            &*body.len().to_string(),
        );
        assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), encoding);

        let actual = body::to_bytes(response.into_body()).await.unwrap();
        assert_eq!(actual, body.as_bytes());
    }
}
