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
use crate::blocking::runtime;
use crate::raw::DefaultRawBody;
use bytes::Bytes;
use futures::executor;
use futures::future;
use http_body::Body;
use hyper::{HeaderMap, StatusCode};
use std::error;
use std::io::{self, BufRead, Read};
use std::pin::Pin;
use tokio::io::{AsyncBufRead, AsyncReadExt};

/// A blocking HTTP response.
pub struct Response<B = DefaultRawBody>(crate::Response<B>);

impl<B> Response<B> {
    pub(crate) fn new(inner: crate::Response<B>) -> Response<B> {
        Response(inner)
    }

    /// Returns the response's status.
    pub fn status(&self) -> StatusCode {
        self.0.status()
    }

    /// Returns the response's headers.
    pub fn headers(&self) -> &HeaderMap {
        self.0.headers()
    }

    /// Consumes the response, returning its body.
    pub fn into_body(self) -> ResponseBody<B> {
        ResponseBody(Box::pin(self.0.into_body()))
    }
}

/// A blocking streaming response body.
pub struct ResponseBody<B = DefaultRawBody>(Pin<Box<crate::ResponseBody<B>>>);

impl<B> ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    /// Reads the next chunk of bytes from the response.
    ///
    /// Compared to the `Read` implementation, this method avoids some copies of the body data when working with an API
    /// that already consumes `Bytes` objects.
    pub fn read_bytes(&mut self) -> io::Result<Option<Bytes>> {
        runtime()?.enter(|| executor::block_on(self.0.as_mut().read_bytes()))
    }
}

impl<B> Read for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        runtime()?.enter(|| executor::block_on(self.0.read(buf)))
    }
}

impl<B> BufRead for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        // lifetime shenanigans mean we can't return the value of poll_fill_buf directly
        runtime()?.enter(|| {
            executor::block_on(future::poll_fn(|cx| {
                self.0.as_mut().poll_fill_buf(cx).map_ok(|_| ())
            }))
        })?;
        Ok(self.0.buffer())
    }

    fn consume(&mut self, n: usize) {
        self.0.as_mut().consume(n)
    }
}
