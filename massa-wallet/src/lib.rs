// Copyright (c) 2022 MASSA LABS <info@massa.net>

pub use error::WalletError;
use massa_hash::hash::Hash;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::composite::PubkeySig;
use massa_models::prehash::{Map, Set};
use massa_models::Operation;
use massa_models::OperationContent;
use massa_models::SerializeCompact;
use massa_signature::{derive_public_key, sign, PrivateKey, PublicKey};
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod error;

/// Contains the private keys created in the wallet.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Wallet {
    pub keys: Map<Address, (PublicKey, PrivateKey)>,
    pub wallet_path: PathBuf,
}

impl Wallet {
    /// Generates a new wallet initialized with the provided json file content
    pub fn new(path: PathBuf) -> Result<Wallet, WalletError> {
        let keys = if path.is_file() {
            serde_json::from_str::<Vec<PrivateKey>>(&std::fs::read_to_string(&path)?)?
        } else {
            Vec::new()
        };
        let keys = keys
            .iter()
            .map(|key| {
                let pub_key = derive_public_key(key);
                Ok((Address::from_public_key(&pub_key), (pub_key, *key)))
            })
            .collect::<Result<Map<Address, _>, WalletError>>()?;
        Ok(Wallet {
            keys,
            wallet_path: path,
        })
    }

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

    pub fn get_wallet_address_list(&self) -> Set<Address> {
        self.keys.keys().copied().collect()
    }

    /// Save the wallet in json format in a file
    fn save(&self) -> Result<(), WalletError> {
        std::fs::write(
            &self.wallet_path,
            serde_json::to_string_pretty(
                &self.keys.iter().map(|(_, (_, pk))| *pk).collect::<Vec<_>>(),
            )?,
        )?;
        Ok(())
    }

    /// Export keys to json string
    pub fn get_full_wallet(&self) -> &Map<Address, (PublicKey, PrivateKey)> {
        &self.keys
    }

    pub fn create_operation(
        &self,
        content: OperationContent,
        address: Address,
    ) -> Result<Operation, WalletError> {
        let hash = Hash::compute_from(&content.to_bytes_compact()?);
        let sender_priv = self
            .find_associated_private_key(address)
            .ok_or(WalletError::MissingKeyError(address))?;
        let signature = sign(&hash, sender_priv)?;
        Ok(Operation { content, signature })
    }
}

impl std::fmt::Display for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f)?;
        for k in &self.keys {
            let private_key = k.1 .1; // TODO: why not taking other fields of this struct?
            let public_key = derive_public_key(&private_key);
            let addr = Address::from_public_key(&public_key);
            writeln!(f)?;
            writeln!(f, "Private key: {}", private_key)?;
            writeln!(f, "Public key: {}", public_key)?;
            writeln!(f, "Address: {}", addr)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConsensusConfigData {
    pub t0: MassaTime,
    pub thread_count: u8,
    pub genesis_timestamp: MassaTime,
    pub delta_f0: u64,
    pub max_block_size: u32,
    pub operation_validity_periods: u64,
    pub periods_per_cycle: u64,
    pub roll_price: Amount,
}
