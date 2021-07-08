use crate::ReplError;
use crypto::signature::{derive_public_key, PrivateKey};
use models::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// contains the private keys created in the wallet.
#[derive(Debug, Serialize, Deserialize)]
pub struct Wallet {
    keys: Vec<PrivateKey>,
    wallet_path: String,
}

impl Wallet {
    /// Generates a new wallet initialized with the provided json file content
    pub fn new(json_file: &str) -> Result<Wallet, ReplError> {
        let path = std::path::Path::new(json_file);
        let keys = if path.exists() {
            serde_json::from_str::<Vec<PrivateKey>>(&std::fs::read_to_string(path)?)?
        } else {
            Vec::new()
        };
        Ok(Wallet {
            keys,
            wallet_path: json_file.to_string(),
        })
    }

    /// Adds a new private key to wallet, if it was missing
    pub fn add_private_key(&mut self, key: PrivateKey) -> Result<(), ReplError> {
        if self
            .keys
            .iter()
            .find(|file_key| file_key == &&key)
            .is_none()
        {
            self.keys.push(key);
            self.save()?;
        }
        Ok(())
    }

    /// Finds the private key associated with given address
    pub fn find_associated_private_key(&self, address: Address) -> Option<&PrivateKey> {
        self.keys.iter().find(|priv_key| {
            let pub_key = crypto::derive_public_key(&priv_key);
            Address::from_public_key(&pub_key)
                .map(|addr| if addr == address { true } else { false })
                .unwrap_or(false)
        })
    }

    pub fn get_wallet_address_list(&self) -> HashSet<Address> {
        self.keys
            .iter()
            .map(|key| {
                let public_key = derive_public_key(&key);
                Address::from_public_key(&public_key).unwrap() //private key has been tested: should never panic
            })
            .collect()
    }

    //save the wallet in json format in a file
    fn save(&self) -> Result<(), ReplError> {
        std::fs::write(&self.wallet_path, self.to_json_string()?)?;
        Ok(())
    }

    /// Export keys to json string
    pub fn to_json_string(&self) -> Result<String, ReplError> {
        serde_json::to_string_pretty(&self.keys).map_err(|err| err.into())
    }
}

impl std::fmt::Display for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Wallet private key list:")?;
        for key in &self.keys {
            let public_key = derive_public_key(&key);
            let addr = Address::from_public_key(&public_key).map_err(|_| std::fmt::Error)?;
            writeln!(f, "key:{} public:{} addr:{}", key, public_key, addr)?;
        }
        Ok(())
    }
}
