//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Json RPC API for a massa-node

#![allow(clippy::too_many_arguments)]

use crate::api_trait::MassaApiServer;
use crate::{ApiServer, ApiV2, StopHandle, API};
use async_trait::async_trait;
use itertools::{izip, Itertools};
use jsonrpsee::core::error::SubscriptionClosed;
use jsonrpsee::core::{Error as JsonRpseeError, RpcResult};
use jsonrpsee::types::SubscriptionResult;
use jsonrpsee::SubscriptionSink;
use massa_api_exports::address::AddressInfo;
use massa_api_exports::block::{BlockInfo, BlockInfoContent};
use massa_api_exports::config::APIConfig;
use massa_api_exports::endorsement::EndorsementInfo;
use massa_api_exports::error::ApiError;
use massa_api_exports::operation::OperationInfo;
use massa_api_exports::slot::SlotAmount;
use massa_consensus_exports::{ConsensusChannels, ConsensusController};
use massa_models::address::Address;
use massa_models::block::{BlockGraphStatus, BlockId};
use massa_models::endorsement::{EndorsementId, SecureShareEndorsement};
use massa_models::operation::{OperationId, SecureShareOperation};
use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_models::slot::Slot;
use massa_models::timeslots;
use massa_models::version::Version;
use massa_pool_exports::PoolChannels;
use massa_protocol_exports::ProtocolSenders;
use massa_storage::Storage;
use serde::Serialize;
use std::net::SocketAddr;
use tokio_stream::wrappers::BroadcastStream;

impl API<ApiV2> {
    /// generate a new massa API
    pub fn new(
        consensus_controller: Box<dyn ConsensusController>,
        consensus_channels: ConsensusChannels,
        protocol_senders: ProtocolSenders,
        pool_channels: PoolChannels,
        api_settings: APIConfig,
        version: Version,
        storage: Storage,
    ) -> Self {
        API(ApiV2 {
            consensus_controller,
            consensus_channels,
            protocol_senders,
            pool_channels,
            api_settings,
            version,
            storage,
        })
    }
}

#[async_trait]
impl ApiServer for API<ApiV2> {
    async fn serve(
        self,
        url: &SocketAddr,
        api_config: &APIConfig,
    ) -> Result<StopHandle, JsonRpseeError> {
        crate::serve(self.into_rpc(), url, api_config).await
    }
}

#[doc(hidden)]
#[async_trait]
impl MassaApiServer for API<ApiV2> {
    fn get_version(&self) -> RpcResult<Version> {
        Ok(self.0.version)
    }

    fn get_endorsements(&self, eds: Vec<EndorsementId>) -> RpcResult<Vec<EndorsementInfo>> {
        // get the endorsements and the list of blocks that contain them from storage
        let storage_info: Vec<(SecureShareEndorsement, PreHashSet<BlockId>)> = {
            let read_blocks = self.0.storage.read_blocks();
            let read_endos = self.0.storage.read_endorsements();
            eds.iter()
                .filter_map(|id| {
                    read_endos.get(id).cloned().map(|ed| {
                        (
                            ed,
                            read_blocks
                                .get_blocks_by_endorsement(id)
                                .cloned()
                                .unwrap_or_default(),
                        )
                    })
                })
                .collect()
        };

        // keep only the ops found in storage
        let eds: Vec<EndorsementId> = storage_info.iter().map(|(ed, _)| ed.id).collect();

        // ask pool whether it carries the operations
        let in_pool = self
            .0
            .consensus_channels
            .pool_command_sender
            .contains_endorsements(&eds);

        let consensus_controller = self.0.consensus_controller.clone();
        let api_cfg = self.0.api_settings.clone();

        if eds.len() as u64 > api_cfg.max_arguments {
            return Err(ApiError::BadRequest("too many arguments".into()).into());
        }

        // check finality by cross-referencing Consensus and looking for final blocks that contain the endorsement
        let is_final: Vec<bool> = {
            let involved_blocks: Vec<BlockId> = storage_info
                .iter()
                .flat_map(|(_ed, bs)| bs.iter())
                .unique()
                .cloned()
                .collect();

            let involved_block_statuses = consensus_controller.get_block_statuses(&involved_blocks);

            let block_statuses: PreHashMap<BlockId, BlockGraphStatus> = involved_blocks
                .into_iter()
                .zip(involved_block_statuses.into_iter())
                .collect();
            storage_info
                .iter()
                .map(|(_ed, bs)| {
                    bs.iter()
                        .any(|b| block_statuses.get(b) == Some(&BlockGraphStatus::Final))
                })
                .collect()
        };

        // gather all values into a vector of EndorsementInfo instances
        let mut res: Vec<EndorsementInfo> = Vec::with_capacity(eds.len());
        let zipped_iterator = izip!(
            eds.into_iter(),
            storage_info.into_iter(),
            in_pool.into_iter(),
            is_final.into_iter()
        );
        for (id, (endorsement, in_blocks), in_pool, is_final) in zipped_iterator {
            res.push(EndorsementInfo {
                id,
                endorsement,
                in_pool,
                is_final,
                in_blocks: in_blocks.into_iter().collect(),
            });
        }

        // return values in the right order
        Ok(res)
    }

    fn get_blocks(&self, ids: Vec<BlockId>) -> RpcResult<Vec<BlockInfo>> {
        let consensus_controller = self.0.consensus_controller.clone();
        let storage = self.0.storage.clone_without_refs();
        let blocks = ids
            .into_iter()
            .filter_map(|id| {
                if let Some(wrapped_block) = storage.read_blocks().get(&id).cloned() {
                    if let Some(graph_status) = consensus_controller
                        .get_block_statuses(&[id])
                        .into_iter()
                        .next()
                    {
                        let is_final = graph_status == BlockGraphStatus::Final;
                        let is_in_blockclique =
                            graph_status == BlockGraphStatus::ActiveInBlockclique;
                        let is_candidate = graph_status == BlockGraphStatus::ActiveInBlockclique
                            || graph_status == BlockGraphStatus::ActiveInAlternativeCliques;
                        let is_discarded = graph_status == BlockGraphStatus::Discarded;

                        return Some(BlockInfo {
                            id,
                            content: Some(BlockInfoContent {
                                is_final,
                                is_in_blockclique,
                                is_candidate,
                                is_discarded,
                                block: wrapped_block.content,
                            }),
                        });
                    }
                }

                None
            })
            .collect::<Vec<BlockInfo>>();

        Ok(blocks)
    }

    fn get_operations(&self, ops: Vec<OperationId>) -> RpcResult<Vec<OperationInfo>> {
        // get the operations and the list of blocks that contain them from storage
        let storage_info: Vec<(SecureShareOperation, PreHashSet<BlockId>)> = {
            let read_blocks = self.0.storage.read_blocks();
            let read_ops = self.0.storage.read_operations();
            ops.iter()
                .filter_map(|id| {
                    read_ops.get(id).cloned().map(|op| {
                        (
                            op,
                            read_blocks
                                .get_blocks_by_operation(id)
                                .cloned()
                                .unwrap_or_default(),
                        )
                    })
                })
                .collect()
        };

        // keep only the ops found in storage
        let ops: Vec<OperationId> = storage_info.iter().map(|(op, _)| op.id).collect();

        // ask pool whether it carries the operations
        let in_pool = self
            .0
            .consensus_channels
            .pool_command_sender
            .contains_operations(&ops);

        let api_cfg = self.0.api_settings.clone();
        let consensus_controller = self.0.consensus_controller.clone();
        if ops.len() as u64 > api_cfg.max_arguments {
            return Err(ApiError::BadRequest("too many arguments".into()).into());
        }

        // check finality by cross-referencing Consensus and looking for final blocks that contain the op
        let is_final: Vec<bool> = {
            let involved_blocks: Vec<BlockId> = storage_info
                .iter()
                .flat_map(|(_op, bs)| bs.iter())
                .unique()
                .cloned()
                .collect();

            let involved_block_statuses = consensus_controller.get_block_statuses(&involved_blocks);

            let block_statuses: PreHashMap<BlockId, BlockGraphStatus> = involved_blocks
                .into_iter()
                .zip(involved_block_statuses.into_iter())
                .collect();
            storage_info
                .iter()
                .map(|(_op, bs)| {
                    bs.iter()
                        .any(|b| block_statuses.get(b) == Some(&BlockGraphStatus::Final))
                })
                .collect()
        };

        // gather all values into a vector of OperationInfo instances
        let mut res: Vec<OperationInfo> = Vec::with_capacity(ops.len());
        let zipped_iterator = izip!(
            ops.into_iter(),
            storage_info.into_iter(),
            in_pool.into_iter(),
            is_final.into_iter()
        );
        for (id, (operation, in_blocks), in_pool, is_final) in zipped_iterator {
            res.push(OperationInfo {
                id,
                operation,
                in_pool,
                is_final,
                in_blocks: in_blocks.into_iter().collect(),
            });
        }

        // return values in the right order
        Ok(res)
    }

    fn get_addresses(&self, addresses: Vec<Address>) -> RpcResult<Vec<AddressInfo>> {
        // get info from storage about which blocks the addresses have created
        let created_blocks: Vec<PreHashSet<BlockId>> = {
            let lck = self.0.storage.read_blocks();
            addresses
                .iter()
                .map(|address| {
                    lck.get_blocks_created_by(address)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect()
        };

        // get info from storage about which operations the addresses have created
        let created_operations: Vec<PreHashSet<OperationId>> = {
            let lck = self.0.storage.read_operations();
            addresses
                .iter()
                .map(|address| {
                    lck.get_operations_created_by(address)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect()
        };

        // get info from storage about which endorsements the addresses have created
        let created_endorsements: Vec<PreHashSet<EndorsementId>> = {
            let lck = self.0.storage.read_endorsements();
            addresses
                .iter()
                .map(|address| {
                    lck.get_endorsements_created_by(address)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect()
        };

        // get execution info
        let execution_infos = self
            .0
            .consensus_channels
            .execution_controller
            .get_addresses_infos(&addresses);

        // get future draws from selector
        let selection_draws = {
            let cur_slot = timeslots::get_current_latest_block_slot(
                self.0.api_settings.thread_count,
                self.0.api_settings.t0,
                self.0.api_settings.genesis_timestamp,
            )
            .expect("could not get latest current slot")
            .unwrap_or_else(|| Slot::new(0, 0));
            let slot_end = Slot::new(
                cur_slot
                    .period
                    .saturating_add(self.0.api_settings.draw_lookahead_period_count),
                cur_slot.thread,
            );
            addresses
                .iter()
                .map(|addr| {
                    self.0
                        .consensus_channels
                        .selector_controller
                        .get_address_selections(addr, cur_slot, slot_end)
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
        };

        // compile results
        let mut res = Vec::with_capacity(addresses.len());
        let iterator = izip!(
            addresses.into_iter(),
            created_blocks.into_iter(),
            created_operations.into_iter(),
            created_endorsements.into_iter(),
            execution_infos.into_iter(),
            selection_draws.into_iter(),
        );

        for (
            address,
            created_blocks,
            created_operations,
            created_endorsements,
            execution_infos,
            (next_block_draws, next_endorsement_draws),
        ) in iterator
        {
            res.push(AddressInfo {
                // general address info
                address,
                thread: address.get_thread(self.0.api_settings.thread_count),

                // final execution info
                final_balance: execution_infos.final_balance,
                final_roll_count: execution_infos.final_roll_count,
                final_datastore_keys: execution_infos
                    .final_datastore_keys
                    .into_iter()
                    .collect::<Vec<_>>(),

                // candidate execution info
                candidate_balance: execution_infos.candidate_balance,
                candidate_roll_count: execution_infos.candidate_roll_count,
                candidate_datastore_keys: execution_infos
                    .candidate_datastore_keys
                    .into_iter()
                    .collect::<Vec<_>>(),

                // deferred credits
                deferred_credits: execution_infos
                    .future_deferred_credits
                    .into_iter()
                    .map(|(slot, amount)| SlotAmount { slot, amount })
                    .collect::<Vec<_>>(),

                // selector info
                next_block_draws,
                next_endorsement_draws,

                // created objects
                created_blocks: created_blocks.into_iter().collect::<Vec<_>>(),
                created_endorsements: created_endorsements.into_iter().collect::<Vec<_>>(),
                created_operations: created_operations.into_iter().collect::<Vec<_>>(),

                // cycle infos
                cycle_infos: execution_infos.cycle_infos,
            });
        }

        Ok(res)
    }

    fn subscribe_new_blocks(&self, sink: SubscriptionSink) -> SubscriptionResult {
        broadcast_via_ws(self.0.consensus_channels.block_sender.clone(), sink);
        Ok(())
    }

    fn subscribe_new_blocks_headers(&self, sink: SubscriptionSink) -> SubscriptionResult {
        broadcast_via_ws(self.0.consensus_channels.block_header_sender.clone(), sink);
        Ok(())
    }

    fn subscribe_new_filled_blocks(&self, sink: SubscriptionSink) -> SubscriptionResult {
        broadcast_via_ws(self.0.consensus_channels.filled_block_sender.clone(), sink);
        Ok(())
    }

    fn subscribe_new_operations(&self, sink: SubscriptionSink) -> SubscriptionResult {
        broadcast_via_ws(self.0.pool_channels.operation_sender.clone(), sink);
        Ok(())
    }
}

/// Brodcast the stream(sender) content via a WebSocket
fn broadcast_via_ws<T: Serialize + Send + Clone + 'static>(
    sender: tokio::sync::broadcast::Sender<T>,
    mut sink: SubscriptionSink,
) {
    let rx = BroadcastStream::new(sender.subscribe());
    tokio::spawn(async move {
        match sink.pipe_from_try_stream(rx).await {
            SubscriptionClosed::Success => {
                sink.close(SubscriptionClosed::Success);
            }
            SubscriptionClosed::RemotePeerAborted => (),
            SubscriptionClosed::Failed(err) => {
                sink.close(err);
            }
        };
    });
}
