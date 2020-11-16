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
use crate::util::atomic_f64::AtomicF64;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::{Duration, Instant};

const DECAYS_PER_HALF_LIFE: u64 = 10;

fn decay_factor() -> f64 {
    0.5f64.powf(1. / DECAYS_PER_HALF_LIFE as f64)
}

pub struct CoarseExponentialDecayReservoir {
    value: AtomicF64,
    start: Instant,
    last_decay_nanos: AtomicU64,
    decay_interval_nanos: u64,
}

impl CoarseExponentialDecayReservoir {
    pub fn new(half_life: Duration) -> CoarseExponentialDecayReservoir {
        CoarseExponentialDecayReservoir {
            value: AtomicF64::new(0.),
            start: Instant::now(),
            last_decay_nanos: AtomicU64::new(0),
            decay_interval_nanos: half_life.as_nanos() as u64 / DECAYS_PER_HALF_LIFE,
        }
    }

    pub fn update(&self, updates: f64) {
        self.decay_if_necessary();
        self.value.fetch_add(updates, Ordering::SeqCst);
    }

    pub fn get(&self) -> f64 {
        self.decay_if_necessary();
        self.value.load(Ordering::SeqCst)
    }

    fn decay_if_necessary(&self) {
        let now_nanos = self.start.elapsed().as_nanos() as u64;
        let last_decay_nanos_snapshot = self.last_decay_nanos.load(Ordering::SeqCst);
        let nanos_since_last_decay = now_nanos - last_decay_nanos_snapshot;
        let decays = nanos_since_last_decay / self.decay_interval_nanos;

        if decays == 0 {
            return;
        }

        if self
            .last_decay_nanos
            .compare_exchange(
                last_decay_nanos_snapshot,
                last_decay_nanos_snapshot + decays * self.decay_interval_nanos,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            return;
        }

        self.decay(decays as i32);
    }

    fn decay(&self, decays: i32) {
        let _ = self
            .value
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
                Some(old * decay_factor().powi(decays))
            });
    }
}
