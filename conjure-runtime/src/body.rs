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
use crate::BodyWriter;
use async_trait::async_trait;
use bytes::Bytes;
use conjure_error::Error;
use hyper::header::HeaderValue;
use pin_project::pin_project;
use std::pin::Pin;

/// A request body.
///
/// This trait can be most easily implemented with the [async-trait crate](https://docs.rs/async-trait). While it
/// supports both in-memory and streaming bodies, in-memory bodies can use the `BytesBody` type rather than creating a
/// new implementation of this trait.
///
/// # Examples
///
/// ```
/// use async_trait::async_trait;
/// use conjure_runtime::{Body, BodyWriter};
/// use conjure_error::Error;
/// use http::HeaderValue;
/// use std::pin::Pin;
/// use tokio::io::AsyncWriteExt;
///
/// pub struct SimpleBody;
///
/// #[async_trait]
/// impl Body for SimpleBody {
///     fn content_length(&self) -> Option<u64> {
///         None
///     }
///
///     fn content_type(&self) -> HeaderValue {
///         HeaderValue::from_static("application/octet-stream")
///     }
///
///     async fn write(self: Pin<&mut Self>, mut w: Pin<&mut BodyWriter>) -> Result<(), Error> {
///         w.write_all(b"hello world").await.map_err(Error::internal_safe)
///     }
///
///     async fn reset(self: Pin<&mut Self>) -> bool {
///         true
///     }
/// }
/// ```
#[async_trait]
pub trait Body {
    /// Returns the length of the body if known.
    fn content_length(&self) -> Option<u64>;

    /// Returns the content type of the body.
    fn content_type(&self) -> HeaderValue;

    /// Returns the entire body if it is fully buffered.
    ///
    /// `write` will only be called if this method returns `None`.
    ///
    /// The default implementation returns `None`.
    fn full_body(&self) -> Option<Bytes> {
        None
    }

    /// Writes the body data out.
    async fn write(self: Pin<&mut Self>, w: Pin<&mut BodyWriter>) -> Result<(), Error>;

    /// Resets the body to its start.
    ///
    /// Returns `true` iff the body was successfully reset.
    ///
    /// Requests with non-resettable bodies cannot be retried.
    async fn reset(self: Pin<&mut Self>) -> bool;
}

/// A simple type implementing `Body` which consists of a byte buffer and a content type.
///
/// It reports its content length and is resettable.
pub struct BytesBody {
    body: Bytes,
    content_type: HeaderValue,
}

impl BytesBody {
    /// Creates a new `BytesBody`.
    pub fn new<T>(body: T, content_type: HeaderValue) -> BytesBody
    where
        T: Into<Bytes>,
    {
        BytesBody {
            body: body.into(),
            content_type,
        }
    }
}

#[async_trait]
impl Body for BytesBody {
    fn content_length(&self) -> Option<u64> {
        Some(self.body.len() as u64)
    }

    fn content_type(&self) -> HeaderValue {
        self.content_type.clone()
    }

    fn full_body(&self) -> Option<Bytes> {
        Some(self.body.clone())
    }

    async fn write(self: Pin<&mut Self>, _: Pin<&mut BodyWriter>) -> Result<(), Error> {
        unreachable!()
    }

    async fn reset(self: Pin<&mut Self>) -> bool {
        true
    }
}

#[pin_project]
pub(crate) struct ResetTrackingBody<T>
where
    T: ?Sized,
{
    needs_reset: bool,
    #[pin]
    body: T,
}

impl<T> ResetTrackingBody<T>
where
    T: Body + Send,
{
    pub fn new(body: T) -> ResetTrackingBody<T> {
        ResetTrackingBody {
            needs_reset: false,
            body,
        }
    }

    #[cfg(test)]
    pub fn get_ref(&self) -> &T {
        &self.body
    }
}

impl<T> ResetTrackingBody<T>
where
    T: ?Sized,
{
    pub fn needs_reset(&self) -> bool {
        self.needs_reset
    }
}

#[async_trait]
impl<T> Body for ResetTrackingBody<T>
where
    T: ?Sized + Body + Send,
{
    fn content_length(&self) -> Option<u64> {
        self.body.content_length()
    }

    fn content_type(&self) -> HeaderValue {
        self.body.content_type()
    }

    fn full_body(&self) -> Option<Bytes> {
        self.body.full_body()
    }

    async fn write(self: Pin<&mut Self>, w: Pin<&mut BodyWriter>) -> Result<(), Error> {
        let this = self.project();
        *this.needs_reset = true;
        this.body.write(w).await
    }

    async fn reset(self: Pin<&mut Self>) -> bool {
        let this = self.project();
        let ok = this.body.reset().await;
        if ok {
            *this.needs_reset = false;
        }
        ok
    }
}
