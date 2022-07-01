// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Standalone massa wallet
//! Keypair management
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

pub use error::WalletError;

use massa_cipher::{decrypt, encrypt};
use massa_hash::Hash;
use massa_models::address::Address;
use massa_models::composite::PubkeySig;
use massa_models::operation::OperationSerializer;
use massa_models::prehash::{Map, Set};
use massa_models::wrapped::WrappedContent;
use massa_models::{Operation, WrappedOperation};
use massa_signature::{KeyPair, PublicKey};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod error;

/// Contains the keypairs created in the wallet.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Wallet {
    /// Keypairs and addresses
    pub keys: Map<Address, KeyPair>,
    /// Path to the file containing the keypairs (encrypted)
    pub wallet_path: PathBuf,
    /// Password
    pub password: String,
}

impl Wallet {
    /// Generates a new wallet initialized with the provided file content
    pub fn new(path: PathBuf, password: String) -> Result<Wallet, WalletError> {
        if path.is_file() {
            let content = &std::fs::read(&path)?[..];
            let (_version, decrypted_content) = decrypt(&password, content)?;
            let keys = serde_json::from_slice::<Map<Address, KeyPair>>(&decrypted_content[..])?;
            Ok(Wallet {
                keys,
                wallet_path: path,
                password,
            })
        } else {
            let wallet = Wallet {
                keys: Map::default(),
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
    pub fn sign_message(&self, address: Address, msg: Vec<u8>) -> Option<PubkeySig> {
        if let Some(key) = self.keys.get(&address) {
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

    /// Adds a new keypair to wallet, if it was missing
    /// returns corresponding address
    pub fn add_keypair(&mut self, key: KeyPair) -> Result<Address, WalletError> {
        if !self
            .keys
            .iter()
            .any(|(_, file_key)| file_key.to_bytes() == key.to_bytes())
        {
            let ad = Address::from_public_key(&key.get_public_key());
            self.keys.insert(ad, key);
            self.save()?;
            Ok(ad)
        } else {
            // key already in wallet
            Ok(*self
                .keys
                .iter()
                .find(|(_, file_key)| file_key.to_bytes() == key.to_bytes())
                .unwrap()
                .0)
        }
    }

    /// Remove a wallet entry (keys and address) given the address
    /// The file is overwritten
    pub fn remove_address(&mut self, address: Address) -> Result<(), WalletError> {
        self.keys
            .remove(&address)
            .ok_or(WalletError::MissingKeyError(address))?;
        self.save()
    }

    /// Finds the keypair associated with given address
    pub fn find_associated_keypair(&self, address: Address) -> Option<&KeyPair> {
        self.keys.get(&address)
    }

    /// Finds the public key associated with given address
    pub fn find_associated_public_key(&self, address: Address) -> Option<PublicKey> {
        self.keys
            .get(&address)
            .map(|keypair| keypair.get_public_key())
    }

    /// Get all addresses in the wallet
    pub fn get_wallet_address_list(&self) -> Set<Address> {
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
    pub fn get_full_wallet(&self) -> &Map<Address, KeyPair> {
        &self.keys
    }

    /// Signs an operation with the keypair corresponding to the given address
    pub fn create_operation(
        &self,
        content: Operation,
        address: Address,
    ) -> Result<WrappedOperation, WalletError> {
        let sender_keypair = self
            .find_associated_keypair(address)
            .ok_or(WalletError::MissingKeyError(address))?;
        Ok(Operation::new_wrapped(content, OperationSerializer::new(), sender_keypair).unwrap())
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
