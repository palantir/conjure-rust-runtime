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
use bytes::{Buf, Bytes};
use futures::ready;
use http::{HeaderMap, StatusCode};
use http_body::Body;
use pin_project::pin_project;
use std::io;
use std::mem;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncRead};

/// An asynchronous HTTP response.
pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: ResponseBody,
}

impl Response {
    pub(crate) fn new<B>(response: hyper::Response<B>) -> Response
    where
        B: Body<Data = Bytes, Error = io::Error> + 'static + Sync + Send,
    {
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
    pub fn into_body(self) -> ResponseBody {
        self.body
    }
}

/// An asynchronous streaming response body.
pub struct ResponseBody {
    body: Pin<Box<dyn Body<Data = Bytes, Error = io::Error> + Sync + Send>>,
    cur: Bytes,
}

impl ResponseBody {
    fn new<B>(body: B) -> ResponseBody
    where
        B: Body<Data = Bytes, Error = io::Error> + 'static + Sync + Send,
    {
        ResponseBody {
            body: Box::pin(FuseBody::new(body)),
            cur: Bytes::new(),
        }
    }

    /// Reads the next chunk of bytes from the response.
    ///
    /// Compared to the `AsyncRead` implementation, this method avoids some copies of the body data when working with
    /// an API that already consumes `Bytes` objects.
    pub async fn read_bytes(&mut self) -> io::Result<Option<Bytes>> {
        if self.cur.has_remaining() {
            Ok(Some(mem::replace(&mut self.cur, Bytes::new())))
        } else {
            self.body.data().await.transpose()
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
            match ready!(Pin::new(&mut self.body).poll_data(cx)).transpose()? {
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
