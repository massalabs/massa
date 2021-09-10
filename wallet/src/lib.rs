// Copyright (c) 2021 MASSA LABS <info@massa.net>

// TODO:
//   * `wallet_info`: Shows wallet info
//   * `wallet_new_privkey`: Generates a new private key and adds it to the wallet. Returns the associated address.
//   * `send_transaction`: sends a transaction from <from_address> to <to_address> (from_address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <from_address> <to_address> <amount> <fee>
//   * `wallet_add_privkey`: Adds a list of private keys to the wallet. Returns the associated addresses. Parameters: list of private keys separated by ,  (no space).
//   * `buy_rolls`: buy roll count for <address> (address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <address>  <roll count> <fee>
//   * `sell_rolls`: sell roll count for <address> (address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <address>  <roll count> <fee>
//   * `cmd_testnet_rewards_program`: Returns rewards id. Parameter: <staking_address> <discord_ID>

use std::collections::HashMap;
use std::net::SocketAddr;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crypto::hash::Hash;
use crypto::signature::{derive_public_key, PrivateKey};
use models::address::{Address, AddressState, Addresses};
use models::amount::Amount;
use models::error::ReplError;
use models::ledger::LedgerData;
use models::node::PubkeySig;
use models::operation::{Operation, OperationContent, OperationType};
use models::slot::Slot;
use models::SerializeCompact;
use time::UTime;

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
        let keys = if path.is_file() {
            serde_json::from_str::<Vec<PrivateKey>>(&std::fs::read_to_string(path)?)?
        } else {
            Vec::new()
        };
        Ok(Wallet {
            keys,
            wallet_path: json_file.to_string(),
        })
    }

    pub fn sign_message(&self, address: Address, msg: Vec<u8>) -> Option<PubkeySig> {
        if let Some(key) = self.find_associated_private_key(address) {
            let public_key = crypto::derive_public_key(key);
            if let Ok(signature) = crypto::sign(&Hash::hash(&msg), key) {
                Some(PubkeySig {
                    public_key,
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
    pub fn add_private_key(&mut self, key: PrivateKey) -> Result<(), ReplError> {
        if !self.keys.iter().any(|file_key| file_key == &key) {
            self.keys.push(key);
            self.save()?;
        }
        Ok(())
    }

    /// Finds the private key associated with given address
    pub fn find_associated_private_key(&self, address: Address) -> Option<&PrivateKey> {
        self.keys.iter().find(|priv_key| {
            let pub_key = crypto::derive_public_key(priv_key);
            Address::from_public_key(&pub_key)
                .map(|addr| addr == address)
                .unwrap_or(false)
        })
    }

    pub fn get_wallet_address_list(&self) -> Vec<Address> {
        self.keys
            .iter()
            .map(|key| {
                let public_key = derive_public_key(key);
                Address::from_public_key(&public_key).unwrap() // private key has been tested: should never panic
            })
            .collect()
    }

    /// Save the wallet in json format in a file
    fn save(&self) -> Result<(), ReplError> {
        std::fs::write(&self.wallet_path, self.to_json_string()?)?;
        Ok(())
    }

    /// Export keys to json string
    pub fn to_json_string(&self) -> Result<String, ReplError> {
        serde_json::to_string_pretty(&self.keys).map_err(|err| err.into())
    }

    pub fn create_operation(
        &self,
        operation_type: OperationType,
        from_address: Address,
        fee: Amount,
        data: &ReplData,
    ) -> Result<Operation, ReplError> {
        //get node serialisation context
        let url = format!("http://{}/api/v1/node_config", data.node_ip);
        let resp = reqwest::blocking::get(&url)?;
        if resp.status() != StatusCode::OK {
            return Err(ReplError::GeneralError(format!(
                "Error during node connection. Server response code: {}",
                resp.status()
            )));
        }
        let context = resp.json::<models::SerializationContext>()?;

        // Set the context for the client process.
        models::init_serialization_context(context);

        // Get pool config
        // let url = format!("http://{}/api/v1/pool_config", data.node_ip);
        // let resp = reqwest::blocking::get(&url)?;
        // if resp.status() != StatusCode::OK {
        //     return Err(ReplError::GeneralError(format!(
        //         "Error during node connection. Server answer code: {}",
        //         resp.status()
        //     )));
        // }
        // let pool_cfg = resp.json::<pool::PoolConfig>()?;

        // Get consensus config
        let url = format!("http://{}/api/v1/consensus_config", data.node_ip);
        let resp = reqwest::blocking::get(&url)?;
        if resp.status() != StatusCode::OK {
            return Err(ReplError::GeneralError(format!(
                "Error during node connection. Server response code: {}",
                resp.status()
            )));
        }
        let consensus_cfg = resp.json::<ConsensusConfigData>()?;

        // Get from address private key
        let private_key = self
            .find_associated_private_key(from_address)
            .ok_or_else(|| {
                ReplError::GeneralError(format!(
                    "No private key found in the wallet for the specified FROM address: {}",
                    from_address.to_string()
                ))
            })?;
        let public_key = derive_public_key(private_key);

        let slot = consensus::get_current_latest_block_slot(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            0,
        )
        .map_err(|err| {
            ReplError::GeneralError(format!(
                "Error during current time slot computation: {:?}",
                err
            ))
        })?
        .unwrap_or_else(|| Slot::new(0, 0));

        let mut expire_period = slot.period + consensus_cfg.operation_validity_periods;
        if slot.thread >= from_address.get_thread(consensus_cfg.thread_count) {
            expire_period += 1;
        }

        // We don't care if that fails
        let _ = check_if_valid(data, &operation_type, from_address, fee, consensus_cfg);

        let operation_content = OperationContent {
            fee,
            expire_period,
            sender_public_key: public_key,
            op: operation_type,
        };

        let hash = Hash::hash(&operation_content.to_bytes_compact().unwrap());
        let signature = crypto::sign(&hash, private_key).unwrap();

        Ok(Operation {
            content: operation_content,
            signature,
        })
    }
}

fn check_if_valid(
    data: &ReplData,
    operation_type: &OperationType,
    from_address: Address,
    fee: Amount,
    consensus_cfg: ConsensusConfigData,
) -> Result<(), ReplError> {
    // Get address info
    let addrs = serde_qs::to_string(&Addresses {
        addrs: vec![from_address].into_iter().collect(),
    })?;
    let url = format!("http://{}/api/v1/addresses_info?{}", data.node_ip, addrs);
    let resp = reqwest::blocking::get(&url)?;
    if resp.status() == StatusCode::OK {
        let map_info = resp.json::<HashMap<Address, AddressState>>()?;

        if let Some(info) = map_info.get(&from_address) {
            match operation_type {
                OperationType::Transaction { amount, .. } => {
                    if info.candidate_ledger_data.balance < fee.saturating_add(*amount) {
                        println!("Warning : currently address {} has not enough coins for that transaction. It may be rejected", from_address);
                    }
                }
                OperationType::RollBuy { roll_count } => {
                    if info.candidate_ledger_data.balance
                        < consensus_cfg
                            .roll_price
                            .checked_mul_u64(*roll_count)
                            .ok_or(ReplError::GeneralError("".to_string()))?
                            .saturating_add(fee)
                    // It's just to print a warning
                    {
                        println!("Warning : currently address {} has not enough coins for that roll buy. It may be rejected", from_address);
                        println!(
                            "Info : current roll price is {} coins",
                            consensus_cfg.roll_price
                        );
                    }
                }
                OperationType::RollSell { roll_count } => {
                    if info.candidate_rolls < *roll_count
                        || info.candidate_ledger_data.balance < fee
                    {
                        println!("Warning : currently address {} has not enough rolls or coins for that roll sell. It may be rejected", from_address);
                    }
                }
            }
        } else {
            println!("Warning : currently address {} is not known by consensus. That operation may be rejected", from_address);
        }
    }
    Ok(())
}

/// contains the private keys created in the wallet.
#[derive(Debug)]
pub struct WalletInfo<'a> {
    pub wallet: &'a Wallet,
    pub balances: HashMap<Address, WrappedAddressState>,
}

impl<'a> std::fmt::Display for WalletInfo<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "WARNING: do not share your private keys")?;
        for key in &self.wallet.keys {
            let public_key = derive_public_key(key);
            let addr = Address::from_public_key(&public_key).map_err(|_| std::fmt::Error)?;
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

pub struct ReplData {
    pub node_ip: SocketAddr,
    pub cli: bool,
    pub wallet: Option<Wallet>,
}

impl Default for ReplData {
    fn default() -> Self {
        ReplData {
            node_ip: "0.0.0.0:3030".parse().unwrap(),
            cli: false,
            wallet: None,
        }
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
