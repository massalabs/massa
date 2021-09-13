// Copyright (c) 2021 MASSA LABS <info@massa.net>

// TODO:
//   * `wallet_info`: Shows wallet info
//   * `wallet_new_privkey`: Generates a new private key and adds it to the wallet. Returns the associated address.
//   * `send_transaction`: sends a transaction from <from_address> to <to_address> (from_address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <from_address> <to_address> <amount> <fee>
//   * `wallet_add_privkey`: Adds a list of private keys to the wallet. Returns the associated addresses. Parameters: list of private keys separated by ,  (no space).
//   * `buy_rolls`: buy roll count for <address> (address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <address>  <roll count> <fee>
//   * `sell_rolls`: sell roll count for <address> (address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <address>  <roll count> <fee>
//   * `cmd_testnet_rewards_program`: Returns rewards id. Parameter: <staking_address> <discord_ID>

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crypto::hash::Hash;
use crypto::signature::{PrivateKey, PublicKey};
use models::address::{Address, AddressHashMap};
use models::amount::Amount;
use models::crypto::PubkeySig;
use models::ledger::LedgerData;
use time::UTime;

mod error;

pub use error::WalletError;

/// contains the private keys created in the wallet.
#[derive(Debug, Serialize, Deserialize)]
pub struct Wallet {
    keys: HashMap<Address, (PublicKey, PrivateKey)>,
    wallet_path: String,
}

impl Wallet {
    /// Generates a new wallet initialized with the provided json file content
    pub fn new(json_file: &str) -> Result<Wallet, WalletError> {
        let path = std::path::Path::new(json_file);
        let keys = if path.is_file() {
            serde_json::from_str::<Vec<PrivateKey>>(&std::fs::read_to_string(path)?)?
        } else {
            Vec::new()
        };
        let keys = keys
            .iter()
            .map(|key| {
                let pub_key = crypto::derive_public_key(key);
                Ok((Address::from_public_key(&pub_key)?, (pub_key, *key)))
            })
            .collect::<Result<HashMap<_, _>, WalletError>>()?;
        Ok(Wallet {
            keys,
            wallet_path: json_file.to_string(),
        })
    }

    pub fn sign_message(&self, address: Address, msg: Vec<u8>) -> Option<PubkeySig> {
        if let Some((public_key, key)) = self.keys.get(&address) {
            if let Ok(signature) = crypto::sign(&Hash::hash(&msg), key) {
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
    pub fn add_private_key(&mut self, key: PrivateKey) -> Result<(), WalletError> {
        if !self.keys.iter().any(|(_, (_, file_key))| file_key == &key) {
            let pub_key = crypto::derive_public_key(&key);
            self.keys
                .insert(Address::from_public_key(&pub_key)?, (pub_key, key));
            self.save()?;
        }
        Ok(())
    }

    pub fn remove_address(&mut self, address: Address) -> Option<(PublicKey, PrivateKey)> {
        self.keys.remove(&address)
    }

    /// Finds the private key associated with given address
    pub fn find_associated_private_key(&self, address: Address) -> Option<&PrivateKey> {
        self.keys.get(&address).map(|(_pub_key, priv_key)| priv_key)
    }

    pub fn get_wallet_address_list(&self) -> HashSet<Address> {
        self.keys.keys().copied().collect()
    }

    /// Save the wallet in json format in a file
    fn save(&self) -> Result<(), WalletError> {
        std::fs::write(&self.wallet_path, serde_json::to_string_pretty(&self.keys)?)?;
        Ok(())
    }

    /// Export keys to json string
    pub fn get_full_wallet(&self) -> &HashMap<Address, (PublicKey, PrivateKey)> {
        &self.keys
    }
}

/// contains the private keys created in the wallet.
#[derive(Debug)]
pub struct WalletInfo<'a> {
    pub wallet: &'a Wallet,
    pub balances: AddressHashMap<WrappedAddressState>,
}

impl<'a> std::fmt::Display for WalletInfo<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "WARNING: do not share your private keys")?;
        for (addr, (public_key, key)) in &self.wallet.keys {
            writeln!(f)?;
            writeln!(f, "Private key: {}", key)?;
            writeln!(f, "Public key: {}", public_key)?;
            writeln!(f, "Address: {}", addr)?;
            match self.balances.get(&addr) {
                Some(balance) => {
                    write!(f, "State: \n{}", balance)?;
                }
                None => writeln!(f, "No balance info available. Is your node running?")?,
            }
        }
        Ok(())
    }
}

// impl std::fmt::Display for Wallet {
//     fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
//         writeln!(f)?;
//         for key in &self.keys {
//             let public_key = derive_public_key(key);
//             let addr = Address::from_public_key(&public_key).map_err(|_| std::fmt::Error)?;
//             writeln!(f)?;
//             writeln!(f, "Private key: {}", key)?;
//             writeln!(f, "Public key: {}", public_key)?;
//             writeln!(f, "Address: {}", addr)?;
//         }
//         Ok(())
//     }
// }

#[derive(Debug, Deserialize, Clone)]
pub struct ConsensusConfigData {
    pub t0: UTime,
    pub thread_count: u8,
    pub genesis_timestamp: UTime,
    pub delta_f0: u64,
    pub max_block_size: u32,
    pub operation_validity_periods: u64,
    pub periods_per_cycle: u64,
    pub roll_price: Amount,
}

impl<'a> std::fmt::Display for WrappedAddressState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "    final balance: {}", self.final_ledger_data.balance)?;
        writeln!(
            f,
            "    candidate balance: {}",
            self.candidate_ledger_data.balance
        )?;
        writeln!(f, "    locked balance: {}", self.locked_balance)?;
        writeln!(f, "    final rolls: {}", self.final_rolls)?;
        writeln!(f, "    candidate rolls: {}", self.candidate_rolls)?;

        if let Some(active) = self.active_rolls {
            writeln!(f, "    active rolls: {}", active)?;
        } else {
            writeln!(f, "    No active roll")?;
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WrappedAddressState {
    pub final_rolls: u64,
    pub active_rolls: Option<u64>,
    pub candidate_rolls: u64,
    pub locked_balance: Amount,
    pub candidate_ledger_data: LedgerData,
    pub final_ledger_data: LedgerData,
}
