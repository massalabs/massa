use std::ops::{BitXor, BitXorAssign};

use crate::Hash;

/// Extended Hash
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashXof<const SIZE: usize>(pub [u8; SIZE]);

impl<const SIZE: usize> HashXof<SIZE> {
    /// Truncate the extended hash to a regular hash
    pub fn clip_to_massa_hash(&self) -> Hash {
        Hash::from_bytes(&self.0[..32].try_into().unwrap())
    }

    /// Compute from raw data
    pub fn compute_from(data: &[u8]) -> HashXof<SIZE> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(data);
        let mut hash = [0u8; SIZE];
        let mut output_reader = hasher.finalize_xof();
        output_reader.fill(&mut hash);
        HashXof(hash)
    }

    /// Compute from key and value
    pub fn compute_from_kv(key: &[u8], value: &[u8]) -> HashXof<SIZE> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(key);
        hasher.update(value);
        let mut hash = [0u8; SIZE];
        let mut output_reader = hasher.finalize_xof();
        output_reader.fill(&mut hash);
        HashXof(hash)
    }
}

impl<const SIZE: usize> BitXorAssign for HashXof<SIZE> {
    fn bitxor_assign(&mut self, rhs: Self) {
        *self = *self ^ rhs;
    }
}

impl<const SIZE: usize> BitXor for HashXof<SIZE> {
    type Output = Self;

    fn bitxor(self, other: Self) -> Self {
        let xored = self
            .0
            .iter()
            .zip(other.0.iter())
            .map(|(a, b)| a ^ b)
            .collect::<Vec<u8>>()
            .try_into()
            .unwrap();
        HashXof(xored)
    }
}
