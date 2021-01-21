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
use crate::client::BaseBody;
use crate::raw::DefaultRawBody;
use bytes::{Buf, Bytes};
use futures::ready;
use http::{HeaderMap, StatusCode};
use http_body::Body;
use pin_project::pin_project;
use std::error;
use std::io;
use std::marker::PhantomPinned;
use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncRead, ReadBuf};

/// An asynchronous HTTP response.
pub struct Response<B = DefaultRawBody> {
    status: StatusCode,
    headers: HeaderMap,
    body: ResponseBody<B>,
}

impl<B> Response<B> {
    pub(crate) fn new(response: hyper::Response<BaseBody<B>>) -> Response<B> {
        let (parts, body) = response.into_parts();
        let body = ResponseBody::new(body);

        Response {
            status: parts.status,
            headers: parts.headers,
            body,
        }
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
    pub fn into_body(self) -> ResponseBody<B> {
        self.body
    }
}

/// An asynchronous streaming response body.
#[pin_project]
pub struct ResponseBody<B = DefaultRawBody> {
    #[pin]
    body: FuseBody<BaseBody<B>>,
    cur: Bytes,
    // Make sure we can make our internal BaseBody !Unpin in the future if we want
    #[pin]
    _p: PhantomPinned,
}

impl<B> ResponseBody<B> {
    fn new(body: BaseBody<B>) -> ResponseBody<B> {
        ResponseBody {
            body: FuseBody::new(body),
            cur: Bytes::new(),
            _p: PhantomPinned,
        }
    }
}

impl<B> ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    /// Reads the next chunk of bytes from the response.
    ///
    /// Compared to the `AsyncRead` implementation, this method avoids some copies of the body data when working with
    /// an API that already consumes `Bytes` objects.
    pub async fn read_bytes(self: Pin<&mut Self>) -> io::Result<Option<Bytes>> {
        let mut this = self.project();

        if this.cur.has_remaining() {
            Ok(Some(mem::replace(this.cur, Bytes::new())))
        } else {
            this.body.data().await.transpose()
        }
    }

    pub(crate) fn buffer(&self) -> &[u8] {
        &self.cur
    }
}

impl<B> AsyncRead for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
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

impl<B> AsyncBufRead for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    fn poll_fill_buf(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        while !self.cur.has_remaining() {
            match ready!(self.as_mut().project().body.poll_data(cx)).transpose()? {
                Some(bytes) => *self.as_mut().project().cur = bytes,
                None => break,
            }
        }

        Poll::Ready(Ok(self.project().cur))
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.project().cur.advance(amt);
    }
}

#[pin_project]
struct FuseBody<B> {
    #[pin]
    body: B,
    done: bool,
}

impl<B> FuseBody<B> {
    fn new(body: B) -> FuseBody<B> {
        FuseBody { body, done: false }
    }
}

impl<B> Body for FuseBody<B>
where
    B: Body,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();

        if *this.done {
            return Poll::Ready(None);
        }

        let chunk = ready!(this.body.poll_data(cx));
        if chunk.is_none() {
            *this.done = true;
        }

        Poll::Ready(chunk)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        self.project().body.poll_trailers(cx)
    }
}
