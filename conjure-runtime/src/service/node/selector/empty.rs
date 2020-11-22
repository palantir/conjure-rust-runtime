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
use conjure_error::Error;
use http::Request;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// A node selector that always returns an error.
pub struct EmptyNodeSelectorLayer {
    service: Arc<str>,
}

impl EmptyNodeSelectorLayer {
    pub fn new(service: &str) -> EmptyNodeSelectorLayer {
        EmptyNodeSelectorLayer {
            service: service.into(),
        }
    }
}

impl<S> Layer<S> for EmptyNodeSelectorLayer {
    type Service = EmptyNodeSelectorService<S>;

    fn layer(self, _: S) -> Self::Service {
        EmptyNodeSelectorService {
            service: self.service,
            _p: PhantomData,
        }
    }
}

pub struct EmptyNodeSelectorService<S> {
    service: Arc<str>,
    _p: PhantomData<S>,
}

impl<S, B> Service<Request<B>> for EmptyNodeSelectorService<S>
where
    S: Service<Request<B>, Error = Error>,
{
    type Response = S::Response;
    type Error = Error;
    type Future = EmptyNodeSelectorFuture<S, B>;

    fn call(&self, _: Request<B>) -> Self::Future {
        EmptyNodeSelectorFuture {
            service: self.service.clone(),
            _p: PhantomData,
        }
    }
}

pub struct EmptyNodeSelectorFuture<S, B> {
    service: Arc<str>,
    _p: PhantomData<(S, B)>,
}

impl<S, B> Future for EmptyNodeSelectorFuture<S, B>
where
    S: Service<Request<B>, Error = Error>,
{
    type Output = Result<S::Response, S::Error>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(Err(Error::internal_safe("service configured with no URIs")
            .with_safe_param("service", &*self.service)))
    }
}
