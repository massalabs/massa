// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::pos::ExportProofOfStake;

use super::{
    block_graph::*,
    config::{ConsensusConfig, CHANNEL_SIZE},
    consensus_worker::{
        AddressState, ConsensusCommand, ConsensusEvent, ConsensusManagementCommand, ConsensusStats,
        ConsensusWorker,
    },
    error::ConsensusError,
    pos::ProofOfStake,
};
use communication::protocol::{ProtocolCommandSender, ProtocolEventReceiver};
use crypto::signature::PrivateKey;
use logging::debug;
use models::{
    Address, Block, BlockId, OperationId, OperationSearchResult, Slot, StakersCycleProductionStats,
};
use pool::PoolCommandSender;
use std::collections::{HashMap, HashSet, VecDeque};
use storage::StorageAccess;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// Creates a new consensus controller.
///
/// # Arguments
/// * cfg: consensus configuration
/// * protocol_command_sender: a ProtocolCommandSender instance to send commands to Protocol.
/// * protocol_event_receiver: a ProtocolEventReceiver instance to receive events from Protocol.
pub async fn start_consensus_controller(
    cfg: ConsensusConfig,
    protocol_command_sender: ProtocolCommandSender,
    protocol_event_receiver: ProtocolEventReceiver,
    pool_command_sender: PoolCommandSender,
    opt_storage_command_sender: Option<StorageAccess>,
    boot_pos: Option<ExportProofOfStake>,
    boot_graph: Option<BootsrapableGraph>,
    clock_compensation: i64,
) -> Result<
    (
        ConsensusCommandSender,
        ConsensusEventReceiver,
        ConsensusManager,
    ),
    ConsensusError,
> {
    debug!("starting consensus controller");
    massa_trace!(
        "consensus.consensus_controller.start_consensus_controller",
        {}
    );

    // ensure that the parameters are sane
    if cfg.thread_count == 0 {
        return Err(ConsensusError::ConfigError(
            "thread_count shoud be strictly more than 0".to_string(),
        ));
    }
    if cfg.t0 == 0.into() {
        return Err(ConsensusError::ConfigError(
            "t0 shoud be strictly more than 0".to_string(),
        ));
    }
    if cfg.t0.checked_rem_u64(cfg.thread_count as u64)? != 0.into() {
        return Err(ConsensusError::ConfigError(
            "thread_count should divide t0".to_string(),
        ));
    }

    // start worker
    let block_db = BlockGraph::new(cfg.clone(), boot_graph).await?;
    let pos = ProofOfStake::new(cfg.clone(), block_db.get_genesis_block_ids(), boot_pos).await?;
    let (command_tx, command_rx) = mpsc::channel::<ConsensusCommand>(CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel::<ConsensusEvent>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<ConsensusManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        let res = ConsensusWorker::new(
            cfg_copy,
            protocol_command_sender,
            protocol_event_receiver,
            pool_command_sender,
            opt_storage_command_sender,
            block_db,
            pos,
            command_rx,
            event_tx,
            manager_rx,
            clock_compensation,
        )
        .await?
        .run_loop()
        .await;
        match res {
            Err(err) => {
                error!("consensus worker crashed: {:?}", err);
                Err(err)
            }
            Ok(v) => {
                info!("consensus worker finished cleanly");
                Ok(v)
            }
        }
    });
    Ok((
        ConsensusCommandSender(command_tx),
        ConsensusEventReceiver(event_rx),
        ConsensusManager {
            join_handle,
            manager_tx,
        },
    ))
}

#[derive(Clone)]
pub struct ConsensusCommandSender(pub mpsc::Sender<ConsensusCommand>);

impl ConsensusCommandSender {
    /// Gets all the aviable information on the block graph returning a Blockgraphexport.
    pub async fn get_block_graph_status(&self) -> Result<BlockGraphExport, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<BlockGraphExport>();
        massa_trace!("consensus.consensus_controller.get_block_graph_status", {});
        self.0
            .send(ConsensusCommand::GetBlockGraphStatus(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_block_graph_status".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_block_graph_status response read error".to_string(),
            )
        })
    }
    /// Gets the whole block and its status corresponding to given hash.
    ///
    /// # Arguments
    /// * hash: hash corresponding to the block we want.
    pub async fn get_block_status(
        &self,
        block_id: BlockId,
    ) -> Result<Option<ExportBlockStatus>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<Option<ExportBlockStatus>>();
        massa_trace!("consensus.consensus_controller.get_active_block", {});
        self.0
            .send(ConsensusCommand::GetBlockStatus {
                block_id,
                response_tx,
            })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_block_status".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_block_status response read error".to_string(),
            )
        })
    }

    /// Gets the whole block corresponding to given hash.
    ///
    /// # Arguments
    /// * hash: hash corresponding to the block we want.
    pub async fn get_active_block(
        &self,
        block_id: BlockId,
    ) -> Result<Option<Block>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<Option<Block>>();
        massa_trace!("consensus.consensus_controller.get_active_block", {});
        self.0
            .send(ConsensusCommand::GetActiveBlock {
                block_id,
                response_tx,
            })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_active_block".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_active_block response read error".to_string(),
            )
        })
    }

    /// Gets (slot, public_key) were the staker with public_key was selected for slot, between start_slot and end_slot.
    ///
    /// # Arguments
    /// * start_slot: begining of the considered interval.
    /// * end_slot: end of the considered interval.
    pub async fn get_selection_draws(
        &self,
        start: Slot,
        end: Slot,
    ) -> Result<Vec<(Slot, (Address, Vec<Address>))>, ConsensusError> {
        massa_trace!("consensus.consensus_controller.get_selection_draws", {});
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(ConsensusCommand::GetSelectionDraws {
                start,
                end,
                response_tx,
            })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_selection_draws".into(),
                )
            })?;
        let res = response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_selection_draws response read error".to_string(),
            )
        })?;
        res
    }

    pub async fn get_bootstrap_state(
        &self,
    ) -> Result<(ExportProofOfStake, BootsrapableGraph), ConsensusError> {
        let (response_tx, response_rx) =
            oneshot::channel::<(ExportProofOfStake, BootsrapableGraph)>();
        massa_trace!("consensus.consensus_controller.get_bootstrap_state", {});
        self.0
            .send(ConsensusCommand::GetBootstrapState(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_bootstrap_state".into(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_bootstrap_state response read error".to_string(),
            )
        })
    }
    pub async fn get_block_ids_by_creator(
        &self,
        address: Address,
    ) -> Result<HashMap<BlockId, Status>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel();
        massa_trace!("consensus.consensus_controller.get_block_ids_by_creator", {
        });
        self.0
            .send(ConsensusCommand::GetBlockIdsByCreator {
                address,
                response_tx,
            })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_block_ids_by_creator".into(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_block_ids_by_creator response read error".to_string(),
            )
        })
    }

    pub async fn get_operations(
        &self,
        operation_ids: HashSet<OperationId>,
    ) -> Result<HashMap<OperationId, OperationSearchResult>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel();
        massa_trace!("consensus.consensus_controller.get_operatiosn", {
            "operation_ids": operation_ids
        });
        self.0
            .send(ConsensusCommand::GetOperations {
                operation_ids,
                response_tx,
            })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_operations".into(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_operations response read error".to_string(),
            )
        })
    }

    /// Gets the candidate and final ledger data of a list of addresses
    pub async fn get_addresses_info(
        &self,
        addresses: HashSet<Address>,
    ) -> Result<HashMap<Address, AddressState>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<HashMap<Address, AddressState>>();
        massa_trace!("consensus.consensus_controller.get_addresses_info", {
            "addresses": addresses
        });
        self.0
            .send(ConsensusCommand::GetAddressesInfo {
                addresses,
                response_tx,
            })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_addresses_info".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_addresses_info response read error".to_string(),
            )
        })
    }

    /// Returns hashmap: Operation id -> if it is final
    pub async fn get_operations_involving_address(
        &self,
        address: Address,
    ) -> Result<HashMap<OperationId, OperationSearchResult>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel();
        massa_trace!(
            "consensus.consensus_controller.get_operations_involving_address",
            { "address": address }
        );
        self.0
            .send(ConsensusCommand::GetRecentOperations {
                address,
                response_tx,
            })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_operations_involving_address".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_operations_involving_address response read error"
                    .to_string(),
            )
        })
    }

    pub async fn get_stats(&self) -> Result<ConsensusStats, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel();
        massa_trace!("consensus.consensus_controller.get_stats", {});
        self.0
            .send(ConsensusCommand::GetStats(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_stats".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_stats response read error".to_string(),
            )
        })
    }

    pub async fn get_active_stakers(
        &self,
    ) -> Result<Option<HashMap<Address, u64>>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel();
        massa_trace!("consensus.consensus_controller.get_active_stakers", {});
        self.0
            .send(ConsensusCommand::GetActiveStakers(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_active_stakers".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_active_stakers response read error".to_string(),
            )
        })
    }

    pub async fn register_staking_private_keys(
        &self,
        keys: Vec<PrivateKey>,
    ) -> Result<(), ConsensusError> {
        massa_trace!(
            "consensus.consensus_controller.register_staking_private_keys",
            {}
        );
        self.0
            .send(ConsensusCommand::RegisterStakingPrivateKeys(keys))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError("send error consensus command".to_string())
            })
    }

    pub async fn remove_staking_addresses(
        &self,
        addresses: HashSet<Address>,
    ) -> Result<(), ConsensusError> {
        massa_trace!("consensus.consensus_controller.remove_staking_addresses", {
        });
        self.0
            .send(ConsensusCommand::RemoveStakingAddresses(addresses))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError("send error consensus command".to_string())
            })
    }

    pub async fn get_staking_addresses(&self) -> Result<HashSet<Address>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel();
        massa_trace!("consensus.consensus_controller.get_staking_addresses", {});
        self.0
            .send(ConsensusCommand::GetStakingAddressses(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_staking_addresses".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_staking_addresses response read error".to_string(),
            )
        })
    }

    pub async fn get_stakers_production_stats(
        &self,
        addrs: HashSet<Address>,
    ) -> Result<Vec<StakersCycleProductionStats>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel();
        massa_trace!(
            "consensus.consensus_controller.get_stakers_production_stats",
            {}
        );
        self.0
            .send(ConsensusCommand::GetStakersProductionStats { addrs, response_tx })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_stakers_production_stats".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_stakers_production_statsresponse read error".to_string(),
            )
        })
    }
}

pub struct ConsensusEventReceiver(pub mpsc::Receiver<ConsensusEvent>);

impl ConsensusEventReceiver {
    pub async fn wait_event(&mut self) -> Result<ConsensusEvent, ConsensusError> {
        let evt = self
            .0
            .recv()
            .await
            .ok_or(ConsensusError::ControllerEventError);
        evt
    }

    /// drains remaining events and returns them in a VecDeque
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<ConsensusEvent> {
        let mut remaining_events: VecDeque<ConsensusEvent> = VecDeque::new();

        while let Some(evt) = self.0.recv().await {
            remaining_events.push_back(evt);
        }
        remaining_events
    }
}

pub struct ConsensusManager {
    join_handle: JoinHandle<Result<ProtocolEventReceiver, ConsensusError>>,
    manager_tx: mpsc::Sender<ConsensusManagementCommand>,
}

impl ConsensusManager {
    pub async fn stop(
        self,
        consensus_event_receiver: ConsensusEventReceiver,
    ) -> Result<ProtocolEventReceiver, ConsensusError> {
        massa_trace!("consensus.consensus_controller.stop", {});
        drop(self.manager_tx);
        let _remaining_events = consensus_event_receiver.drain().await;
        let protocol_event_receiver = self.join_handle.await??;
        Ok(protocol_event_receiver)
    }
}
