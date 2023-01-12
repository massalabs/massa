//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Json RPC API for a massa-node
use std::net::SocketAddr;

use crate::api_trait::MassaApiServer;
use crate::{APIConfig, ApiServer, ApiV2, StopHandle, API};
use async_trait::async_trait;
use itertools::izip;
use jsonrpsee::core::error::SubscriptionClosed;
use jsonrpsee::core::{Error as JsonRpseeError, RpcResult};
use jsonrpsee::types::SubscriptionResult;
use jsonrpsee::SubscriptionSink;
use massa_consensus_exports::ConsensusChannels;
use massa_execution_exports::ExecutionController;
use massa_models::address::Address;
use massa_models::api::{AddressInfo, SlotAmount};
use massa_models::block::BlockId;
use massa_models::endorsement::EndorsementId;
use massa_models::operation::OperationId;
use massa_models::prehash::PreHashSet;
use massa_models::slot::Slot;
use massa_models::timeslots;
use massa_models::version::Version;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolSenders;
use massa_storage::Storage;
use massa_pool_exports::PoolChannels;
use serde::Serialize;
use tokio_stream::wrappers::BroadcastStream;

impl API<ApiV2> {
    /// generate a new massa API
    pub fn new(
        execution_controller: Box<dyn ExecutionController>,
        selector_controller: Box<dyn SelectorController>,
        consensus_channels: ConsensusChannels,
        pool_channels: PoolChannels,
        api_settings: APIConfig,
        version: Version,
        storage: Storage,
    ) -> Self {
        API(ApiV2 {
            execution_controller,
            selector_controller,
            consensus_channels,
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
    async fn get_addresses(&self, addresses: Vec<Address>) -> RpcResult<Vec<AddressInfo>> {
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
        let execution_infos = self.0.execution_controller.get_addresses_infos(&addresses);

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

    async fn get_version(&self) -> RpcResult<Version> {
        Ok(self.0.version)
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
