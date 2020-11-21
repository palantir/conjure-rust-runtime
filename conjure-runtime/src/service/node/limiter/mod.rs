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
use crate::service::node::limiter::ciad::{CiadConcurrencyLimiter, EndpointLevel, HostLevel};
use conjure_error::Error;
use futures::future::{self, MaybeDone};
use futures::ready;
use http::{Method, Response};
use parking_lot::Mutex;
use pin_project::pin_project;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

mod ciad;
mod deficit_semaphore;

#[derive(PartialEq, Eq, Hash)]
struct Endpoint {
    method: Method,
    pattern: String,
}

pub struct Limiter {
    host: Arc<CiadConcurrencyLimiter<HostLevel>>,
    endpoints: Mutex<HashMap<Endpoint, Arc<CiadConcurrencyLimiter<EndpointLevel>>>>,
}

impl Limiter {
    pub fn new() -> Self {
        Limiter {
            host: CiadConcurrencyLimiter::new(),
            endpoints: Mutex::new(HashMap::new()),
        }
    }

    pub fn acquire(&self, method: &Method, pattern: &str) -> Acquire {
        Acquire {
            host: future::maybe_done(self.host.clone().acquire()),
            endpoint: self
                .endpoints
                .lock()
                .entry(Endpoint {
                    method: method.clone(),
                    pattern: pattern.to_string(),
                })
                .or_insert_with(CiadConcurrencyLimiter::new)
                .clone()
                .acquire(),
        }
    }
}

#[pin_project]
pub struct Acquire {
    #[pin]
    host: MaybeDone<ciad::Acquire<HostLevel>>,
    #[pin]
    endpoint: ciad::Acquire<EndpointLevel>,
}

impl Future for Acquire {
    type Output = Permit;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        ready!(this.host.as_mut().poll(cx));
        let endpoint = ready!(this.endpoint.poll(cx));

        Poll::Ready(Permit {
            host: this.host.take_output().unwrap(),
            endpoint,
        })
    }
}

pub struct Permit {
    host: ciad::Permit<HostLevel>,
    endpoint: ciad::Permit<EndpointLevel>,
}

impl Permit {
    pub fn on_response<B>(&mut self, response: &Result<Response<B>, Error>) {
        self.host.on_response(response);
        self.endpoint.on_response(response);
    }
}
