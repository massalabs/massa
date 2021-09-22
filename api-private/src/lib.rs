// Copyright (c) 2021 MASSA LABS <info@massa.net>
#![feature(async_closure)]
use api_dto::{AddressInfo, BalanceInfo, RollsInfo};
use communication::network::NetworkCommandSender;
use consensus::{get_latest_block_slot_at_timestamp, ConsensusCommandSender};
use crypto::signature::{PrivateKey, PublicKey, Signature};
use error::PrivateApiError;
use jsonrpc_core::{BoxFuture, IoHandler};
use jsonrpc_derive::rpc;
use jsonrpc_http_server::ServerBuilder;
use models::address::{Address, AddressHashSet};
use models::node::NodeId;
use models::Slot;
use rpc_server::rpc_server;
pub use rpc_server::API;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::thread;
use time::UTime;

mod error;

/// Private Massa-RPC "manager mode" endpoints
#[rpc(server)]
pub trait MassaPrivate {
    fn serve_massa_private(&mut self, _: ConsensusCommandSender, _: NetworkCommandSender);

    /// Starts the node and waits for node to start.
    /// Signals if the node is already running.
    #[rpc(name = "start_node")]
    fn start_node(&self) -> Result<(), PrivateApiError>;

    /// Gracefully stop the node.
    #[rpc(name = "stop_node")]
    fn stop_node(&self) -> BoxFuture<Result<(), PrivateApiError>>;

    #[rpc(name = "node_sign_message")]
    fn node_sign_message(
        &self,
        _: Vec<u8>,
    ) -> BoxFuture<Result<(PublicKey, Signature), PrivateApiError>>;

    /// Add a new private key for the node to use to stake.
    #[rpc(name = "add_staking_keys")]
    fn add_staking_keys(&self, _: Vec<PrivateKey>) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Remove an address used to stake.
    #[rpc(name = "remove_staking_keys")]
    fn remove_staking_keys(&self, _: Vec<Address>) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Return hashset of staking addresses.
    #[rpc(name = "list_staking_keys")]
    fn list_staking_keys(&self) -> BoxFuture<Result<AddressHashSet, PrivateApiError>>;

    #[rpc(name = "ban")]
    fn ban(&self, _: NodeId) -> BoxFuture<Result<(), PrivateApiError>>;

    #[rpc(name = "unban")]
    fn unban(&self, _: IpAddr) -> BoxFuture<Result<(), PrivateApiError>>;

    #[rpc(name = "get_addresses")]
    fn get_addresses(
        &self,
        _: Vec<Address>,
    ) -> BoxFuture<Result<Vec<AddressInfo>, PrivateApiError>>;
}

impl MassaPrivate for API {
    fn serve_massa_private(
        &mut self,
        consensus: ConsensusCommandSender,
        network: NetworkCommandSender,
    ) {
        self.consensus_command_sender = Some(consensus);
        self.network_command_sender = Some(network);
        rpc_server!(&self.clone());
    }

    fn start_node(&self) -> Result<(), PrivateApiError> {
        todo!()
    }

    fn stop_node(&self) -> BoxFuture<Result<(), PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let closure = async move || {
            Ok(cmd_sender
                .ok_or(PrivateApiError::MissingCommandSender(
                    "consensus command sender".to_string(),
                ))?
                .stop()
                .await?)
        };
        Box::pin(closure())
    }

    fn node_sign_message(
        &self,
        message: Vec<u8>,
    ) -> BoxFuture<Result<(PublicKey, Signature), PrivateApiError>> {
        let network_command_sender = self.network_command_sender.clone();
        let closure = async move || {
            Ok(network_command_sender
                .ok_or(PrivateApiError::MissingCommandSender(
                    "Network command sender".to_string(),
                ))?
                .node_sign_message(message)
                .await?)
        };
        Box::pin(closure())
    }

    fn add_staking_keys(&self, keys: Vec<PrivateKey>) -> BoxFuture<Result<(), PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let closure = async move || {
            Ok(cmd_sender
                .ok_or(PrivateApiError::MissingCommandSender(
                    "consensus command sender".to_string(),
                ))?
                .register_staking_private_keys(keys)
                .await?)
        };
        Box::pin(closure())
    }

    fn remove_staking_keys(&self, keys: Vec<Address>) -> BoxFuture<Result<(), PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let closure = async move || {
            Ok(cmd_sender
                .ok_or(PrivateApiError::MissingCommandSender(
                    "consensus command sender".to_string(),
                ))?
                .remove_staking_addresses(keys.into_iter().collect())
                .await?)
        };
        Box::pin(closure())
    }

    fn list_staking_keys(&self) -> BoxFuture<Result<AddressHashSet, PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let closure = async move || {
            Ok(cmd_sender
                .ok_or(PrivateApiError::MissingCommandSender(
                    "consensus command sender".to_string(),
                ))?
                .get_staking_addresses()
                .await?)
        };
        Box::pin(closure())
    }

    fn ban(&self, node_id: NodeId) -> BoxFuture<Result<(), PrivateApiError>> {
        let network_command_sender = self.network_command_sender.clone();
        let closure = async move || {
            Ok(network_command_sender
                .ok_or(PrivateApiError::MissingCommandSender(
                    "Network command sender".to_string(),
                ))?
                .ban(node_id)
                .await?)
        };
        Box::pin(closure())
    }

    fn unban(&self, ip: IpAddr) -> BoxFuture<Result<(), PrivateApiError>> {
        let network_command_sender = self.network_command_sender.clone();
        let closure = async move || {
            Ok(network_command_sender
                .ok_or(PrivateApiError::MissingCommandSender(
                    "Network command sender".to_string(),
                ))?
                .unban(ip)
                .await?)
        };
        Box::pin(closure())
    }

    fn get_addresses(
        &self,
        addresses: Vec<Address>,
    ) -> BoxFuture<Result<Vec<AddressInfo>, PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let cfg = self.consensus_config.clone();
        let api_cfg = self.api_config.clone();
        let addrs = addresses.clone();
        let closure = async move || {
            let mut res = Vec::new();
            let cmd_sender = cmd_sender.ok_or(PrivateApiError::MissingCommandSender(
                "consensus command sender".to_string(),
            ))?;

            // roll and balance info

            let cloned = addrs.clone();
            let states = cmd_sender
                .get_addresses_info(cloned.into_iter().collect())
                .await?;

            // next draws info
            let now = UTime::now(0)?; // todo get clock compensation ?

            let cfg = cfg.ok_or(PrivateApiError::MissingConfig(
                "consensus config".to_string(),
            ))?;

            let api_cfg =
                api_cfg.ok_or(PrivateApiError::MissingConfig("api config".to_string()))?;
            let current_slot = get_latest_block_slot_at_timestamp(
                cfg.thread_count,
                cfg.t0,
                cfg.genesis_timestamp,
                now,
            )?
            .unwrap_or(Slot::new(0, 0));

            let next_draws = cmd_sender
                .get_selection_draws(
                    current_slot,
                    Slot::new(
                        current_slot.period + api_cfg.draw_lookahead_period_count,
                        current_slot.thread,
                    ),
                )
                .await?;

            // block info
            let mut blocks = HashMap::new();
            let cloned = addrs.clone();
            for ad in cloned.iter() {
                blocks.insert(ad, cmd_sender.get_block_ids_by_creator(*ad).await?);
            }

            // endorsements info
            // todo add get_endorsements_by_address consensus command

            // operations info
            let mut ops = HashMap::new();
            let cloned = addrs.clone();
            for ad in cloned.iter() {
                ops.insert(ad, cmd_sender.get_operations_involving_address(*ad).await?);
            }

            // staking addrs
            let staking_addrs = cmd_sender.get_staking_addresses().await?;

            for address in addrs.into_iter() {
                let state = states.get(&address).ok_or(PrivateApiError::NotFound)?;
                res.push(AddressInfo {
                    address,
                    thread: address.get_thread(cfg.thread_count),
                    balance: BalanceInfo {
                        final_balance: state.final_ledger_data.balance,
                        candidate_balance: state.candidate_ledger_data.balance,
                        locked_balance: state.locked_balance,
                    },
                    rolls: RollsInfo {
                        active_rolls: state.active_rolls.unwrap_or_default(),
                        final_rolls: state.final_rolls,
                        candidate_rolls: state.candidate_rolls,
                    },
                    block_draws: next_draws
                        .iter()
                        .filter(|(_, (ad, _))| *ad == address)
                        .map(|(slot, _)| *slot)
                        .collect(),
                    endorsement_draws: next_draws
                        .iter()
                        .filter(|(_, (_, ads))| ads.contains(&address))
                        .map(|(slot, (_, ads))| {
                            ads.iter()
                                .enumerate()
                                .filter(|(_, ad)| **ad == address)
                                .map(|(i, _)| (*slot, i as u64))
                                .collect::<Vec<(Slot, u64)>>()
                        })
                        .flatten()
                        .collect(),
                    blocks_created: blocks
                        .get(&address)
                        .ok_or(PrivateApiError::NotFound)?
                        .keys()
                        .copied()
                        .collect(),
                    involved_in_endorsements: HashSet::new().into_iter().collect(), // todo update
                    involved_in_operations: ops
                        .get(&address)
                        .ok_or(PrivateApiError::NotFound)?
                        .keys()
                        .copied()
                        .collect(),
                    is_staking: staking_addrs.contains(&address),
                })
            }

            Ok(res)
        };
        Box::pin(closure())
    }
}
