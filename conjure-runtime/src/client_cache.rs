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

use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use conjure_error::Error;
use conjure_runtime_config::{ProxyConfig, SecurityConfig};
use linked_hash_map::LinkedHashMap;
use parking_lot::Mutex;
use url::Url;

use crate::{
    raw::DefaultRawClient, Builder, ClientQos, ClientState, Idempotency, NodeSelectionStrategy,
    ServerQos, ServiceError, UserAgent,
};

const MAX_CACHED_CHANNELS: usize = 1_000;

struct CachedState {
    state: Weak<ClientState<DefaultRawClient>>,
    id: usize,
}

struct Inner {
    cache: LinkedHashMap<Arc<CacheKey>, CachedState>,
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
        let key = Arc::new(CacheKey {
            service: builder.get_service().to_string(),
            user_agent: builder.get_user_agent().clone(),
            uris: builder.get_uris().to_vec(),
            security: builder.get_security().clone(),
            proxy: builder.get_proxy().clone(),
            connect_timeout: builder.get_connect_timeout(),
            read_timeout: builder.get_read_timeout(),
            write_timeout: builder.get_write_timeout(),
            backoff_slot_size: builder.get_backoff_slot_size(),
            max_num_retries: builder.get_max_num_retries(),
            client_qos: builder.get_client_qos(),
            server_qos: builder.get_server_qos(),
            service_error: builder.get_service_error(),
            idempotency: builder.get_idempotency(),
            node_selection_strategy: builder.get_node_selection_strategy(),
            rng_seed: builder.get_rng_seed(),
        });

        let mut inner = self.inner.lock();
        if let Some(state) = inner
            .cache
            .get_refresh(&key)
            .and_then(|w| w.state.upgrade())
        {
            return Ok(state.clone());
        }

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

#[derive(Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    service: String,
    user_agent: UserAgent,
    uris: Vec<Url>,
    security: SecurityConfig,
    proxy: ProxyConfig,
    connect_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
    backoff_slot_size: Duration,
    max_num_retries: u32,
    client_qos: ClientQos,
    server_qos: ServerQos,
    service_error: ServiceError,
    idempotency: Idempotency,
    node_selection_strategy: NodeSelectionStrategy,
    rng_seed: Option<u64>,
}

pub struct CacheEvictor {
    inner: Weak<Mutex<Inner>>,
    key: Arc<CacheKey>,
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
