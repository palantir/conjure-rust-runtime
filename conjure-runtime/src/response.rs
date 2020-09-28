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
use crate::service::gzip::DecodedBody;
use bytes::{Buf, Bytes};
use conjure_error::Error;
use futures::stream::Fuse;
use futures::{ready, Future, Stream, StreamExt, TryStreamExt};
use hyper::{HeaderMap, StatusCode};
use pin_project::pin_project;
use std::io;
use std::mem;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncReadExt};
use tokio::time::{self, Delay};
use witchcraft_log::info;
use zipkin::{Detached, OpenSpan};

/// An asynchronous HTTP response.
pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: ResponseBody,
}

impl Response {
    pub(crate) fn new(
        response: hyper::Response<DecodedBody>,
        deadline: Instant,
        span: OpenSpan<Detached>,
    ) -> Result<Response, Error> {
        let (parts, body) = response.into_parts();
        let body = ResponseBody::new(body, deadline, span)?;

        Ok(Response {
            status: parts.status,
            headers: parts.headers,
            body,
        })
    }

    pub(crate) async fn into_error(self, propagate_service_errors: bool) -> Error {
        let status = self.status();

        let mut buf = vec![];
        // limit how much we read in case something weird's going on
        if let Err(e) = self.into_body().take(10 * 1024).read_to_end(&mut buf).await {
            info!(
                "error reading response body",
                error: Error::internal_safe(e),
            );
        }

        let error = RemoteError {
            status,
            error: conjure_serde::json::client_from_slice(&buf).ok(),
        };
        let log_body = error.error.is_none();
        let mut error = match &error.error {
            Some(e) if propagate_service_errors => {
                let e = e.clone();
                Error::service_safe(error, e)
            }
            _ => Error::internal_safe(error),
        };
        if log_body {
            error = error.with_unsafe_param("body", String::from_utf8_lossy(&buf));
        }

        error
    }

    /// Returns the response's status.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns the response's headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Consumes the response, returning its body.
    pub fn into_body(self) -> ResponseBody {
        self.body
    }
}

/// An asynchronous streaming response body.
pub struct ResponseBody {
    stream: Fuse<TimeoutStream<DecodedBody>>,
    cur: Bytes,
    _span: OpenSpan<Detached>,
}

impl ResponseBody {
    #[allow(clippy::borrow_interior_mutable_const)]
    fn new(
        body: DecodedBody,
        deadline: Instant,
        span: OpenSpan<Detached>,
    ) -> Result<ResponseBody, Error> {
        let stream = TimeoutStream {
            stream: body,
            deadline: time::delay_until(time::Instant::from(deadline)),
        };

        Ok(ResponseBody {
            stream: stream.fuse(),
            cur: Bytes::new(),
            _span: span,
        })
    }

    /// Reads the next chunk of bytes from the response.
    ///
    /// Compared to the `AsyncRead` implementation, this method avoids some copies of the body data when working with
    /// an API that already consumes `Bytes` objects.
    pub async fn read_bytes(&mut self) -> io::Result<Option<Bytes>> {
        if self.cur.has_remaining() {
            Ok(Some(mem::replace(&mut self.cur, Bytes::new())))
        } else {
            self.stream.try_next().await
        }
    }
}

impl AsyncRead for ResponseBody {
    unsafe fn prepare_uninitialized_buffer(&self, _: &mut [MaybeUninit<u8>]) -> bool {
        false
    }

    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let read_buf = ready!(self.as_mut().poll_fill_buf(cx))?;
        let nread = usize::min(buf.len(), read_buf.len());
        buf[..nread].copy_from_slice(&read_buf[..nread]);
        self.consume(nread);
        Poll::Ready(Ok(nread))
    }
}

impl AsyncBufRead for ResponseBody {
    fn poll_fill_buf(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        while !self.cur.has_remaining() {
            match ready!(self.stream.poll_next_unpin(cx)).transpose()? {
                Some(bytes) => self.cur = bytes,
                None => break,
            }
        }

        Poll::Ready(Ok(&self.get_mut().cur))
    }

    fn consume(mut self: Pin<&mut Self>, amt: usize) {
        self.cur.advance(amt);
    }
}

#[pin_project]
struct TimeoutStream<T> {
    #[pin]
    stream: T,
    #[pin]
    deadline: Delay,
}

impl<T, U> Stream for TimeoutStream<T>
where
    T: Stream<Item = io::Result<U>>,
{
    type Item = io::Result<U>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        if this.deadline.poll(cx).is_ready() {
            return Poll::Ready(Some(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                TimeoutError(()),
            ))));
        }

        this.stream.poll_next(cx)
    }
}
