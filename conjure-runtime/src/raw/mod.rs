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
//! "Raw" HTTP client APIs.
//!
//! The [`conjure_runtime::Client`] wraps a raw HTTP client, which is used to handle the actual HTTP communication. A
//! default raw client is provided, but this can be overridden if desired.
//!
//! # Behavior
//!
//! The raw client interacts directly with the [`http::Request`] and [`http::Response`] types, with a body type
//! implementing the [`http_body::Body`] trait rather than using the [`AsyncBody`] type. The request's URI is provided in
//! absolute-form, and all headers have already been set in the header map. The HTTP response should be
//! returned directly, without any interpretation of the status code, handling of redirects, etc.
//!
//! A raw client is expected to implement `Service<Request<RawBody>, Response = Response<B>>`, where `B` implements the
//! [`http_body::Body`] trait. The error type returned by the client must implement `Into<Box<dyn Error + Sync + Send>>`
//! and be safe-loggable.
//!
//! Some configuration set in the [`Builder`] affects the raw client, including the connect timeout, security
//! configuration, and proxy configuration. The default raw client respects these settings, but other implementations
//! will need to handle them on their own.
//!
//! [`conjure_runtime::Client`]: crate::Client
//! [`AsyncBody`]: conjure_http::client::AsyncBody
use crate::builder;
pub use crate::raw::body::*;
pub use crate::raw::default::*;
use crate::Builder;
use conjure_error::Error;
use std::future::Future;
use std::sync::Arc;

mod body;
mod default;

/// An asynchronous function from request to response.
///
/// This trait is based on the `tower::Service` trait, but differs in two ways. It does not have a `poll_ready` method
/// as our client-side backpressure depends on the request, and the `call` method takes `&self` rather than `&mut self`
/// as our client is designed to be used through a shared reference.
pub trait Service<R> {
    /// The response type returned by the service.
    type Response;
    /// The error type returned by the service.
    type Error;

    /// Asynchronously perform the request.
    fn call(&self, req: R) -> impl Future<Output = Result<Self::Response, Self::Error>> + Send;
}

impl<R, T> Service<R> for Arc<T>
where
    T: ?Sized + Service<R>,
{
    type Response = T::Response;
    type Error = T::Error;

    fn call(&self, req: R) -> impl Future<Output = Result<T::Response, T::Error>> {
        (**self).call(req)
    }
}

/// A factory of raw HTTP clients.
pub trait BuildRawClient {
    /// The raw client type.
    type RawClient;

    /// Creates a new raw client.
    fn build_raw_client(
        &self,
        builder: &Builder<builder::Complete<Self>>,
    ) -> Result<Self::RawClient, Error>
    where
        Self: Sized;
}
