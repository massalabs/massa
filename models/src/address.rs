// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::api::{LedgerInfo, RollsInfo};
use crate::hhasher::{HHashMap, HHashSet, PreHashed};
use crate::ModelsError;
use massa_hash::hash::{Hash, HASH_SIZE_BYTES};
use serde::{Deserialize, Serialize};
use signature::PublicKey;
use std::str::FromStr;

pub const ADDRESS_SIZE_BYTES: usize = HASH_SIZE_BYTES;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Address(Hash);

pub type AddressHashMap<T> = HHashMap<Address, T>;
pub type AddressHashSet = HHashSet<Address>;

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0.to_bs58_check())
    }
}

impl FromStr for Address {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Address(Hash::from_str(s)?))
    }
}

impl PreHashed for Address {}

impl Address {
    /// Gets the associated tread. Depends on the thread_count
    pub fn get_thread(&self, thread_count: u8) -> u8 {
        (self.to_bytes()[0])
            .checked_shr(8 - thread_count.trailing_zeros())
            .unwrap_or(0)
    }

    /// Computes address associated with given public key
    pub fn from_public_key(public_key: &PublicKey) -> Result<Self, ModelsError> {
        Ok(Address(Hash::from(&public_key.to_bytes())))
    }

    /// ## Example
    /// ```rust
    /// # use signature::{PublicKey, PrivateKey, Signature,
    /// #       generate_random_private_key, derive_public_key};
    /// # use massa_hash::hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use models::Address;
    /// # let private_key = generate_random_private_key();
    /// # let public_key = derive_public_key(&private_key);
    /// # let address = Address::from_public_key(&public_key).unwrap();
    /// let bytes = address.to_bytes();
    /// let res_addr = Address::from_bytes(&bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn to_bytes(&self) -> [u8; ADDRESS_SIZE_BYTES] {
        self.0.to_bytes()
    }

    /// ## Example
    /// ```rust
    /// # use signature::{PublicKey, PrivateKey, Signature,
    /// #       generate_random_private_key, derive_public_key};
    /// # use massa_hash::hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use models::Address;
    /// # let private_key = generate_random_private_key();
    /// # let public_key = derive_public_key(&private_key);
    /// # let address = Address::from_public_key(&public_key).unwrap();
    /// let bytes = address.clone().into_bytes();
    /// let res_addr = Address::from_bytes(&bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn into_bytes(self) -> [u8; ADDRESS_SIZE_BYTES] {
        self.0.into_bytes()
    }

    /// ## Example
    /// ```rust
    /// # use signature::{PublicKey, PrivateKey, Signature,
    /// #       generate_random_private_key, derive_public_key};
    /// # use massa_hash::hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use models::Address;
    /// # let private_key = generate_random_private_key();
    /// # let public_key = derive_public_key(&private_key);
    /// # let address = Address::from_public_key(&public_key).unwrap();
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
    /// # use signature::{PublicKey, PrivateKey, Signature,
    /// #       generate_random_private_key, derive_public_key};
    /// # use massa_hash::hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use models::Address;
    /// # let private_key = generate_random_private_key();
    /// # let public_key = derive_public_key(&private_key);
    /// # let address = Address::from_public_key(&public_key).unwrap();
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
    /// # use signature::{PublicKey, PrivateKey, Signature,
    /// #       generate_random_private_key, derive_public_key};
    /// # use massa_hash::hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use models::Address;
    /// # let private_key = generate_random_private_key();
    /// # let public_key = derive_public_key(&private_key);
    /// # let address = Address::from_public_key(&public_key).unwrap();
    /// let ser = address.to_bs58_check();
    /// let res_addr = Address::from_bs58_check(&ser).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn to_bs58_check(&self) -> String {
        self.0.to_bs58_check()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Addresses {
    pub addrs: AddressHashSet,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddressCycleProductionStats {
    pub cycle: u64,
    pub is_final: bool,
    pub ok_count: u64,
    pub nok_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddressState {
    pub ledger_info: LedgerInfo,
    pub rolls: RollsInfo,
    pub production_stats: Vec<AddressCycleProductionStats>,
}
