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
use bytes::buf::Writer;
use bytes::{BufMut, Bytes, BytesMut};
use flate2::write::GzDecoder;
use futures::ready;
use http::header::{Entry, ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH};
use http::{HeaderMap, HeaderValue, Request, Response};
use http_body::{Body, SizeHint};
use once_cell::sync::Lazy;
use pin_project::pin_project;
use std::error::Error;
use std::io::Write;
use std::pin::Pin;
use std::task::{Context, Poll};

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
    S: Service<Request<B1>, Response = Response<B2>> + Sync + Send,
    B1: Sync + Send,
    B2: Body<Data = Bytes>,
    B2::Error: Into<Box<dyn Error + Sync + Send>>,
{
    type Response = Response<DecodedBody<B2>>;
    type Error = S::Error;

    async fn call(&self, mut req: Request<B1>) -> Result<Self::Response, Self::Error> {
        if let Entry::Vacant(e) = req.headers_mut().entry(ACCEPT_ENCODING) {
            e.insert(GZIP.clone());
        }

        let response = self.inner.call(req).await?;
        let (mut parts, body) = response.into_parts();

        let decoder = match parts.headers.get(CONTENT_ENCODING) {
            Some(v) if v == *GZIP => {
                parts.headers.remove(CONTENT_ENCODING);
                parts.headers.remove(CONTENT_LENGTH);
                Some(GzDecoder::new(BytesMut::new().writer()))
            }
            _ => None,
        };

        let body = DecodedBody {
            body,
            decoder,
            done: false,
        };

        Ok(Response::from_parts(parts, body))
    }
}

#[pin_project]
pub struct DecodedBody<B> {
    #[pin]
    body: B,
    decoder: Option<GzDecoder<Writer<BytesMut>>>,
    done: bool,
}

impl<B> Body for DecodedBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn Error + Sync + Send>>,
{
    type Data = Bytes;

    type Error = Box<dyn Error + Sync + Send>;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let mut this = self.project();

        loop {
            if *this.done {
                return Poll::Ready(None);
            }

            let next = ready!(this.body.as_mut().poll_data(cx))
                .transpose()
                .map_err(Into::into)?;

            let Some(decoder) = this.decoder else {
                return Poll::Ready(next.map(Ok));
            };

            match next {
                Some(next) => {
                    decoder.write_all(&next)?;
                    if this.body.is_end_stream() {
                        decoder.try_finish()?;
                        *this.done = true;
                    } else {
                        decoder.flush()?;
                    }
                }
                None => {
                    decoder.try_finish()?;
                    *this.done = true;
                }
            }

            if !decoder.get_ref().get_ref().is_empty() {
                return Poll::Ready(Some(Ok(decoder.get_mut().get_mut().split().freeze())));
            }
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        self.project().body.poll_trailers(cx).map_err(Into::into)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        if self.decoder.is_some() {
            SizeHint::new()
        } else {
            self.body.size_hint()
        }
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
