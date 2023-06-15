// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Standalone massa wallet
//! Keypair management
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(map_try_insert)]

pub use error::WalletError;

use massa_cipher::{decrypt, encrypt};
use massa_hash::Hash;
use massa_models::address::Address;
use massa_models::composite::PubkeySig;
use massa_models::operation::{Operation, OperationSerializer, SecureShareOperation};
use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_models::secure_share::SecureShareContent;
use massa_signature::{KeyPair, PublicKey};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod error;

/// Contains the keypairs created in the wallet.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Wallet {
    /// Keypairs and addresses
    pub keys: PreHashMap<Address, KeyPair>,
    /// Path to the file containing the keypairs (encrypted)
    wallet_path: PathBuf,
    /// Password
    password: String,
}

impl Wallet {
    /// Generates a new wallet initialized with the provided file content
    pub fn new(path: PathBuf, password: String) -> Result<Wallet, WalletError> {
        if path.is_file() {
            let content = &std::fs::read(&path)?[..];
            let (_version, decrypted_content) = decrypt(&password, content)?;
            let keys =
                serde_json::from_slice::<PreHashMap<Address, KeyPair>>(&decrypted_content[..])?;
            Ok(Wallet {
                keys,
                wallet_path: path,
                password,
            })
        } else {
            let wallet = Wallet {
                keys: PreHashMap::default(),
                wallet_path: path,
                password,
            };
            wallet.save()?;
            Ok(wallet)
        }
    }

    /// Sign arbitrary message with the associated keypair
    /// returns none if the address isn't in the wallet or if an error occurred during the signature
    /// else returns the public key that signed the message and the signature
    pub fn sign_message(&self, address: &Address, msg: Vec<u8>) -> Option<PubkeySig> {
        if let Some(key) = self.keys.get(address) {
            if let Ok(signature) = key.sign(&Hash::compute_from(&msg)) {
                Some(PubkeySig {
                    public_key: key.get_public_key(),
                    signature,
                })
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Adds a list of keypairs to the wallet, returns their addresses.
    /// The wallet file is updated.
    pub fn add_keypairs(&mut self, keys: Vec<KeyPair>) -> Result<Vec<Address>, WalletError> {
        let mut changed = false;
        let mut addrs = Vec::with_capacity(keys.len());
        for key in keys {
            let addr = Address::from_public_key(&key.get_public_key());
            if self.keys.try_insert(addr, key).is_ok() {
                changed = true;
            }
            addrs.push(addr);
        }
        if changed {
            self.save()?;
        }
        Ok(addrs)
    }

    /// Removes wallet entries given a list of addresses. Missing entries are ignored.
    /// The wallet file is updated.
    pub fn remove_addresses(&mut self, addresses: &Vec<Address>) -> Result<(), WalletError> {
        let mut changed = false;
        for address in addresses {
            if self.keys.remove(address).is_some() {
                changed = true;
            }
        }
        if changed {
            self.save()?;
        }
        Ok(())
    }

    /// Finds the keypair associated with given address
    pub fn find_associated_keypair(&self, address: &Address) -> Option<&KeyPair> {
        self.keys.get(address)
    }

    /// Finds the public key associated with given address
    pub fn find_associated_public_key(&self, address: &Address) -> Option<PublicKey> {
        self.keys
            .get(address)
            .map(|keypair| keypair.get_public_key())
    }

    /// Get all addresses in the wallet
    pub fn get_wallet_address_list(&self) -> PreHashSet<Address> {
        self.keys.keys().copied().collect()
    }

    /// Save the wallet in json format in a file
    /// Only the keypair is dumped
    fn save(&self) -> Result<(), WalletError> {
        let ser_keys = serde_json::to_string(&self.keys)?;
        let encrypted_content = encrypt(&self.password, ser_keys.as_bytes())?;
        std::fs::write(&self.wallet_path, encrypted_content)?;
        Ok(())
    }

    /// Export keys and addresses
    pub fn get_full_wallet(&self) -> &PreHashMap<Address, KeyPair> {
        &self.keys
    }

    /// Signs an operation with the keypair corresponding to the given address
    pub fn create_operation(
        &self,
        content: Operation,
        address: Address,
    ) -> Result<SecureShareOperation, WalletError> {
        let sender_keypair = self
            .find_associated_keypair(&address)
            .ok_or_else(|| WalletError::MissingKeyError(address))?;
        Ok(Operation::new_verifiable(content, OperationSerializer::new(), sender_keypair).unwrap())
    }
}

impl std::fmt::Display for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f)?;
        for (addr, keypair) in &self.keys {
            writeln!(f, "Secret key: {}", keypair)?;
            writeln!(f, "Public key: {}", keypair.get_public_key())?;
            writeln!(f, "Address: {}", addr)?;
        }
        Ok(())
    }
}

/// Test utils
#[cfg(feature = "testing")]
pub mod test_exports;
