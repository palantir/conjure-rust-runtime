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
use crate::{Body, Client, ResetTrackingBody, Response};
use conjure_error::Error;
use conjure_object::BearerToken;
use hyper::header::{HeaderValue, ACCEPT};
use hyper::http::header::AUTHORIZATION;
use hyper::{HeaderMap, Method};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::pin::Pin;

static DEFAULT_ACCEPT: Lazy<HeaderValue> = Lazy::new(|| HeaderValue::from_static("*/*"));

/// A builder for an asynchronous HTTP request.
pub struct RequestBuilder<'a> {
    pub(crate) client: &'a Client,
    pub(crate) request: Request<'a>,
}

impl<'a> RequestBuilder<'a> {
    pub(crate) fn new(client: &'a Client, method: Method, pattern: &'a str) -> RequestBuilder<'a> {
        RequestBuilder {
            client,
            request: Request::new(client, method, pattern),
        }
    }

    /// Returns a mutable reference to the headers of this request.
    ///
    /// The following headers are set by default, but can be overridden:
    ///
    /// * `Accept-Encoding: gzip, deflate`
    /// * `Accept: */*`
    /// * `User-Agent: <provided at Client construction>`
    ///
    /// The following headers are fully controlled by `conjure_runtime`, which will overwrite any existing value.
    ///
    /// * `Connection`
    /// * `Content-Length`
    /// * `Content-Type`
    /// * `Host`
    /// * `Proxy-Authorization`
    /// * `X-B3-Flags`
    /// * `X-B3-ParentSpanId`
    /// * `X-B3-Sampled`
    /// * `X-B3-SpanId`
    /// * `X-B3-TraceId`
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.request.headers
    }

    /// Sets the `Authorization` request header to a bearer token.
    ///
    /// This is a simple convenience wrapper.
    pub fn bearer_token(mut self, token: &BearerToken) -> RequestBuilder<'a> {
        self.request.bearer_token(token);
        self
    }

    /// Adds a parameter.
    ///
    /// Parameters which match names in the path pattern will be treated as
    /// path parameters, and other parameters will be treated as query
    /// parameters. Only one instance of path parameters may be provided, but
    /// multiple instances of query parameters may be provided.
    #[allow(clippy::needless_pass_by_value)] // we intentionally take T by value here
    pub fn param<T>(mut self, name: &str, value: T) -> RequestBuilder<'a>
    where
        T: ToString,
    {
        self.request.param(name, value);
        self
    }

    /// Sets the idempotency of the request.
    ///
    /// Idempotent requests can be retried if an IO error is encountered.
    ///
    /// This is by default derived from the HTTP method. `GET`, `HEAD`,
    /// `OPTIONS`, `TRACE`, `PUT`, and `DELETE` are defined as idempotent.
    pub fn idempotent(mut self, idempotent: bool) -> RequestBuilder<'a> {
        self.request.idempotent = idempotent;
        self
    }

    /// Sets the request body.
    pub fn body<T>(mut self, body: T) -> RequestBuilder<'a>
    where
        T: Body + Sync + Send + 'a,
    {
        self.request.body(body);
        self
    }

    /// Makes the request.
    pub async fn send(self) -> Result<Response, Error> {
        self.client.send(self.request).await
    }
}

pub(crate) struct Request<'a> {
    pub(crate) method: Method,
    pub(crate) pattern: &'a str,
    pub(crate) params: HashMap<String, Vec<String>>,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Option<Pin<Box<ResetTrackingBody<dyn Body + Sync + Send + 'a>>>>,
    pub(crate) idempotent: bool,
    pub(crate) propagate_qos_errors: bool,
    pub(crate) propagate_service_errors: bool,
}

impl<'a> Request<'a> {
    pub(crate) fn new(client: &Client, method: Method, pattern: &'a str) -> Request<'a> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, DEFAULT_ACCEPT.clone());

        Request {
            idempotent: client.assume_idempotent() || method.is_idempotent(),
            method,
            pattern,
            params: HashMap::new(),
            headers,
            body: None,
            propagate_qos_errors: client.propagate_qos_errors(),
            propagate_service_errors: client.propagate_service_errors(),
        }
    }

    pub(crate) fn bearer_token(&mut self, token: &BearerToken) {
        let value = format!("Bearer {}", token.as_str());
        let value = HeaderValue::try_from(value).expect("already checked syntax");
        self.headers.insert(AUTHORIZATION, value);
    }

    #[allow(clippy::needless_pass_by_value)] // we intentionally take T by value here
    pub(crate) fn param<T>(&mut self, name: &str, value: T)
    where
        T: ToString,
    {
        self.params
            .entry(name.to_string())
            .or_insert_with(Vec::new)
            .push(value.to_string());
    }

    pub(crate) fn body<T>(&mut self, body: T)
    where
        T: Body + Sync + Send + 'a,
    {
        self.body = Some(Box::pin(ResetTrackingBody::new(body)));
    }
}
