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
    /// Any information related to the protocol, staking, peers and the consensus
    Protocol,
    /// Any information concerning a block of the blockchain
    Block,
    /// For cryptographic signature
    Signature,
    /// For any information related to the wallet, addresses or public keys
    Wallet,
    /// For any secret information
    Secret,
    /// To separate some informations on the screen by barely visible characters
    Separator,
    /// When displaying a timestamp or date
    Time,
}

impl Style {
    fn style<T: ToString>(&self, msg: T) -> console::StyledObject<std::string::String> {
        style(msg.to_string()).color256(match self {
            Style::Id => 218,        // #ffafd7
            Style::Pending => 172,   // #d78700
            Style::Finished => 81,   // #5fd7ff
            Style::Good => 112,      // #87d700
            Style::Bad => 160,       // #d70000
            Style::Unknown => 248,   // #a8a8a8
            Style::Coins => 141,     // #af87ff
            Style::Protocol => 151,  // #afd7af
            Style::Block => 158,     // #afffd7
            Style::Signature => 220, // #ffd700
            Style::Wallet => 213,    // #ff87ff
            Style::Secret => 64,     // #5f8700
            Style::Separator => 239, // #4e4e4e
            Style::Time => 117,      // #87d7ff
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
        for info in self {
            println!("{}", Style::Separator.style("========"));
            println!("Address {} (thread {}):",
                Style::Wallet.style(info.address),
                Style::Protocol.style(info.thread),
            );
            println!(
                "\tBalance: {}={}, {}={}",
                Style::Finished.style("final"),
                Style::Coins.style(info.final_balance),
                Style::Pending.style("candidate"),
                Style::Coins.style(info.candidate_balance),
            );
            println!(
                "\tRolls: {}={}, {}={}",
                Style::Finished.style("final"),
                Style::Protocol.style(info.final_roll_count),
                Style::Pending.style("candidate"),
                Style::Protocol.style(info.candidate_roll_count),
            );

            print!("\tLocked coins:");
            if info.deferred_credits.is_empty() {
                println!(" {}", Style::Coins.style("0"));
            } else {
                println!();
                for slot_amount in &info.deferred_credits {
                    println!(
                        "\t\t{} locked coins will be unlocked at slot {}",
                        Style::Coins.style(slot_amount.amount),
                        Style::Protocol.style(slot_amount.slot),
                    );
                }
            }
            if !info.cycle_infos.is_empty() {
                println!("\tCycle infos:");
            }
            for cycle_info in &info.cycle_infos {
                println!(
                    "\t\tCycle {} ({}): produced {} and missed {} blocks{}",
                    Style::Protocol.style(cycle_info.cycle),
                    if cycle_info.is_final {
                        Style::Finished.style("final")
                    } else {
                        Style::Pending.style("candidate")
                    },
                    Style::Good.style(cycle_info.ok_count),
                    Style::Bad.style(cycle_info.nok_count),
                    match cycle_info.active_rolls {
                        Some(rolls) => format!(" with {} active rolls", Style::Good.style(rolls)),
                        None => "".into(),
                    },
                );
            }
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
        for info in self {
            println!("{}", style("==========").color256(237));
            print!("Operation {}", Style::Id.style(info.id));
            if info.in_pool {
                print!(", {}", Style::Pending.style("in pool"));
            }
            if let Some(f) = info.is_operation_final {
                print!(", operation is {}", if f {
                    Style::Finished.style("final")
                } else {
                    Style::Pending.style("not final")
                });
            } else {
                print!(", finality {}", Style::Unknown.style("unknown"));
            }
            println!(", {}", match info.op_exec_status {
                Some(true) => Style::Good.style("success"),
                Some(false) => Style::Bad.style("failed"),
                None => Style::Unknown.style("unknown status"),
            });
            if info.in_blocks.is_empty() {
                println!("{}", Style::Block.style("Not in any blocks"));
            } else {
                println!("In blocks:");
                for bid in info.in_blocks.iter() {
                    println!("\t- {}", Style::Block.style(bid));
                }
            }
            println!("Signature: {}", Style::Signature.style(info.operation.signature));
            println!("Creator pubkey: {}",
                Style::Wallet.style(info.operation.content_creator_pub_key));
            println!("Creator address: {}",
                Style::Wallet.style(info.operation.content_creator_address));
            println!("Fee: {}", Style::Coins.style(info.operation.content.fee));
            println!("Expire period: {}", Style::Pending.style(info.operation.content.expire_period));
            println!("Operation type: {}", Style::Id.style(&info.operation.content.op));
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
