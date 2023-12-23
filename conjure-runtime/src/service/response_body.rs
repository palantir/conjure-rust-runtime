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
use crate::{BaseBody, ResponseBody};
use bytes::Bytes;
use http::Response;
use http_body::Body;
use pin_project::pin_project;
use std::error;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A layer which wraps the response body in the conjure-runtime public `ResponseBody` type.
pub struct ResponseBodyLayer;

impl<S> Layer<S> for ResponseBodyLayer {
    type Service = ResponseBodyService<S>;

    fn layer(self, inner: S) -> Self::Service {
        ResponseBodyService { inner }
    }
}

pub struct ResponseBodyService<S> {
    inner: S,
}

impl<S, R, B> Service<R> for ResponseBodyService<S>
where
    S: Service<R, Response = Response<BaseBody<B>>>,
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type Response = Response<ResponseBody<B>>;
    type Error = S::Error;

    fn call(&self, req: R) -> impl Future<Output = Result<Self::Response, Self::Error>> {
        ResponseBodyFuture {
            future: self.inner.call(req),
        }
    }
}

#[pin_project]
pub struct ResponseBodyFuture<F> {
    #[pin]
    future: F,
}

impl<F, B, E> Future for ResponseBodyFuture<F>
where
    F: Future<Output = Result<Response<BaseBody<B>>, E>>,
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type Output = Result<Response<ResponseBody<B>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project()
            .future
            .poll(cx)
            .map_ok(|res| res.map(ResponseBody::new))
    }
}
