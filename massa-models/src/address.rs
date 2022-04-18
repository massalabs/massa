// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::prehash::PreHashed;
use crate::ModelsError;
use crate::{
    api::{LedgerInfo, RollsInfo},
    constants::ADDRESS_SIZE_BYTES,
};
use massa_hash::Hash;
use massa_signature::PublicKey;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Derived from a public key
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Address(pub Hash);
const ADDRESS_STRING_PREFIX: &str = "ADR";

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", ADDRESS_STRING_PREFIX, self.0.to_bs58_check())
        } else {
            write!(f, "{}", self.0.to_bs58_check())
        }
    }
}

impl FromStr for Address {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                Ok(Address(Hash::from_str(s)?))
            } else if v[0] != ADDRESS_STRING_PREFIX {
                Err(ModelsError::WrongPrefix(
                    ADDRESS_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                Ok(Address(Hash::from_str(v[1])?))
            }
        } else {
            Ok(Address(Hash::from_str(s)?))
        }
    }
}

impl PreHashed for Address {}

impl Address {
    /// Gets the associated thread. Depends on the `thread_count`
    pub fn get_thread(&self, thread_count: u8) -> u8 {
        (self.to_bytes()[0])
            .checked_shr(8 - thread_count.trailing_zeros())
            .unwrap_or(0)
    }

    /// Computes address associated with given public key
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Address(Hash::compute_from(&public_key.to_bytes()))
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, PrivateKey, Signature,
    /// #       generate_random_private_key, derive_public_key};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::Address;
    /// # let private_key = generate_random_private_key();
    /// # let public_key = derive_public_key(&private_key);
    /// # let address = Address::from_public_key(&public_key);
    /// let bytes = address.to_bytes();
    /// let res_addr = Address::from_bytes(&bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn to_bytes(&self) -> [u8; ADDRESS_SIZE_BYTES] {
        self.0.to_bytes()
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, PrivateKey, Signature,
    /// #       generate_random_private_key, derive_public_key};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::Address;
    /// # let private_key = generate_random_private_key();
    /// # let public_key = derive_public_key(&private_key);
    /// # let address = Address::from_public_key(&public_key);
    /// let bytes = address.clone().into_bytes();
    /// let res_addr = Address::from_bytes(&bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn into_bytes(self) -> [u8; ADDRESS_SIZE_BYTES] {
        self.0.into_bytes()
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, PrivateKey, Signature,
    /// #       generate_random_private_key, derive_public_key};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::Address;
    /// # let private_key = generate_random_private_key();
    /// # let public_key = derive_public_key(&private_key);
    /// # let address = Address::from_public_key(&public_key);
    /// let bytes = address.to_bytes();
    /// let res_addr = Address::from_bytes(&bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn from_bytes(data: &[u8; ADDRESS_SIZE_BYTES]) -> Result<Address, ModelsError> {
        Ok(Address(
            Hash::from_bytes(data).map_err(|_| ModelsError::HashError)?,
        ))
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, PrivateKey, Signature,
    /// #       generate_random_private_key, derive_public_key};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::Address;
    /// # let private_key = generate_random_private_key();
    /// # let public_key = derive_public_key(&private_key);
    /// # let address = Address::from_public_key(&public_key);
    /// let ser = address.to_bs58_check();
    /// let res_addr = Address::from_bs58_check(&ser).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<Address, ModelsError> {
        Ok(Address(
            Hash::from_bs58_check(data).map_err(|_| ModelsError::HashError)?,
        ))
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, PrivateKey, Signature,
    /// #       generate_random_private_key, derive_public_key};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::Address;
    /// # let private_key = generate_random_private_key();
    /// # let public_key = derive_public_key(&private_key);
    /// # let address = Address::from_public_key(&public_key);
    /// let ser = address.to_bs58_check();
    /// let res_addr = Address::from_bs58_check(&ser).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn to_bs58_check(&self) -> String {
        self.0.to_bs58_check()
    }
}

/// Production stats for a given address during a given cycle
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddressCycleProductionStats {
    /// cycle number
    pub cycle: u64,
    /// true if that cycle is final
    pub is_final: bool,
    /// `ok_count` blocks were created by this address during that cycle
    pub ok_count: u64,
    /// `ok_count` blocks were missed by this address during that cycle
    pub nok_count: u64,
}

/// Address state as know by consensus
/// Used to answer to API
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddressState {
    /// Parallel balance information
    pub ledger_info: LedgerInfo,
    /// Rolls information
    pub rolls: RollsInfo,
    /// stats for still in memory cycles
    pub production_stats: Vec<AddressCycleProductionStats>,
}
