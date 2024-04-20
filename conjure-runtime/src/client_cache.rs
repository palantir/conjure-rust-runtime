// Copyright 2024 Palantir Technologies, Inc.
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

use std::sync::{Arc, Weak};

use conjure_error::Error;
use linked_hash_map::LinkedHashMap;
use parking_lot::Mutex;

use crate::{builder::CachedConfig, raw::DefaultRawClient, Builder, ClientState};

const MAX_CACHED_CHANNELS: usize = 1_000;

struct CachedState {
    state: Weak<ClientState<DefaultRawClient>>,
    id: usize,
}

struct Inner {
    cache: LinkedHashMap<Arc<CachedConfig>, CachedState>,
    next_id: usize,
}

#[derive(Clone)]
pub struct ClientCache {
    inner: Arc<Mutex<Inner>>,
}

impl ClientCache {
    pub fn new() -> Self {
        ClientCache {
            inner: Arc::new(Mutex::new(Inner {
                cache: LinkedHashMap::new(),
                next_id: 0,
            })),
        }
    }

    pub fn get(&self, builder: &Builder) -> Result<Arc<ClientState<DefaultRawClient>>, Error> {
        let key = builder.cached_config();

        let mut inner = self.inner.lock();
        if let Some(state) = inner.cache.get_refresh(key).and_then(|w| w.state.upgrade()) {
            return Ok(state.clone());
        }

        let key = Arc::new(key.clone());
        let mut state = ClientState::new(builder)?;
        let id = inner.next_id;
        inner.next_id += 1;
        state.evictor = Some(CacheEvictor {
            inner: Arc::downgrade(&self.inner),
            key: key.clone(),
            id,
        });
        let state = Arc::new(state);
        let cached_state = CachedState {
            state: Arc::downgrade(&state),
            id,
        };
        inner.cache.insert(key, cached_state);

        while inner.cache.len() > MAX_CACHED_CHANNELS {
            inner.cache.pop_front();
        }

        Ok(state)
    }
}

pub struct CacheEvictor {
    inner: Weak<Mutex<Inner>>,
    key: Arc<CachedConfig>,
    id: usize,
}

impl Drop for CacheEvictor {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let mut inner = inner.lock();

        if let Some(cached_state) = inner.cache.get(&self.key) {
            if cached_state.id == self.id {
                inner.cache.remove(&self.key);
            }
        }
    }
}
