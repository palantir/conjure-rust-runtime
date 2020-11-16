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
use std::sync::atomic::{AtomicU64, Ordering};

pub struct AtomicF64(AtomicU64);

impl AtomicF64 {
    pub fn new(value: f64) -> AtomicF64 {
        AtomicF64(AtomicU64::new(value.to_bits()))
    }

    pub fn load(&self, ordering: Ordering) -> f64 {
        let v = self.0.load(ordering);
        f64::from_bits(v)
    }

    pub fn fetch_update<F>(
        &self,
        set_order: Ordering,
        fetch_order: Ordering,
        mut f: F,
    ) -> Result<f64, f64>
    where
        F: FnMut(f64) -> Option<f64>,
    {
        self.0
            .fetch_update(set_order, fetch_order, |v| {
                f(f64::from_bits(v)).map(f64::to_bits)
            })
            .map(f64::from_bits)
            .map_err(f64::from_bits)
    }

    pub fn fetch_add(&self, n: f64, ordering: Ordering) -> f64 {
        self.fetch_update(ordering, ordering, |old| Some(old + n))
            .unwrap()
    }

    pub fn compare_exchange(
        &self,
        current: f64,
        new: f64,
        success: Ordering,
        failure: Ordering,
    ) -> Result<f64, f64> {
        self.0
            .compare_exchange(current.to_bits(), new.to_bits(), success, failure)
            .map(f64::from_bits)
            .map_err(f64::from_bits)
    }
}
