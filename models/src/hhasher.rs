/// Unsafe but fast hasher that is used when the thing to hash is itself already a hash
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::hash::{BuildHasherDefault, Hasher};

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

pub type BuildHHasher = BuildHasherDefault<HHasher>;

pub type HHashMap<K, V> = HashMap<K, V, BuildHHasher>;
pub type HHashSet<T> = HashSet<T, BuildHHasher>;
