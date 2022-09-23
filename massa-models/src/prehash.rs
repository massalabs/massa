// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::marker::PhantomData;

impl PreHashed for massa_hash::Hash {}

/// A trait indicating that its carrier is already a hash with at least 64 bits
/// and doesn't need to be re-hashed for hash-table purposes
pub trait PreHashed {}

/// A `Hasher` for `PreHashed` keys that is faster because it avoids re-hashing hashes but simply truncates them.
/// Note: when truncating, it takes the last 8 bytes of the key instead of the first 8 bytes.
/// This is done to circumvent biases induced by first-byte manipulations in addresses related to the thread assignment process
pub struct HashMapper<T: PreHashed> {
    source: PhantomData<T>,
    hash: u64,
}

/// Default implementation for `HashMapper` (zero hash)
impl<T: PreHashed> Default for HashMapper<T> {
    fn default() -> Self {
        HashMapper {
            source: Default::default(),
            hash: Default::default(),
        }
    }
}

/// `Hasher` implementation for `HashMapper`
impl<T: PreHashed> Hasher for HashMapper<T> {
    /// finish the hashing process and return the truncated `u64` hash
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }

    /// write the bytes of a `PreHashed` key into the `HashMapper`
    /// Panics if `bytes.len()` is strictly lower than 8
    /// Note: the truncated `u64` is completely overwritten by the last 8 items of "bytes" at every call
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        // assumes bytes.len() is at least 8, otherwise panics
        self.hash = u64::from_ne_bytes(
            bytes[bytes.len().checked_sub(8).unwrap()..]
                .try_into()
                .unwrap(),
        );
    }
}

/// `BuildHasherDefault` specialization for `HashMapper`
pub type BuildHashMapper<T> = BuildHasherDefault<HashMapper<T>>;

/// `HashMap` specialization for `PreHashed` keys
/// This hashmap is about 2x faster than the default `HashMap`
pub type PreHashMap<K, V> = HashMap<K, V, BuildHashMapper<K>>;

/// `HashSet` specialization for `PreHashed` keys
/// This hashset is about 2x faster than the default `HashSet`
pub type PreHashSet<T> = HashSet<T, BuildHashMapper<T>>;

/// Trait allowing pre-allocations
pub trait CapacityAllocator {
    /// pre-allocate with a given capacity
    fn with_capacity(capacity: usize) -> Self;
}

impl<K: PreHashed, V> CapacityAllocator for PreHashMap<K, V> {
    /// pre-allocate with a given capacity
    fn with_capacity(capacity: usize) -> Self {
        PreHashMap::with_capacity_and_hasher(capacity, BuildHashMapper::default())
    }
}

impl<K: PreHashed> CapacityAllocator for PreHashSet<K> {
    /// pre-allocate with a given capacity
    fn with_capacity(capacity: usize) -> Self {
        PreHashSet::with_capacity_and_hasher(capacity, BuildHashMapper::default())
    }
}
