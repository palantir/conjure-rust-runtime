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
use conjure_error::Error;
use pin_project::pin_project;
use std::error;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::layer::Layer;
use tower::Service;

/// A layer which converts the error type of a service to an internal `conjure_error::Error`.
pub struct MapErrorLayer;

impl<S> Layer<S> for MapErrorLayer {
    type Service = MapErrorService<S>;

    fn layer(&self, inner: S) -> MapErrorService<S> {
        MapErrorService { inner }
    }
}

pub struct MapErrorService<S> {
    inner: S,
}

impl<S, R> Service<R> for MapErrorService<S>
where
    S: Service<R>,
    S::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type Error = Error;
    type Response = S::Response;
    type Future = MapErrorFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        self.inner.poll_ready(cx).map_err(Error::internal)
    }

    fn call(&mut self, req: R) -> Self::Future {
        MapErrorFuture {
            future: self.inner.call(req),
        }
    }
}

#[pin_project]
pub struct MapErrorFuture<F> {
    #[pin]
    future: F,
}

impl<F, T, E> Future for MapErrorFuture<F>
where
    F: Future<Output = Result<T, E>>,
    E: Into<Box<dyn error::Error + Sync + Send>>,
{
    type Output = Result<T, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().future.poll(cx).map_err(Error::internal)
    }
}
