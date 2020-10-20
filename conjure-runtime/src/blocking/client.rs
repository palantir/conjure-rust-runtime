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
use crate::blocking::RequestBuilder;
use crate::Builder;
use http::Method;

/// A blocking HTTP client to a remote service.
///
/// It implements the Conjure `Client` trait, but also offers a "raw" request interface for use with services that don't
/// provide Conjure service definitions.
#[derive(Clone)]
pub struct Client(pub(crate) crate::Client);

impl Client {
    /// Returns a new `Builder` for clients.
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// Returns a new request builder.
    ///
    /// The `pattern` argument is a template for the request path. The `param` method on the builder is used to fill
    /// in the parameters in the pattern with dynamic values.
    pub fn request(&self, method: Method, pattern: &'static str) -> RequestBuilder<'_> {
        RequestBuilder::new(self, method, pattern)
    }

    /// Returns a new builder for a GET request.
    pub fn get(&self, pattern: &'static str) -> RequestBuilder<'_> {
        self.request(Method::GET, pattern)
    }

    /// Returns a new builder for a POST request.
    pub fn post(&self, pattern: &'static str) -> RequestBuilder<'_> {
        self.request(Method::POST, pattern)
    }

    /// Returns a new builder for a PUT request.
    pub fn put(&self, pattern: &'static str) -> RequestBuilder<'_> {
        self.request(Method::PUT, pattern)
    }

    /// Returns a new builder for a DELETE request.
    pub fn delete(&self, pattern: &'static str) -> RequestBuilder<'_> {
        self.request(Method::DELETE, pattern)
    }

    /// Returns a new builder for a PATCH request.
    pub fn patch(&self, pattern: &'static str) -> RequestBuilder<'_> {
        self.request(Method::PATCH, pattern)
    }
}
