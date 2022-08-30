//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Unreliable but high-performance hash cache

use crate::prehash::{CapacityAllocator, PreHashMap, PreHashSet, PreHashed};
use std::collections::VecDeque;

/// A cache set structure for hashed keys.
/// Optimized for working "most of the time" and being efficient.
#[derive(Debug, Clone)]
pub struct HashCacheSet<K: std::hash::Hash + PreHashed + PartialEq + Eq + Clone + Copy> {
    /// Set of IDs
    key_set: PreHashSet<K>,
    /// Max len of the cache
    capacity: usize,
}

impl<K: std::hash::Hash + PreHashed + PartialEq + Eq + Clone + Copy> HashCacheSet<K> {
    /// Create a new PreHashCache with a given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            key_set: PreHashSet::with_capacity(capacity.saturating_add(1)),
            capacity,
        }
    }

    /// Check if the cache contains an item
    pub fn contains(&self, key: &K) -> bool {
        self.key_set.contains(key)
    }

    /// Add a key to the cache.
    /// Returns true if the element is new.
    pub fn insert(&mut self, key: K) -> bool {
        let res = self.key_set.insert(key);
        if res {
            if self.key_set.len() > self.capacity {
                self.key_set.clear();
            }
        }
        res
    }

    /// Remove key from cache.
    /// Returns true if it was removed.
    pub fn remove(&mut self, key: &K) -> bool {
        self.key_set.remove(key)
    }
}

/// A cache map structure for hashed keys.
/// Optimized for working "most of the time" and being efficient.
#[derive(Debug, Clone)]
pub struct HashCacheMap<K: std::hash::Hash + PreHashed + PartialEq + Eq + Clone + Copy, V> {
    /// Map of items
    item_map: PreHashMap<K, V>,
    /// Max len of the cache
    capacity: usize,
}

impl<K: std::hash::Hash + PreHashed + PartialEq + Eq + Clone + Copy, V> HashCacheMap<K, V> {
    /// Create a new PreHashCache with a given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            item_map: PreHashMap::with_capacity(capacity.saturating_add(1)),
            capacity,
        }
    }

    /// Check if the cache contains a key
    pub fn contains_key(&self, item: &K) -> bool {
        self.item_map.contains_key(item)
    }

    /// Add a key to the cache.
    /// Returns Some(V) if V was overwritten.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let res = self.item_map.insert(key, value);
        if res.is_none() {
            if self.item_map.len() > self.capacity {
                self.item_map.clear();
            }
        }
        res
    }

    /// Remove key from cache.
    /// Returns  the item if it was removed.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.item_map.remove(key)
    }
}
