// Copyright (c) 2021 MASSA LABS <info@massa.net>

use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::hash::{BuildHasherDefault, Hasher};
use std::marker::PhantomData;

/// A trait indicating that its carrier is already a hash with at least 64 bits
/// and doesn't need to be re-hashed for hash-table purposes
pub trait PreHashed {}

/// A Hasher for PreHashed keys that is faster because it avoids re-hashing hashes but simply truncates them.
/// Note: when truncating, it takes the last 8 bytes of the key instead of the first 8 bytes.
/// This is done to circumvent biases induced by first-byte manipulations in addresses related to the thread assignment process
pub struct HHasher<T: PreHashed> {
    source: PhantomData<T>,
    hash: u64,
}

/// Default implementation for HHasher (zero hash)
impl<T: PreHashed> Default for HHasher<T> {
    fn default() -> Self {
        HHasher {
            source: Default::default(),
            hash: Default::default(),
        }
    }
}

/// Hasher implementation for HHasher
impl<T: PreHashed> Hasher for HHasher<T> {
    /// finish the hashing process and return the truncated u64 hash
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }

    /// write the bytes of a PreHashed key into the HHasher
    /// Panics if bytes.len() is strictly lower than 8
    /// Note: the truncated u64 is completely overwritten by the last 8 items of "bytes" at every call
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

/// BuildHasherDefault specialization for HHasher
pub type BuildHHasher<T> = BuildHasherDefault<HHasher<T>>;

/// HashMap specialization for PreHashed keys
/// This hashmap is about 2x faster than the default HashMap
pub type HHashMap<K, V> = HashMap<K, V, BuildHHasher<K>>;

/// HashSet specialization for PreHashed keys
/// This hashset is about 2x faster than the default HashSet
pub type HHashSet<T> = HashSet<T, BuildHHasher<T>>;
