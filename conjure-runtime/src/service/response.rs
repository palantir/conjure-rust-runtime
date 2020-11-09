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
use crate::client::BaseBody;
use crate::raw::Service;
use crate::service::Layer;
use crate::Response;
use bytes::Bytes;
use http_body::Body;
use pin_project::pin_project;
use std::error;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A layer which converts a hyper `Response` to a conjure-runtime `Response`.
pub struct ResponseLayer;

impl<S> Layer<S> for ResponseLayer {
    type Service = ResponseService<S>;

    fn layer(self, inner: S) -> ResponseService<S> {
        ResponseService { inner }
    }
}

pub struct ResponseService<S> {
    inner: S,
}

impl<S, R, B> Service<R> for ResponseService<S>
where
    S: Service<R, Response = http::Response<BaseBody<B>>>,
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type Response = Response<B>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn call(&self, req: R) -> Self::Future {
        ResponseFuture {
            future: self.inner.call(req),
        }
    }
}

#[pin_project]
pub struct ResponseFuture<F> {
    #[pin]
    future: F,
}

impl<F, B, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<http::Response<BaseBody<B>>, E>>,
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        this.future.poll(cx).map_ok(Response::new)
    }
}
