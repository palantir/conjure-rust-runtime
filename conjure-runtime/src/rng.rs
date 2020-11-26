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
use crate::Builder;
use parking_lot::Mutex;
use rand::RngCore;
use rand_pcg::Pcg64;

// One layer of indirection to avoid having to lock around thread_rng when a custom RNG isn't used.
pub enum ConjureRng {
    Thread,
    Deterministic(Mutex<Pcg64>),
}

impl ConjureRng {
    pub fn new<T>(builder: &Builder<T>) -> Self {
        if builder.get_deterministic() {
            // fixed seed from https://docs.rs/rand_pcg/0.2.1/rand_pcg/struct.Lcg128Xsl64.html
            ConjureRng::Deterministic(Mutex::new(Pcg64::new(
                0xcafef00dd15ea5e5,
                0xa02bdbf7bb3c0a7ac28fa16a64abf96,
            )))
        } else {
            ConjureRng::Thread
        }
    }

    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut dyn RngCore) -> R,
    {
        match self {
            ConjureRng::Thread => f(&mut rand::thread_rng()),
            ConjureRng::Deterministic(rng) => f(&mut *rng.lock()),
        }
    }
}
