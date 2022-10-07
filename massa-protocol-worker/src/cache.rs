// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Simple unreliable, but fast cache implementations

use massa_models::{
    prehash::{CapacityAllocator, PreHashMap, PreHashSet, PreHashed},
    wrapped::Id,
};
use std::collections::{hash_map, VecDeque};

/// Structure holding a finite capacity cache set that is entirely cleared when full.
/// Supports efficient deletion.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct HashCacheSet<K: PreHashed + std::hash::Hash + Id + Clone + Copy + PartialEq + Eq> {
    /// Cache capacity
    capacity: usize,
    /// Container
    container: PreHashSet<K>,
}

#[allow(dead_code)]
impl<K: PreHashed + std::hash::Hash + Id + Clone + Copy + PartialEq + Eq> HashCacheSet<K> {
    /// Create a new cache instance
    pub fn new(capacity: usize) -> Self {
        HashCacheSet {
            capacity,
            container: PreHashSet::with_capacity(capacity.saturating_add(1)),
        }
    }

    /// Check if a key is present in the cache
    pub fn contains(&self, key: &K) -> bool {
        self.container.contains(key)
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.container.clear();
    }

    /// Insert a new key. Returns `true` if the element was not in cache before.
    pub fn insert(&mut self, key: K) -> bool {
        if self.capacity == 0 {
            return true;
        }

        if !self.container.insert(key) {
            // element was already in cache
            return false;
        }

        if self.container.len() > self.capacity {
            // over capacity: clear and re-insert
            self.clear();
            self.container.insert(key);
        }

        true
    }

    /// Remove an element from cache.
    /// Returns `true` if the element was present.
    pub fn remove(&mut self, key: &K) -> bool {
        self.container.remove(key)
    }

    /// Extend with new elements
    pub fn extend<I: IntoIterator<Item = K>>(&mut self, iter: I) {
        iter.into_iter().for_each(|k| {
            self.insert(k);
        });
    }
}

/// Structure holding a finite capacity cache map that is entirely cleared when full.
/// Supports efficient deletion.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct HashCacheMap<K: PreHashed + std::hash::Hash + Id + Clone + Copy + PartialEq + Eq, V> {
    /// Cache capacity
    capacity: usize,
    /// Container
    container: PreHashMap<K, V>,
}

#[allow(dead_code)]
impl<K: PreHashed + std::hash::Hash + Id + Clone + Copy + PartialEq + Eq, V> HashCacheMap<K, V> {
    /// Create a new cache instance
    pub fn new(capacity: usize) -> Self {
        HashCacheMap {
            capacity,
            container: PreHashMap::with_capacity(capacity),
        }
    }

    /// Check if a key is present in the cache
    pub fn contains_key(&self, key: &K) -> bool {
        self.container.contains_key(key)
    }

    /// Get an immutable reference to an element, `None` if not found
    pub fn get(&self, key: &K) -> Option<&V> {
        self.container.get(key)
    }

    /// Get a mutable reference to an element, `None` if not found
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.container.get_mut(key)
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.container.clear();
    }

    /// Insert a new item. Returns the previous value if any.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if self.capacity == 0 {
            return None;
        }
        let len = self.container.len();
        match self.container.entry(key) {
            hash_map::Entry::Occupied(mut occ) => {
                // item was already present
                return Some(occ.insert(value));
            }
            hash_map::Entry::Vacant(vac) => {
                // item was absent
                if len >= self.capacity {
                    // the container was full: clear then insert
                    self.container.clear();
                    self.container.insert(key, value);
                    return None;
                } else {
                    // container not full
                    vac.insert(value);
                    return None;
                }
            }
        }
    }

    /// Remove an element from cache.
    /// Returns the removed value if any.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.container.remove(key)
    }

    /// Extend with new elements
    pub fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        iter.into_iter().for_each(|(k, v)| {
            self.insert(k, v);
        });
    }
}

/// Structure holding a finite capacity cache set that deletes the oldest item when full.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct LinearHashCacheSet<K: PreHashed + std::hash::Hash + Id + Clone + Copy + PartialEq + Eq> {
    /// Cache capacity
    capacity: usize,
    /// Container
    container: PreHashSet<K>,
    /// Queue
    queue: VecDeque<K>,
}

#[allow(dead_code)]
impl<K: PreHashed + std::hash::Hash + Id + Clone + Copy + PartialEq + Eq> LinearHashCacheSet<K> {
    /// Create a new cache instance
    pub fn new(capacity: usize) -> Self {
        LinearHashCacheSet {
            capacity,
            container: PreHashSet::with_capacity(capacity.saturating_add(1)),
            queue: VecDeque::with_capacity(capacity.saturating_add(1)),
        }
    }

    /// Check if a key is present in the cache
    pub fn contains(&self, key: &K) -> bool {
        self.container.contains(key)
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.container.clear();
        self.queue.clear();
    }

    /// Tries to insert a new key, does not do anything (and returns `false`) if the element already exists.
    pub fn try_insert(&mut self, key: K) -> bool {
        if self.capacity == 0 {
            return true;
        }

        if !self.container.insert(key) {
            // element was already in cache
            return false;
        }

        // add to queue
        self.queue.push_back(key);

        while self.container.len() > self.capacity {
            // over capacity: remove the oldest elements
            self.container.remove(&self.queue.pop_front().unwrap());
        }

        true
    }

    /// Extend with new elements. Items that are already in cache are ignored.
    pub fn try_extend<I: IntoIterator<Item = K>>(&mut self, iter: I) {
        iter.into_iter().for_each(|k| {
            self.try_insert(k);
        });
    }
}

/// Structure holding a finite capacity cache map that deletes the oldest item when full.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct LinearHashCacheMap<
    K: PreHashed + std::hash::Hash + Id + Clone + Copy + PartialEq + Eq,
    V,
> {
    /// Cache capacity
    capacity: usize,
    /// Container
    container: PreHashMap<K, V>,
    /// Queue
    queue: VecDeque<K>,
}

#[allow(dead_code)]
impl<K: PreHashed + std::hash::Hash + Id + Clone + Copy + PartialEq + Eq, V>
    LinearHashCacheMap<K, V>
{
    /// Create a new cache instance
    pub fn new(capacity: usize) -> Self {
        LinearHashCacheMap {
            capacity,
            container: PreHashMap::with_capacity(capacity.saturating_add(1)),
            queue: VecDeque::with_capacity(capacity.saturating_add(1)),
        }
    }

    /// Check if a key is present in the cache
    pub fn contains_key(&self, key: &K) -> bool {
        self.container.contains_key(key)
    }

    /// Get an immutable reference to an element, `None` if not found
    pub fn get(&self, key: &K) -> Option<&V> {
        self.container.get(key)
    }

    /// Get a mutable reference to an element, `None` if not found
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.container.get_mut(key)
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.container.clear();
        self.queue.clear();
    }

    /// Tries to insert a new item. Does nothing and returns `false` if the item was already present.
    pub fn insert(&mut self, key: K, value: V) -> bool {
        if self.capacity == 0 {
            return true;
        }

        match self.container.try_insert(key, value) {
            Err(_) => {
                // item was already present
                false
            }
            Ok(_) => {
                // item was absent

                // add to queue
                self.queue.push_back(key);

                // prune container
                while self.container.len() > self.capacity {
                    self.container.remove(&self.queue.pop_front().unwrap());
                }

                true
            }
        }
    }

    /// Extend with new elements
    pub fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        iter.into_iter().for_each(|(k, v)| {
            self.insert(k, v);
        });
    }
}
