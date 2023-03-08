// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::cmds::ExtendedWallet;
use console::style;
use erased_serde::{Serialize, Serializer};
use massa_api_exports::{
    address::AddressInfo, block::BlockInfo, datastore::DatastoreEntryOutput,
    endorsement::EndorsementInfo, execution::ExecuteReadOnlyResponse, node::NodeStatus,
    operation::OperationInfo,
};
use massa_models::composite::PubkeySig;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::PreHashSet;
use massa_models::{address::Address, operation::OperationId};
use massa_signature::{KeyPair, PublicKey};
use massa_wallet::Wallet;
use std::str;
use std::net::IpAddr;

#[macro_export]
macro_rules! massa_fancy_ascii_art_logo {
    () => {
        println!(
            "{}\n{}\n{}\n{}\n{}\n",
            style("███    ███  █████  ███████ ███████  █████ ").color256(160),
            style("████  ████ ██   ██ ██      ██      ██   ██").color256(161),
            style("██ ████ ██ ███████ ███████ ███████ ███████").color256(162),
            style("██  ██  ██ ██   ██      ██      ██ ██   ██").color256(163),
            style("██      ██ ██   ██ ███████ ███████ ██   ██").color256(164)
        );
    };
}

#[macro_export]
/// bail a shinny RPC error
macro_rules! rpc_error {
    ($e:expr) => {
        bail!("check if your node is running: {}", $e)
    };
}

#[macro_export]
/// print a yellow warning
macro_rules! client_warning {
    ($e:expr) => {
        println!("{}: {}", style("WARNING").yellow(), $e)
    };
}

pub enum Style {
    /// Any information that identifies an element
    Id,
    /// If a process is ongoing, not final, will change in the future
    Pending,
    /// If a process is finished, fixed, and won't evolve in the future
    Finished,
    /// Good things in general, success of an operation
    Good,
    /// Bad things in general, failure of an operation
    Bad,
    /// For any information that is unknown
    Unknown,
    /// Any amount of Massa coin displayed
    Coins,
    /// Any information concerning a block of the blockchain
    Block,
    /// For cryptographic signature
    Signature,
    /// For addresses or public keys
    Address,
    /// For any secret information
    Secret,
}

impl Style {
    fn style<T: ToString>(&self, msg: T) -> console::StyledObject<std::string::String> {
        style(msg.to_string()).color256(match self {
            Style::Id => 175,
            Style::Pending => 172,
            Style::Finished => 105,
            Style::Good => 118,
            Style::Bad => 160,
            Style::Unknown => 248,
            Style::Coins => 99,
            Style::Block => 158,
            Style::Signature => 220,
            Style::Address => 147,
            Style::Secret => 64,
        })
    }
}

pub trait Output: Serialize {
    fn pretty_print(&self);
}

impl dyn Output {
    pub(crate) fn stdout_json(&self) -> anyhow::Result<()> {
        let json = &mut serde_json::Serializer::new(std::io::stdout());
        let mut format: Box<dyn Serializer> = Box::new(<dyn Serializer>::erase(json));
        self.erased_serialize(&mut format)?;
        Ok(())
    }
}

impl Output for Wallet {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for ExtendedWallet {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for Vec<(Address, PublicKey)> {
    fn pretty_print(&self) {
        match self.len() {
            1 => println!("{}", self[0].1),
            _ => {
                for address_pubkey in self {
                    println!("Address: {}", address_pubkey.0);
                    println!("Public key: {}", address_pubkey.1);
                    println!();
                }
            }
        }
    }
}

impl Output for Vec<(Address, KeyPair)> {
    fn pretty_print(&self) {
        match self.len() {
            1 => println!("{}", self[0].1),
            _ => {
                for address_seckey in self {
                    println!("Address: {}", address_seckey.0);
                    println!("Secret key: {}", address_seckey.1);
                    println!();
                }
            }
        }
    }
}

impl Output for () {
    fn pretty_print(&self) {}
}

impl Output for String {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for &str {
    fn pretty_print(&self) {
        println!("{}", self)
    }
}

impl Output for NodeStatus {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for BlockInfo {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for PreHashSet<Address> {
    fn pretty_print(&self) {
        println!(
            "{}",
            self.iter()
                .fold("".to_string(), |acc, a| format!("{}{}\n", acc, a))
        )
    }
}

impl Output for Vec<AddressInfo> {
    fn pretty_print(&self) {
        for address_info in self {
            println!("{}", address_info);
        }
    }
}

impl Output for Vec<DatastoreEntryOutput> {
    fn pretty_print(&self) {
        for data_entry in self {
            println!("{}", data_entry);
        }
    }
}

impl Output for Vec<EndorsementInfo> {
    fn pretty_print(&self) {
        for endorsement_info in self {
            println!("{}", endorsement_info);
        }
    }
}

impl Output for Vec<IpAddr> {
    fn pretty_print(&self) {
        for ips in self {
            println!("{}", ips);
        }
    }
}

impl Output for Vec<OperationInfo> {
    fn pretty_print(&self) {
        for operation_info in self {
            println!("{}", operation_info);
        }
    }
}

impl Output for Vec<BlockInfo> {
    fn pretty_print(&self) {
        for block_info in self {
            println!("{}", block_info);
        }
    }
}

impl Output for Vec<OperationId> {
    fn pretty_print(&self) {
        for operation_id in self {
            println!("{}", operation_id);
        }
    }
}

impl Output for Vec<Address> {
    fn pretty_print(&self) {
        for addr in self {
            println!("{}", addr);
        }
    }
}

impl Output for Vec<SCOutputEvent> {
    fn pretty_print(&self) {
        for addr in self {
            println!("{}", addr);
        }
    }
}

impl Output for PubkeySig {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}

impl Output for ExecuteReadOnlyResponse {
    fn pretty_print(&self) {
        println!("{}", self);
    }
}
