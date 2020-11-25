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

// One layer of indirection to avoid having to lock around thread_rng when a custom RNG isn't used.
pub enum ConjureRng {
    Thread,
    Custom(Mutex<Box<dyn RngCore + Sync + Send>>),
}

impl ConjureRng {
    pub fn new<T>(builder: &Builder<T>) -> Self {
        match builder.get_rng_builder() {
            Some(builder) => ConjureRng::Custom(Mutex::new(builder())),
            None => ConjureRng::Thread,
        }
    }

    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut dyn RngCore) -> R,
    {
        match self {
            ConjureRng::Thread => f(&mut rand::thread_rng()),
            ConjureRng::Custom(rng) => f(&mut *rng.lock()),
        }
    }
}
