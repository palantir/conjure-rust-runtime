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
use crate::util::weak_reducing_gauge::Reduce;
use conjure_error::Error;
use http::{Method, Response};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

mod ciad;
mod deficit_semaphore;

#[derive(PartialEq, Eq, Hash)]
struct Endpoint {
    method: Method,
    pattern: &'static str,
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

    pub fn host_limiter(&self) -> &Arc<CiadConcurrencyLimiter<HostLevel>> {
        &self.host
    }

    pub async fn acquire(&self, method: &Method, pattern: &'static str) -> Permit {
        let endpoint = self
            .endpoints
            .lock()
            .entry(Endpoint {
                method: method.clone(),
                pattern,
            })
            .or_insert_with(CiadConcurrencyLimiter::new)
            .clone();
        // acquire the endpoint permit first to avoid contention issues in the balanced limiter
        // where requests to a throttled endpoint could "lock out" requests to other endpoints if we
        // take the host permit first.
        let endpoint = endpoint.acquire().await;
        let host = self.host.clone().acquire().await;

        Permit { endpoint, host }
    }
}

pub struct Permit {
    endpoint: ciad::Permit<EndpointLevel>,
    host: ciad::Permit<HostLevel>,
}

impl Permit {
    pub fn on_response<B>(&mut self, response: &Result<Response<B>, Error>) {
        self.endpoint.on_response(response);
        self.host.on_response(response);
    }
}

pub struct LimitReducer;

impl Reduce for LimitReducer {
    type Input = CiadConcurrencyLimiter<HostLevel>;
    type Value = f64;

    fn default(&self) -> Self::Value {
        0.
    }

    fn map(&self, v: &Self::Input) -> Self::Value {
        v.limit()
    }

    fn reduce(&self, a: &mut Self::Value, b: Self::Value) {
        *a = f64::min(*a, b);
    }
}

pub struct InFlightReducer;

impl Reduce for InFlightReducer {
    type Input = CiadConcurrencyLimiter<HostLevel>;
    type Value = usize;

    fn default(&self) -> Self::Value {
        0
    }

    fn map(&self, v: &Self::Input) -> Self::Value {
        v.in_flight()
    }

    fn reduce(&self, a: &mut Self::Value, b: Self::Value) {
        *a += b;
    }
}
