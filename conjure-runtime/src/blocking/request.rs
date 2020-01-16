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
use crate::blocking::{runtime, Body, BodyShim, BodyStreamer, Client, Response};
use crate::Request;
use conjure_error::Error;
use conjure_object::BearerToken;
use futures::channel::oneshot;
use futures::executor;
use hyper::{HeaderMap, Method};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use zipkin::TraceContext;

/// A builder for a blocking HTTP request.
pub struct RequestBuilder<'a> {
    client: &'a Client,
    request: Request<'static>,
    streamer: Option<BodyStreamer<Box<dyn Body + 'a>>>,
}

impl<'a> RequestBuilder<'a> {
    pub(crate) fn new(
        client: &'a Client,
        method: Method,
        pattern: &'static str,
    ) -> RequestBuilder<'a> {
        RequestBuilder {
            client,
            request: Request::new(&client.0, method, pattern),
            streamer: None,
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
        T: Body + 'a,
    {
        let (body, streamer) = BodyShim::new(Box::new(body) as _);
        self.request.body(body);
        self.streamer = Some(streamer);
        self
    }

    /// Makes the request.
    pub fn send(self) -> Result<Response, Error> {
        let (sender, receiver) = oneshot::channel();
        let client = self.client.0.clone();
        let request = self.request;
        runtime()
            .map_err(Error::internal_safe)?
            .spawn(ContextFuture::new(async move {
                let r = client.send(request).await;
                let _ = sender.send(r);
            }));

        if let Some(streamer) = self.streamer {
            streamer.stream();
        }

        match executor::block_on(receiver) {
            Ok(Ok(r)) => Ok(Response::new(r)),
            Ok(Err(e)) => Err(e.with_backtrace()),
            Err(e) => Err(Error::internal_safe(e)),
        }
    }
}

#[pin_project]
struct ContextFuture<F> {
    #[pin]
    future: F,
    context: Option<TraceContext>,
}

impl<F> ContextFuture<F>
where
    F: Future,
{
    fn new(future: F) -> ContextFuture<F> {
        ContextFuture {
            future,
            context: zipkin::current(),
        }
    }
}

impl<F> Future for ContextFuture<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self.project();
        let _guard = this.context.map(zipkin::set_current);
        this.future.poll(cx)
    }
}
