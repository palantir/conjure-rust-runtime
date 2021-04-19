// Copyright 2021 Palantir Technologies, Inc.
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
use crate::errors::{RemoteError, ThrottledError, UnavailableError};
use crate::service::request::Pattern;
use conjure_error::Error;
use futures::ready;
use http::{Request, Response, StatusCode};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use zipkin::{Detached, OpenSpan};

pub fn add_request_tags<A, B>(span: &mut OpenSpan<A>, request: &Request<B>) {
    span.tag("method", request.method().as_str());
    span.tag(
        "pattern",
        request
            .extensions()
            .get::<Pattern>()
            .expect("Pattern missing from request extensions")
            .pattern,
    );
}

#[pin_project]
pub struct HttpSpanFuture<F> {
    #[pin]
    inner: F,
    span: Option<OpenSpan<Detached>>,
}

impl<F> HttpSpanFuture<F> {
    pub fn new(inner: F, span: OpenSpan<Detached>) -> Self {
        HttpSpanFuture {
            inner,
            span: Some(span),
        }
    }
}

impl<F, B> Future for HttpSpanFuture<F>
where
    F: Future<Output = Result<Response<B>, Error>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let span = this.span.as_mut().expect("future polled after completion");
        let _guard = zipkin::set_current(span.context());
        let r = ready!(this.inner.poll(cx));

        match &r {
            Ok(response) => {
                let outcome = if response.status().is_success() {
                    "success"
                } else {
                    "failure"
                };
                span.tag("outcome", outcome);
                // StatusCode::as_str returns the numeric representation, not the canonical reason.
                span.tag("status", response.status().as_str());
            }
            Err(e) => {
                span.tag("outcome", "failure");

                let status = if e.cause().is::<ThrottledError>() {
                    Some(&StatusCode::TOO_MANY_REQUESTS)
                } else if e.cause().is::<UnavailableError>() {
                    Some(&StatusCode::SERVICE_UNAVAILABLE)
                } else if let Some(e) = e.cause().downcast_ref::<RemoteError>() {
                    Some(e.status())
                } else {
                    None
                };

                if let Some(status) = status {
                    span.tag("status", status.as_str());
                }
            }
        }

        *this.span = None;
        Poll::Ready(r)
    }
}
