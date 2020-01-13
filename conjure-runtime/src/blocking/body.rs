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
use crate::blocking::BodyWriter;
use bytes::Bytes;
use conjure_error::Error;
use hyper::header::HeaderValue;

/// A synchronous request body.
pub trait Body {
    /// Returns the length of the body if known.
    fn content_length(&self) -> Option<u64>;

    /// Returns the content type of the body.
    fn content_type(&self) -> HeaderValue;

    /// Returns the entire body if it is fully buffered.
    ///
    /// `write` will only be called if this method returns `None`.
    fn full_body(&self) -> Option<Bytes> {
        None
    }

    /// Writes the body data out.
    fn write(&mut self, w: &mut BodyWriter) -> Result<(), Error>;

    /// Resets the body to its start.
    ///
    /// Returns `true` iff the body was successfully reset.
    ///
    /// Requests with non-resettable bodies cannot be retried.
    fn reset(&mut self) -> bool;
}

impl<T> Body for Box<T>
where
    T: ?Sized + Body,
{
    fn content_length(&self) -> Option<u64> {
        (**self).content_length()
    }

    fn content_type(&self) -> HeaderValue {
        (**self).content_type()
    }

    fn full_body(&self) -> Option<Bytes> {
        (**self).full_body()
    }

    fn write(&mut self, w: &mut BodyWriter) -> Result<(), Error> {
        (**self).write(w)
    }

    fn reset(&mut self) -> bool {
        (**self).reset()
    }
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

    fn write(&mut self, _: &mut BodyWriter) -> Result<(), Error> {
        unreachable!()
    }

    fn reset(&mut self) -> bool {
        true
    }
}
