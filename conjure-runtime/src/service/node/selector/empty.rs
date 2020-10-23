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
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::layer::Layer;
use tower::Service;

/// A node selector that always returns an error.
pub struct EmptyNodeSelectorLayer;

impl<S> Layer<S> for EmptyNodeSelectorLayer {
    type Service = EmptyNodeSelectorService<S>;

    fn layer(&self, _: S) -> Self::Service {
        EmptyNodeSelectorService { _p: PhantomData }
    }
}

pub struct EmptyNodeSelectorService<S> {
    _p: PhantomData<S>,
}

impl<S, R> Service<R> for EmptyNodeSelectorService<S>
where
    S: Service<R, Error = Error>,
{
    type Response = S::Response;
    type Error = Error;
    type Future = EmptyNodeSelectorFuture<S::Future>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: R) -> Self::Future {
        EmptyNodeSelectorFuture { _p: PhantomData }
    }
}

pub struct EmptyNodeSelectorFuture<F> {
    _p: PhantomData<F>,
}

impl<F, R> Future for EmptyNodeSelectorFuture<F>
where
    F: Future<Output = Result<R, Error>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(Err(Error::internal_safe("service configured with no URIs")))
    }
}
