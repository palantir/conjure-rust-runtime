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
use crate::service::node::limiter::deficit_semaphore::DeficitSemaphore;
use crate::util::atomic_f64::AtomicF64;
use futures::ready;
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

mod deficit_semaphore;

const INITIAL_LIMIT: usize = 20;
const BACKOFF_RATIO: f64 = 0.9;
const MIN_LIMIT: usize = 1;
const MAX_LIMIT: usize = 1_000_000;

pub struct ConcurrencyLimiter {
    semaphore: Arc<DeficitSemaphore>,
    limit: AtomicF64,
    in_flight: AtomicUsize,
}

impl ConcurrencyLimiter {
    pub fn new() -> Arc<Self> {
        Arc::new(ConcurrencyLimiter {
            semaphore: DeficitSemaphore::new(INITIAL_LIMIT),
            limit: AtomicF64::new(INITIAL_LIMIT as f64),
            in_flight: AtomicUsize::new(0),
        })
    }

    pub fn acquire(self: Arc<Self>) -> Acquire {
        Acquire {
            future: self.semaphore.clone().acquire(),
            limiter: self,
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
                }
                Err(limit) => old_limit = limit,
            }
        }
    }
}

#[pin_project]
pub struct Acquire {
    #[pin]
    future: deficit_semaphore::Acquire,
    limiter: Arc<ConcurrencyLimiter>,
}

impl Future for Acquire {
    type Output = Permit;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let permit = ready!(this.future.poll(cx));

        Poll::Ready(Permit {
            limiter: this.limiter.clone(),
            in_flight_snapshot: this.limiter.in_flight.fetch_add(1, Ordering::SeqCst) + 1,
            mode: Mode::Ignore,
            _permit: permit,
        })
    }
}

pub enum Mode {
    Ignore,
    Success,
    Dropped,
}

pub struct Permit {
    limiter: Arc<ConcurrencyLimiter>,
    in_flight_snapshot: usize,
    mode: Mode,
    _permit: deficit_semaphore::Permit,
}

impl Drop for Permit {
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

impl Permit {
    pub fn success(&mut self) {
        self.mode = Mode::Success;
    }

    pub fn dropped(&mut self) {
        self.mode = Mode::Dropped;
    }
}
