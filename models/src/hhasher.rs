/// Unsafe but fast hasher that is used when the thing to hash is itself already a hash
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::hash::{BuildHasherDefault, Hasher};
use std::marker::PhantomData;

pub trait PreHashed {}

pub struct HHasher<T: PreHashed> {
    source: PhantomData<T>,
    hash: u64,
}

impl<T: PreHashed> Default for HHasher<T> {
    fn default() -> Self {
        HHasher {
            source: Default::default(),
            hash: Default::default(),
        }
    }
}

impl<T: PreHashed> Hasher for HHasher<T> {
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

pub type BuildHHasher<T> = BuildHasherDefault<HHasher<T>>;
pub type HHashMap<K, V> = HashMap<K, V, BuildHHasher<K>>;
pub type HHashSet<T> = HashSet<T, BuildHHasher<T>>;
