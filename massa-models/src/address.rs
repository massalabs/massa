// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::prehash::PreHashed;
use crate::{
    api::{LedgerInfo, RollsInfo},
    constants::ADDRESS_SIZE_BYTES,
};
use crate::{DeserializeVarInt, ModelsError, SerializeVarInt};
use massa_hash::Hash;
use massa_signature::PublicKey;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Derived from a public key
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Address(pub Hash);

const ADDRESS_PREFIX: char = 'A';
const ADDRESS_VERSION: u64 = 0;

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // note: might want to allocate the vector with capacity
        // in order to avoid re-allocation
        let mut bytes: Vec<u8> = ADDRESS_VERSION.to_varint_bytes();
        bytes.extend(self.0.to_bytes());
        write!(
            f,
            "{}{}",
            ADDRESS_PREFIX,
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

impl FromStr for Address {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == ADDRESS_PREFIX => {
                let data = chars.collect::<String>();
                let mut decoded_bs58_check = bs58::decode(data)
                    .with_check(None)
                    .into_vec()
                    .map_err(|_| ModelsError::AddressParseError)?;
                let (_version, size) = u64::from_varint_bytes(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::AddressParseError)?;
                decoded_bs58_check.drain(0..size);
                Ok(Address(Hash::from_bytes(
                    &decoded_bs58_check
                        .as_slice()
                        .try_into()
                        .map_err(|_| ModelsError::AddressParseError)?,
                )))
            }
            _ => Err(ModelsError::AddressParseError),
        }
    }
}

#[test]
fn test_address_str_format() {
    use massa_signature::{derive_public_key, generate_random_private_key};
    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key);
    let a = address.to_string();
    let b = Address::from_str(&a).unwrap();
    assert!(address == b);
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
    pub fn to_bytes(&self) -> &[u8; ADDRESS_SIZE_BYTES] {
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
    pub fn from_bytes(data: &[u8; ADDRESS_SIZE_BYTES]) -> Address {
        Address(Hash::from_bytes(data))
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
