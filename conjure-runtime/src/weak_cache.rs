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
    capacity: usize,
    next_id: usize,
}

pub struct WeakCache<K, V>
where
    K: Eq + Hash,
{
    inner: Arc<Mutex<Inner<K, V>>>,
}

impl<K, V> Clone for WeakCache<K, V>
where
    K: Eq + Hash,
{
    #[inline]
    fn clone(&self) -> Self {
        WeakCache {
            inner: self.inner.clone(),
        }
    }
}

impl<K, V> WeakCache<K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn new(capacity: usize) -> Self {
        WeakCache {
            inner: Arc::new(Mutex::new(Inner {
                cache: LinkedHashMap::new(),
                capacity,
                next_id: 0,
            })),
        }
    }

    pub fn get<T>(
        &self,
        seed: &T,
        get_key: impl FnOnce(&T) -> &K,
        make_value: impl FnOnce(&T) -> Result<V, Error>,
    ) -> Result<Arc<Cached<K, V>>, Error> {
        let key = get_key(seed);

        let mut inner = self.inner.lock();
        if let Some(state) = inner.cache.get_refresh(key).and_then(|w| w.value.upgrade()) {
            return Ok(state.clone());
        }

        let key = Arc::new(key.clone());
        let value = make_value(seed)?;
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

        while inner.cache.len() > inner.capacity {
            inner.cache.pop_front();
        }

        Ok(value)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn cleanup_after_drop() {
        let cache = WeakCache::new(2);
        let value1 = cache.get(&(), |_| &0, |_| Ok(1)).unwrap();
        let value2 = cache.get(&(), |_| &0, |_| panic!()).unwrap();
        assert_eq!(**value1, 1);
        assert_eq!(**value2, 1);

        drop((value1, value2));
        assert_eq!(cache.inner.lock().cache.len(), 0);

        let value3 = cache.get(&(), |_| &0, |_| Ok(2)).unwrap();
        assert_eq!(**value3, 2);
    }

    #[test]
    fn lru_eviction() {
        let cache = WeakCache::new(2);
        let _value1 = cache.get(&(), |_| &0, |_| Ok(1)).unwrap();
        let _value2 = cache.get(&(), |_| &1, |_| Ok(2)).unwrap();

        // insert 2, evict 0
        let _value3 = cache.get(&(), |_| &2, |_| Ok(3)).unwrap();

        // insert 0, evict 1
        let value4 = cache.get(&(), |_| &0, |_| Ok(4)).unwrap();
        assert_eq!(**value4, 4);

        // refresh 2
        cache.get(&(), |_| &2, |_| panic!()).unwrap();

        // insert 3, evict 0
        cache.get(&(), |_| &3, |_| Ok(5)).unwrap();

        // insert 0, evict 2
        let value5 = cache.get(&(), |_| &0, |_| Ok(6)).unwrap();
        assert_eq!(**value5, 6);
    }
}
