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
use futures::future::BoxFuture;
use std::marker::PhantomData;
use std::task::{Context, Poll};
use tower::layer::Layer;
use tower::Service;

/// A type-erased wrapper for `Layer`s.
pub struct BoxLayer<S, R, U, E>(Box<dyn Layer<S, Service = BoxService<R, U, E>> + Sync + Send>);

impl<S, R, U, E> Layer<S> for BoxLayer<S, R, U, E> {
    type Service = BoxService<R, U, E>;

    fn layer(&self, inner: S) -> Self::Service {
        self.0.layer(inner)
    }
}

impl<S, R, U, E> BoxLayer<S, R, U, E> {
    pub fn new<L>(layer: L) -> BoxLayer<S, R, U, E>
    where
        L: Layer<S> + 'static + Sync + Send,
        L::Service: Service<R, Response = U, Error = E> + 'static + Send,
        <L::Service as Service<R>>::Future: 'static + Send,
        R: 'static,
    {
        BoxLayer(Box::new(LayerShim(layer, PhantomData)))
    }
}

struct LayerShim<L, R>(L, PhantomData<fn(R)>);

impl<L, S, R> Layer<S> for LayerShim<L, R>
where
    L: Layer<S>,
    L::Service: Service<R> + 'static + Send,
    <L::Service as Service<R>>::Future: 'static + Send,
{
    #[allow(clippy::type_complexity)]
    type Service =
        BoxService<R, <L::Service as Service<R>>::Response, <L::Service as Service<R>>::Error>;

    fn layer(&self, inner: S) -> Self::Service {
        BoxService::new(self.0.layer(inner))
    }
}

/// A type-erased wrapper for `Service`s.
pub struct BoxService<R, U, E>(
    Box<dyn Service<R, Response = U, Error = E, Future = BoxFuture<'static, Result<U, E>>> + Send>,
);

impl<R, U, E> Service<R> for BoxService<R, U, E> {
    type Response = U;
    type Error = E;
    type Future = BoxFuture<'static, Result<U, E>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        self.0.call(req)
    }
}

impl<R, U, E> BoxService<R, U, E> {
    pub fn new<S>(service: S) -> BoxService<R, U, E>
    where
        S: Service<R, Response = U, Error = E> + 'static + Send,
        S::Future: 'static + Send,
    {
        BoxService(Box::new(ServiceShim(service)))
    }
}

struct ServiceShim<S>(S);

impl<S, R> Service<R> for ServiceShim<S>
where
    S: Service<R>,
    S::Future: 'static + Send,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        Box::pin(self.0.call(req))
    }
}
