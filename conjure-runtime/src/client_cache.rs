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
    hash::Hash,
    ops::Deref,
    sync::{Arc, Weak},
};

use conjure_error::Error;
use linked_hash_map::LinkedHashMap;
use parking_lot::Mutex;

const MAX_CACHED_CHANNELS: usize = 1_000;

pub struct Cached<K, V>
where
    K: Eq + Hash,
{
    value: V,
    _evictor: Option<CacheEvictor<K, V>>,
}

impl<K, V> Cached<K, V>
where
    K: Eq + Hash,
{
    pub fn uncached(value: V) -> Self {
        Cached {
            value,
            _evictor: None,
        }
    }
}

impl<K, V> Deref for Cached<K, V>
where
    K: Eq + Hash,
{
    type Target = V;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

pub struct CacheEvictor<K, V>
where
    K: Eq + Hash,
{
    inner: Weak<Mutex<Inner<K, V>>>,
    key: Arc<K>,
    id: usize,
}

impl<K, V> Drop for CacheEvictor<K, V>
where
    K: Eq + Hash,
{
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

struct CachedValue<K, V>
where
    K: Eq + Hash,
{
    value: Weak<Cached<K, V>>,
    id: usize,
}

struct Inner<K, V>
where
    K: Eq + Hash,
{
    cache: LinkedHashMap<Arc<K>, CachedValue<K, V>>,
    next_id: usize,
}

pub struct ClientCache<K, V>
where
    K: Eq + Hash,
{
    inner: Arc<Mutex<Inner<K, V>>>,
}

impl<K, V> Clone for ClientCache<K, V>
where
    K: Eq + Hash,
{
    #[inline]
    fn clone(&self) -> Self {
        ClientCache {
            inner: self.inner.clone(),
        }
    }
}

impl<K, V> ClientCache<K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn new() -> Self {
        ClientCache {
            inner: Arc::new(Mutex::new(Inner {
                cache: LinkedHashMap::new(),
                next_id: 0,
            })),
        }
    }

    pub fn get<T>(
        &self,
        builder: &T,
        get_key: impl FnOnce(&T) -> &K,
        make_value: impl FnOnce(&T) -> Result<V, Error>,
    ) -> Result<Arc<Cached<K, V>>, Error> {
        let key = get_key(builder);

        let mut inner = self.inner.lock();
        if let Some(state) = inner.cache.get_refresh(key).and_then(|w| w.value.upgrade()) {
            return Ok(state.clone());
        }

        let key = Arc::new(key.clone());
        let value = make_value(builder)?;
        let id = inner.next_id;
        inner.next_id += 1;
        let value = Arc::new(Cached {
            value,
            _evictor: Some(CacheEvictor {
                inner: Arc::downgrade(&self.inner),
                key: key.clone(),
                id,
            }),
        });
        let cached_value = CachedValue {
            value: Arc::downgrade(&value),
            id,
        };
        inner.cache.insert(key, cached_value);

        while inner.cache.len() > MAX_CACHED_CHANNELS {
            inner.cache.pop_front();
        }

        Ok(value)
    }
}
