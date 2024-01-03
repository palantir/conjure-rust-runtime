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
use std::future::Future;

pub mod gzip;
pub mod http_error;
pub mod map_error;
pub mod metrics;
pub mod node;
pub mod proxy;
pub mod response_body;
pub mod retry;
pub mod root_span;
pub mod timeout;
pub mod tls_metrics;
pub mod trace_propagation;
pub mod user_agent;
pub mod wait_for_spans;

/// A function from one service type to another.
///
/// This trait is based off of the `tower::layer::Layer` trait, but differs in one way. The `layer` method takes `self`
/// rather than `&self`. We build a single service when constructing the client and keep that around, so there's no need
/// to allow for layers to be reused. The `Layer` trait exists solely to help make the initialization of the client's
/// service more orderly and maintainable.
pub trait Layer<S> {
    type Service;

    fn layer(self, inner: S) -> Self::Service;
}

/// A layer which does nothing.
pub struct Identity;

impl<S> Layer<S> for Identity {
    type Service = S;

    fn layer(self, inner: S) -> Self::Service {
        inner
    }
}

/// A layer which applies two other layers in order.
///
/// # Note
///
/// The parameter order here is reversed when compared to `tower::layer::Stack`, as it makes the type definition macro
/// in the `client` module cleaner.
pub struct Stack<T, U> {
    inner: U,
    outer: T,
}

impl<T, U, S> Layer<S> for Stack<T, U>
where
    T: Layer<U::Service>,
    U: Layer<S>,
{
    type Service = T::Service;

    fn layer(self, inner: S) -> Self::Service {
        let inner = self.inner.layer(inner);
        self.outer.layer(inner)
    }
}

/// A builder type for `Service`s.
///
/// The builder allows you to "linearize" the layers composed to build a service in a builder-style API rather than
/// having to make repeated nested calls.
pub struct ServiceBuilder<L> {
    layer: L,
}

impl ServiceBuilder<Identity> {
    pub fn new() -> Self {
        ServiceBuilder { layer: Identity }
    }
}

impl<L> ServiceBuilder<L> {
    pub fn layer<T>(self, layer: T) -> ServiceBuilder<Stack<L, T>> {
        ServiceBuilder {
            layer: Stack {
                inner: layer,
                outer: self.layer,
            },
        }
    }

    pub fn service<S>(self, service: S) -> L::Service
    where
        L: Layer<S>,
    {
        self.layer.layer(service)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn service_fn<T>(f: T) -> ServiceFn<T> {
    ServiceFn(f)
}

pub struct ServiceFn<T>(T);

impl<T, R, F, S, E> Service<R> for ServiceFn<T>
where
    T: Fn(R) -> F,
    F: Future<Output = Result<S, E>> + Send,
{
    type Response = S;
    type Error = E;

    fn call(&self, req: R) -> impl Future<Output = Result<S, E>> {
        (self.0)(req)
    }
}
