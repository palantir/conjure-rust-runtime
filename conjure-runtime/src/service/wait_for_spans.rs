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
use crate::raw::Service;
use crate::service::Layer;
use http::Response;
use http_body::{Body, Frame, SizeHint};
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};
use zipkin::{Detached, Kind, OpenSpan};

/// A layer which wraps the request future in a `conjure-runtime: wait-for-headers` span, and the response's body in a
/// `conjure-runtime: wait-for-body` span.
pub struct WaitForSpansLayer;

impl<S> Layer<S> for WaitForSpansLayer {
    type Service = WaitForSpansService<S>;

    fn layer(self, inner: S) -> WaitForSpansService<S> {
        WaitForSpansService { inner }
    }
}

pub struct WaitForSpansService<S> {
    inner: S,
}

impl<S, R, B> Service<R> for WaitForSpansService<S>
where
    S: Service<R, Response = Response<B>> + Sync + Send,
    R: Send,
{
    type Response = Response<WaitForSpansBody<B>>;
    type Error = S::Error;

    async fn call(&self, req: R) -> Result<Self::Response, Self::Error> {
        let response = zipkin::next_span()
            .with_name("conjure-runtime: wait-for-headers")
            .with_kind(Kind::Client)
            .detach()
            .bind(self.inner.call(req))
            .await?;

        Ok(response.map(|body| WaitForSpansBody {
            body,
            _span: zipkin::next_span()
                .with_name("conjure-runtime: wait-for-body")
                .detach(),
        }))
    }
}

#[pin_project]
pub struct WaitForSpansBody<B> {
    #[pin]
    body: B,
    _span: OpenSpan<Detached>,
}

impl<B> Body for WaitForSpansBody<B>
where
    B: Body,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.project().body.poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}
