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
use http::header::{Entry, ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH};
use http::{HeaderValue, Request, Response};
use http_body::{Body, Frame, SizeHint};
use once_cell::sync::Lazy;
use pin_project::pin_project;
use std::error::Error;
use std::io::Write;
use std::mem;
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

        let state = match parts.headers.get(CONTENT_ENCODING) {
            Some(v) if v == *GZIP => {
                parts.headers.remove(CONTENT_ENCODING);
                parts.headers.remove(CONTENT_LENGTH);
                State::Decompressing(GzDecoder::new(BytesMut::new().writer()))
            }
            _ => State::Done,
        };

        let body = DecodedBody { body, state };

        Ok(Response::from_parts(parts, body))
    }
}

#[allow(clippy::large_enum_variant)]
enum State {
    Decompressing(GzDecoder<Writer<BytesMut>>),
    Last(Frame<Bytes>),
    Done,
}

#[pin_project]
pub struct DecodedBody<B> {
    #[pin]
    body: B,
    state: State,
}

impl<B> Body for DecodedBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn Error + Sync + Send>>,
{
    type Data = Bytes;

    type Error = Box<dyn Error + Sync + Send>;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();

        loop {
            match mem::replace(this.state, State::Done) {
                State::Decompressing(mut decoder) => match this.body.as_mut().poll_frame(cx) {
                    Poll::Ready(Some(Ok(frame))) => match frame.data_ref() {
                        Some(data) => {
                            decoder.write_all(data)?;
                            if this.body.is_end_stream() {
                                decoder.try_finish()?;
                                if decoder.get_ref().get_ref().is_empty() {
                                    return Poll::Ready(None);
                                } else {
                                    let buf = decoder.get_mut().get_mut().split().freeze();
                                    return Poll::Ready(Some(Ok(Frame::data(buf))));
                                }
                            } else {
                                decoder.flush()?;
                                let buf = decoder.get_mut().get_mut().split().freeze();
                                *this.state = State::Decompressing(decoder);

                                if !buf.is_empty() {
                                    return Poll::Ready(Some(Ok(Frame::data(buf))));
                                }
                            }
                        }
                        None => {
                            decoder.try_finish()?;
                            if decoder.get_ref().get_ref().is_empty() {
                                return Poll::Ready(Some(Ok(frame)));
                            } else {
                                *this.state = State::Last(frame);
                                let buf = decoder.get_mut().get_mut().split().freeze();
                                return Poll::Ready(Some(Ok(Frame::data(buf))));
                            }
                        }
                    },
                    Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e.into()))),
                    Poll::Ready(None) => {
                        decoder.try_finish()?;
                        if decoder.get_ref().get_ref().is_empty() {
                            return Poll::Ready(None);
                        } else {
                            let buf = decoder.get_mut().get_mut().split().freeze();
                            return Poll::Ready(Some(Ok(Frame::data(buf))));
                        }
                    }
                    Poll::Pending => {
                        *this.state = State::Decompressing(decoder);
                        return Poll::Pending;
                    }
                },
                State::Last(frame) => return Poll::Ready(Some(Ok(frame))),
                State::Done => return this.body.as_mut().poll_frame(cx).map_err(Into::into),
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        match self.state {
            State::Decompressing(_) | State::Done => self.body.is_end_stream(),
            State::Last(_) => false,
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.state {
            State::Decompressing(_) => SizeHint::new(),
            State::Last(frame) => match frame.data_ref() {
                Some(data) => SizeHint::with_exact(data.len() as u64),
                None => SizeHint::with_exact(0),
            },
            State::Done => self.body.size_hint(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use http_body_util::{BodyExt, Full};
    use std::io::Write;

    #[tokio::test]
    async fn uncompressed() {
        let body = "hello world";

        let service = GzipLayer.layer(service::service_fn(|req: Request<()>| async move {
            assert_eq!(req.headers().get(ACCEPT_ENCODING).unwrap(), "gzip");

            let response = Response::builder()
                .header(CONTENT_LENGTH, body.len().to_string())
                .body(Full::new(Bytes::from(body)))
                .unwrap();
            Ok::<_, hyper::Error>(response)
        }));

        let response = service.call(Request::new(())).await.unwrap();

        assert_eq!(
            response.headers().get(CONTENT_LENGTH).unwrap(),
            &*body.len().to_string(),
        );
        assert_eq!(response.headers().get(CONTENT_ENCODING), None);

        let actual = response.into_body().collect().await.unwrap();
        assert_eq!(actual.to_bytes(), body.as_bytes());
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
                .body(Full::new(Bytes::from(body)))
                .unwrap();
            Ok::<_, hyper::Error>(response)
        }));

        let response = service.call(Request::new(())).await.unwrap();

        assert_eq!(response.headers().get(CONTENT_LENGTH), None);
        assert_eq!(response.headers().get(CONTENT_ENCODING), None);

        let actual = response.into_body().collect().await.unwrap();
        assert_eq!(actual.to_bytes(), body.as_bytes());
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
                .body(Full::new(Bytes::from(body)))
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

        let actual = response.into_body().collect().await.unwrap();
        assert_eq!(actual.to_bytes(), body.as_bytes());
    }
}
