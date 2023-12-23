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
use crate::service::map_error::RawClientError;
use crate::service::node::limiter::deficit_semaphore::{self, DeficitSemaphore};
use crate::util::atomic_f64::AtomicF64;
use conjure_error::Error;
use http::{Response, StatusCode};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const INITIAL_LIMIT: usize = 20;
const BACKOFF_RATIO: f64 = 0.9;
const MIN_LIMIT: usize = 1;
const MAX_LIMIT: usize = 1_000_000;

/// An asynchronous concurrency limiter. "Ciad" stands for "cautious increase, agressive decrease", which is a mouthful!
///
/// The limiter is tagged with a "behavior" which controls how a response from the server affects the limit.
pub struct CiadConcurrencyLimiter<B> {
    semaphore: Arc<DeficitSemaphore>,
    limit: AtomicF64,
    in_flight: AtomicUsize,
    _p: PhantomData<B>,
}

impl<B> CiadConcurrencyLimiter<B> {
    pub fn new() -> Arc<Self> {
        Arc::new(CiadConcurrencyLimiter {
            semaphore: DeficitSemaphore::new(INITIAL_LIMIT),
            limit: AtomicF64::new(INITIAL_LIMIT as f64),
            in_flight: AtomicUsize::new(0),
            _p: PhantomData,
        })
    }

    pub fn limit(&self) -> f64 {
        self.limit.load(Ordering::SeqCst)
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    pub async fn acquire(self: Arc<Self>) -> Permit<B> {
        let permit = self.semaphore.clone().acquire().await;

        Permit {
            in_flight_snapshot: self.in_flight.fetch_add(1, Ordering::SeqCst) + 1,
            mode: Mode::Ignore,
            limiter: self,
            _permit: permit,
        }
    }

    fn update_limit<F>(&self, f: F)
    where
        F: Fn(f64) -> Option<f64>,
    {
        let mut old_limit = self.limit.load(Ordering::SeqCst);

        loop {
            let new_limit = match f(old_limit) {
                Some(new_limit) => new_limit,
                None => return,
            };

            match self.limit.compare_exchange(
                old_limit,
                new_limit,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    let old_limit = old_limit as usize;
                    let new_limit = new_limit as usize;

                    #[allow(clippy::comparison_chain)]
                    if new_limit > old_limit {
                        self.semaphore.add_permits(new_limit - old_limit);
                    } else if old_limit > new_limit {
                        self.semaphore.remove_permits(old_limit - new_limit);
                    }

                    return;
                }
                Err(limit) => old_limit = limit,
            }
        }
    }
}

pub trait Behavior {
    fn on_success<B>(response: &Response<B>) -> Mode;

    fn on_failure(error: &Error) -> Mode;
}

pub enum HostLevel {}

impl Behavior for HostLevel {
    fn on_success<B>(response: &Response<B>) -> Mode {
        match response.status() {
            StatusCode::INTERNAL_SERVER_ERROR | StatusCode::TOO_MANY_REQUESTS => Mode::Ignore,
            code if code.is_server_error() => Mode::Dropped,
            _ => Mode::Success,
        }
    }

    fn on_failure(error: &Error) -> Mode {
        if error.cause().is::<RawClientError>() {
            Mode::Dropped
        } else {
            Mode::Ignore
        }
    }
}

pub enum EndpointLevel {}

impl Behavior for EndpointLevel {
    fn on_success<B>(response: &Response<B>) -> Mode {
        match response.status() {
            StatusCode::INTERNAL_SERVER_ERROR | StatusCode::TOO_MANY_REQUESTS => Mode::Dropped,
            code if code.is_server_error() => Mode::Ignore,
            _ => Mode::Success,
        }
    }

    fn on_failure(_: &Error) -> Mode {
        Mode::Ignore
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum Mode {
    Ignore,
    Success,
    Dropped,
}

pub struct Permit<B> {
    limiter: Arc<CiadConcurrencyLimiter<B>>,
    in_flight_snapshot: usize,
    mode: Mode,
    _permit: deficit_semaphore::Permit,
}

impl<B> Drop for Permit<B> {
    fn drop(&mut self) {
        self.limiter.in_flight.fetch_sub(1, Ordering::SeqCst);

        match self.mode {
            Mode::Ignore => {}
            Mode::Success => self.limiter.update_limit(|original_limit| {
                if self.in_flight_snapshot >= (original_limit * BACKOFF_RATIO) as usize {
                    let increment = 1. / original_limit;
                    Some(f64::min(MAX_LIMIT as f64, original_limit + increment))
                } else {
                    None
                }
            }),
            Mode::Dropped => self.limiter.update_limit(|original_limit| {
                Some(f64::max(
                    MIN_LIMIT as f64,
                    f64::floor(original_limit * BACKOFF_RATIO),
                ))
            }),
        }
    }
}

impl<B> Permit<B>
where
    B: Behavior,
{
    pub fn on_response<T>(&mut self, response: &Result<Response<T>, Error>) {
        self.mode = match response {
            Ok(response) => B::on_success(response),
            Err(e) => B::on_failure(e),
        };
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::FutureExt;

    fn host_limiter() -> Arc<CiadConcurrencyLimiter<HostLevel>> {
        CiadConcurrencyLimiter::new()
    }

    #[tokio::test]
    async fn acquire_returns_permits_while_limit_not_reached() {
        let limiter = host_limiter();

        let max = limiter.limit.load(Ordering::SeqCst);
        let mut permits = vec![];
        for _ in 0..max as usize {
            let permit = limiter.clone().acquire().await;
            permits.push(permit);
        }

        // at limit, cannot acquire permit
        assert!(limiter.clone().acquire().now_or_never().is_none());

        // release one permit, can acquire new permit
        permits.pop().unwrap();
        limiter.acquire().await;
    }

    #[tokio::test]
    #[allow(clippy::float_cmp)]
    async fn ignore_does_not_change_limits() {
        let limiter = host_limiter();

        let max = limiter.limit.load(Ordering::SeqCst);
        let permit = limiter.clone().acquire().await;
        drop(permit);
        assert_eq!(limiter.limit.load(Ordering::SeqCst), max);
    }

    #[tokio::test]
    #[allow(clippy::float_cmp)]
    async fn dropped_reduces_limit() {
        let limiter = host_limiter();

        let max = limiter.limit.load(Ordering::SeqCst);
        let mut permit = limiter.clone().acquire().await;
        permit.mode = Mode::Dropped;
        drop(permit);
        assert_eq!(limiter.limit.load(Ordering::SeqCst), max * 0.9);
    }

    #[tokio::test]
    #[allow(clippy::float_cmp)]
    async fn success_increases_limit_if_sufficient_requests_are_in_flight() {
        let limiter = host_limiter();

        let max = limiter.limit.load(Ordering::SeqCst);
        let mut permits = vec![];
        for _ in 0..(max * 0.9) as usize {
            let permit = limiter.clone().acquire().await;
            permits.push(permit);
        }

        let mut permit = limiter.clone().acquire().await;
        permit.mode = Mode::Success;
        drop(permit);
        let new_max = limiter.limit.load(Ordering::SeqCst);
        assert!(new_max - max > 0. && new_max - max <= 1.);

        drop(permits);
        let new_new_max = limiter.limit.load(Ordering::SeqCst);
        assert_eq!(new_max, new_new_max);
    }
}
