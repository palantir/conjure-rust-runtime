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
use crate::payload::{BodyError, HyperBody};
use crate::Body;
use conjure_error::Error;
use futures::future::{self, BoxFuture};
use http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use http::{HeaderValue, Request};
use std::error;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::layer::Layer;
use tower::Service;
use witchcraft_log::info;

/// A layer which converts a request with a body implementing the Conjure `Body` trait into a request with a body
/// implementing the `http_body::Body` trait.
///
/// It additionally sets the `Content-Length` and `Content-Type` headers based off of the `Body` implementation.
pub(crate) struct BodyWriteLayer;

impl<S> Layer<S> for BodyWriteLayer {
    type Service = BodyWriteService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        BodyWriteService { inner }
    }
}

pub(crate) struct BodyWriteService<S> {
    inner: S,
}

impl<'a, S, B> Service<Request<Option<Pin<&'a mut B>>>> for BodyWriteService<S>
where
    S: Service<Request<HyperBody>, Error = Error>,
    S::Response: Send,
    S::Future: Send + 'a,
    B: ?Sized + Body + Send,
{
    type Response = S::Response;
    type Error = Error;
    type Future = BoxFuture<'a, Result<S::Response, Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Option<Pin<&'a mut B>>>) -> Self::Future {
        let (mut parts, body) = req.into_parts();
        if let Some(body) = &body {
            if let Some(length) = body.content_length() {
                parts
                    .headers
                    .insert(CONTENT_LENGTH, HeaderValue::from(length));
            }
            parts.headers.insert(CONTENT_TYPE, body.content_type());
        }
        let (body, writer) = HyperBody::new(body);
        let req = Request::from_parts(parts, body);
        let future = self.inner.call(req);

        Box::pin(async move {
            let (body_result, response_result) = future::join(writer.write(), future).await;

            match (body_result, response_result) {
                (Ok(()), Ok(response)) => Ok(response),
                (Ok(()), Err(e)) => Err(e),
                (Err(e), Ok(response)) => {
                    info!(
                        "body write reported an error on a successful request",
                        error: e,
                    );
                    Ok(response)
                }
                (Err(body), Err(hyper)) => Err(deconflict_errors(body, hyper)),
            }
        })
    }
}

// An error in the body write will cause an error on the hyper side, and vice versa.
// To pick the right one, we see if the hyper error was due to the body write aborting or not.
fn deconflict_errors(body_error: Error, hyper_error: Error) -> Error {
    let mut cause: &(dyn error::Error + 'static) = hyper_error.cause();
    loop {
        if cause.is::<BodyError>() {
            return body_error;
        }
        cause = match cause.source() {
            Some(cause) => cause,
            None => return hyper_error,
        };
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::body::BytesBody;
    use crate::BodyWriter;
    use async_trait::async_trait;
    use http_body::Body as _;
    use tokio::io::AsyncWriteExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn no_body() {
        let service = BodyWriteLayer.layer(tower::service_fn(|req: Request<_>| async move {
            assert_eq!(req.headers().get(CONTENT_LENGTH), None);
            assert_eq!(req.headers().get(CONTENT_TYPE), None);

            match req.body() {
                HyperBody::Empty => {}
                _ => panic!("expected empty body"),
            }

            Ok(())
        }));

        let request = Request::new(None::<Pin<&mut (dyn Body + Send)>>);
        service.oneshot(request).await.unwrap();
    }

    #[tokio::test]
    async fn fixed_size_body() {
        let body = "hello world";

        let service = BodyWriteLayer.layer(tower::service_fn(|req: Request<_>| async move {
            assert_eq!(
                req.headers().get(CONTENT_LENGTH).unwrap(),
                &*body.len().to_string(),
            );
            assert_eq!(req.headers().get(CONTENT_TYPE).unwrap(), "text/plain");

            match req.body() {
                HyperBody::Single(chunk) => assert_eq!(chunk, body),
                _ => panic!("expected single chunk body"),
            }

            Ok(())
        }));

        let mut body = BytesBody::new(body, HeaderValue::from_static("text/plain"));
        let request = Request::new(Some(Pin::new(&mut body)));
        service.oneshot(request).await.unwrap();
    }

    struct StreamedBody;

    #[async_trait]
    impl Body for StreamedBody {
        fn content_length(&self) -> Option<u64> {
            None
        }

        fn content_type(&self) -> HeaderValue {
            HeaderValue::from_static("text/plain")
        }

        async fn write(self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
            w.write_all(b"hello ").await.unwrap();
            w.flush().await.unwrap();
            w.write_all(b"world").await.unwrap();
            Ok(())
        }

        async fn reset(self: Pin<&mut Self>) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn streamed_body() {
        let service = BodyWriteLayer.layer(tower::service_fn(|req: Request<_>| async move {
            assert_eq!(req.headers().get(CONTENT_LENGTH), None);
            assert_eq!(req.headers().get(CONTENT_TYPE).unwrap(), "text/plain");

            match req.body() {
                HyperBody::Stream { .. } => {}
                _ => panic!("expected streaming body"),
            }
            let body = hyper::body::to_bytes(req.into_body()).await.unwrap();
            assert_eq!(body, "hello world");

            Ok(())
        }));

        let mut body = StreamedBody;
        let request = Request::new(Some(Pin::new(&mut body)));
        service.oneshot(request).await.unwrap();
    }

    struct StreamedInfiniteBody;

    #[async_trait]
    impl Body for StreamedInfiniteBody {
        fn content_length(&self) -> Option<u64> {
            None
        }

        fn content_type(&self) -> HeaderValue {
            HeaderValue::from_static("text/plain")
        }

        async fn write(self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
            loop {
                w.write_all(b"hello").await.map_err(Error::internal_safe)?;
                w.flush().await.map_err(Error::internal_safe)?;
            }
        }

        async fn reset(self: Pin<&mut Self>) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn streamed_body_hangup() {
        let service =
            BodyWriteLayer.layer(tower::service_fn(|req: Request<HyperBody>| async move {
                req.into_body().data().await.unwrap().unwrap();

                Err::<(), _>(Error::internal_safe("blammo"))
            }));

        let mut body = StreamedInfiniteBody;
        let request = Request::new(Some(Pin::new(&mut body)));
        let err = service.oneshot(request).await.err().unwrap();

        assert_eq!(err.cause().to_string(), "blammo");
    }

    struct StreamedErrorBody;

    #[async_trait]
    impl Body for StreamedErrorBody {
        fn content_length(&self) -> Option<u64> {
            None
        }

        fn content_type(&self) -> HeaderValue {
            HeaderValue::from_static("text/plain")
        }

        async fn write(self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
            w.write_all(b"hello ").await.unwrap();
            w.flush().await.unwrap();
            Err(Error::internal_safe("uh oh"))
        }

        async fn reset(self: Pin<&mut Self>) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn streamed_body_error() {
        let service =
            BodyWriteLayer.layer(tower::service_fn(|req: Request<HyperBody>| async move {
                hyper::body::to_bytes(req.into_body())
                    .await
                    .map_err(Error::internal_safe)?;

                Ok(())
            }));

        let mut body = StreamedErrorBody;
        let request = Request::new(Some(Pin::new(&mut body)));
        let err = service.oneshot(request).await.err().unwrap();

        assert_eq!(err.cause().to_string(), "uh oh");
    }
}
