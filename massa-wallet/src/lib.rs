// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Standalone massa wallet
//! private key management
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

pub use error::WalletError;

use massa_cipher::{decrypt, encrypt};
use massa_hash::Hash;
use massa_models::address::Address;
use massa_models::composite::PubkeySig;
use massa_models::prehash::{Map, Set};
use massa_models::signed::Signed;
use massa_models::{Operation, SignedOperation};
use massa_signature::{derive_public_key, sign, PrivateKey, PublicKey};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod error;

/// Contains the private keys created in the wallet.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Wallet {
    /// Private keys and derived public keys and addresses
    pub keys: Map<Address, (PublicKey, PrivateKey)>,
    /// Path to the file containing the private keys (not encrypted)
    pub wallet_path: PathBuf,
    /// Password
    pub password: String,
}

impl Wallet {
    /// Generates a new wallet initialized with the provided file content
    pub fn new(path: PathBuf, password: String) -> Result<Wallet, WalletError> {
        if path.is_file() {
            let content = &std::fs::read(&path)?[..];
            let decrypted_content = decrypt(&password, content)?;
            let priv_keys = serde_json::from_slice::<Vec<PrivateKey>>(&decrypted_content[..])?;
            let keys: Result<Map<Address, (PublicKey, PrivateKey)>, WalletError> = priv_keys
                .iter()
                .map(|priv_key| {
                    let pub_key = derive_public_key(priv_key);
                    Ok((Address::from_public_key(&pub_key), (pub_key, *priv_key)))
                })
                .collect();
            Ok(Wallet {
                keys: keys?,
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

    /// Sign arbitrary message with the associated private key
    /// returns none if the address isn't in the wallet or if an error occurred during the signature
    /// else returns the public key that signed the message and the signature
    pub fn sign_message(&self, address: Address, msg: Vec<u8>) -> Option<PubkeySig> {
        if let Some((public_key, key)) = self.keys.get(&address) {
            if let Ok(signature) = sign(&Hash::compute_from(&msg), key) {
                Some(PubkeySig {
                    public_key: *public_key,
                    signature,
                })
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Adds a new private key to wallet, if it was missing
    /// returns corresponding address
    pub fn add_private_key(&mut self, key: PrivateKey) -> Result<Address, WalletError> {
        if !self.keys.iter().any(|(_, (_, file_key))| file_key == &key) {
            let pub_key = derive_public_key(&key);
            let ad = Address::from_public_key(&pub_key);
            self.keys.insert(ad, (pub_key, key));
            self.save()?;
            Ok(ad)
        } else {
            // key already in wallet
            Ok(*self
                .keys
                .iter()
                .find(|(_, (_, file_key))| file_key == &key)
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

    /// Finds the private key associated with given address
    pub fn find_associated_private_key(&self, address: Address) -> Option<&PrivateKey> {
        self.keys.get(&address).map(|(_pub_key, priv_key)| priv_key)
    }

    /// Finds the public key associated with given address
    pub fn find_associated_public_key(&self, address: Address) -> Option<&PublicKey> {
        self.keys.get(&address).map(|(pub_key, _priv_key)| pub_key)
    }

    /// Get all addresses in the wallet
    pub fn get_wallet_address_list(&self) -> Set<Address> {
        self.keys.keys().copied().collect()
    }

    /// Save the wallet in json format in a file
    /// Only the private keys are dumped
    fn save(&self) -> Result<(), WalletError> {
        let ser_keys = serde_json::to_string(
            &self
                .keys
                .iter()
                .map(|(_, (_, private_key))| *private_key)
                .collect::<Vec<_>>(),
        )?;
        let encrypted_content = encrypt(&self.password, ser_keys.as_bytes())?;
        std::fs::write(&self.wallet_path, encrypted_content)?;
        Ok(())
    }

    /// Export keys and addresses
    pub fn get_full_wallet(&self) -> &Map<Address, (PublicKey, PrivateKey)> {
        &self.keys
    }

    /// Signs an operation with the private key corresponding to the given address
    pub fn create_operation(
        &self,
        content: Operation,
        address: Address,
    ) -> Result<SignedOperation, WalletError> {
        let sender_priv = self
            .find_associated_private_key(address)
            .ok_or(WalletError::MissingKeyError(address))?;
        Ok(Signed::new_signed(content, sender_priv).unwrap().1)
    }
}

impl std::fmt::Display for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f)?;
        for (addr, (public_key, private_key)) in &self.keys {
            writeln!(f, "Private key: {}", private_key)?;
            writeln!(f, "Public key: {}", public_key)?;
            writeln!(f, "Address: {}", addr)?;
        }
        Ok(())
    }
}
