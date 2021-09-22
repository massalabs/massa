/// Unsafe but fast hasher that is used when the thing to hash is itself already a hash
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::hash::{BuildHasherDefault, Hasher};
use std::marker::PhantomData;

pub struct HHasher(u64);

impl Default for HHasher {
    #[inline]
    fn default() -> HHasher {
        HHasher(0)
    }
}

impl Hasher for HHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        // assumes bytes.len() is at least 8, otherwise panics
        self.0 = u64::from_ne_bytes(
            bytes[bytes.len().checked_sub(8).unwrap()..]
                .try_into()
                .unwrap(),
        );
    }
}

pub trait PreHashed {
    /// Implement this and return true,
    /// only if you are sure that the type
    /// can be safely used in the context of hashing.
    fn was_safely_pre_hashed(&self) -> bool;
}

#[derive(Default)]
pub struct HHasher2<T: PreHashed + Default> {
    source: PhantomData<T>,
    hash: u64,
}

impl<T: PreHashed + Default> Hasher for HHasher2<T> {
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }

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

pub type BuildHHasher = BuildHasherDefault<HHasher>;

pub type HHashMap<K, V> = HashMap<K, V, BuildHHasher>;
pub type HHashSet<T> = HashSet<T, BuildHHasher>;

pub type BuildHHasher2<T> = BuildHasherDefault<HHasher2<T>>;

pub type HHashMap2<K, V> = HashMap<K, V, BuildHHasher2<K>>;
pub type HHashSet2<K, T> = HashSet<K, BuildHHasher2<T>>;
