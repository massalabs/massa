//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Json RPC API for a massa-node
use std::net::SocketAddr;

use crate::api_trait::MassaApiServer;
use crate::{ApiServer, ApiV2, StopHandle, API};
use async_trait::async_trait;
use futures::future::{self, Either};
use futures::StreamExt;
use jsonrpsee::core::{Error as JsonRpseeError, RpcResult, SubscriptionResult};
use jsonrpsee::{PendingSubscriptionSink, SubscriptionMessage};
use massa_api_exports::config::APIConfig;
use massa_api_exports::error::ApiError;
use massa_api_exports::page::{PageRequest, PagedVec, PagedVecV2};
use massa_api_exports::ApiRequest;
use massa_consensus_exports::{ConsensusChannels, ConsensusController};
use massa_execution_exports::ExecutionController;
use massa_models::address::Address;
use massa_models::block_id::BlockId;
use massa_models::slot::Slot;
use massa_models::timeslots::get_latest_block_slot_at_timestamp;
use massa_models::version::Version;
use massa_pool_exports::PoolChannels;
use massa_time::MassaTime;
use serde::Serialize;
use tokio_stream::wrappers::BroadcastStream;

impl API<ApiV2> {
    /// generate a new massa API
    pub fn new(
        consensus_controller: Box<dyn ConsensusController>,
        consensus_channels: ConsensusChannels,
        execution_controller: Box<dyn ExecutionController>,
        pool_channels: PoolChannels,
        api_settings: APIConfig,
        version: Version,
    ) -> Self {
        API(ApiV2 {
            consensus_controller,
            consensus_channels,
            execution_controller,
            pool_channels,
            api_settings,
            version,
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
    async fn get_largest_stakers(
        &self,
        api_request: Option<ApiRequest>,
    ) -> RpcResult<PagedVecV2<(Address, u64)>> {
        let execution_controller = self.0.execution_controller.clone();
        let cfg = self.0.api_settings.clone();

        let now = match MassaTime::now() {
            Ok(now) => now,
            Err(e) => return Err(ApiError::TimeError(e).into()),
        };

        let latest_block_slot_at_timestamp_result = get_latest_block_slot_at_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            now,
        );

        let last_start_period_cycle =
            Slot::new(self.0.api_settings.last_start_period, 0).get_cycle(cfg.periods_per_cycle);
        let curr_cycle = match latest_block_slot_at_timestamp_result {
            Ok(Some(cur_slot)) if cur_slot.period <= self.0.api_settings.last_start_period => {
                last_start_period_cycle
            }
            Ok(Some(cur_slot)) => cur_slot.get_cycle(cfg.periods_per_cycle),
            Ok(None) => 0,
            Err(e) => return Err(ApiError::ModelsError(e).into()),
        };

        let mut staker_vec = execution_controller
            .get_cycle_active_rolls(curr_cycle)
            .into_iter()
            .collect::<Vec<(Address, u64)>>();

        staker_vec
            .sort_by(|&(_, roll_counts_a), &(_, roll_counts_b)| roll_counts_b.cmp(&roll_counts_a));

        let paged_vec = if let Some(api_request) = api_request {
            PagedVec::new(staker_vec, api_request.page_request)
        } else {
            PagedVec::new(
                staker_vec,
                Some(PageRequest {
                    offset: 0,
                    limit: 50,
                }),
            )
        };

        Ok(paged_vec.into())
    }

    async fn get_next_block_best_parents(&self) -> RpcResult<Vec<(BlockId, u64)>> {
        Ok(self.0.consensus_controller.get_best_parents())
    }

    async fn get_version(&self) -> RpcResult<Version> {
        Ok(self.0.version)
    }

    async fn subscribe_new_blocks(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
        broadcast_via_ws(self.0.consensus_channels.block_sender.clone(), pending).await
    }

    async fn subscribe_new_blocks_headers(
        &self,
        pending: PendingSubscriptionSink,
    ) -> SubscriptionResult {
        broadcast_via_ws(
            self.0.consensus_channels.block_header_sender.clone(),
            pending,
        )
        .await
    }

    async fn subscribe_new_filled_blocks(
        &self,
        pending: PendingSubscriptionSink,
    ) -> SubscriptionResult {
        broadcast_via_ws(
            self.0.consensus_channels.filled_block_sender.clone(),
            pending,
        )
        .await
    }

    async fn subscribe_new_operations(
        &self,
        pending: PendingSubscriptionSink,
    ) -> SubscriptionResult {
        broadcast_via_ws(self.0.pool_channels.operation_sender.clone(), pending).await
    }
}

// Brodcast the stream(sender) content via a WebSocket
async fn broadcast_via_ws<T: Serialize + Send + Clone + 'static>(
    sender: tokio::sync::broadcast::Sender<T>,
    pending: PendingSubscriptionSink,
) -> SubscriptionResult {
    let sink = pending.accept().await?;
    let closed = sink.closed();
    let stream = BroadcastStream::new(sender.subscribe());
    futures::pin_mut!(closed, stream);

    loop {
        match future::select(closed, stream.next()).await {
            // subscription closed.
            Either::Left((_, _)) => break Ok(()),

            // received new item from the stream.
            Either::Right((Some(Ok(item)), c)) => {
                let notif = SubscriptionMessage::from_json(&item)?;

                if sink.send(notif).await.is_err() {
                    break Ok(());
                }

                closed = c;
            }

            // Send back back the error.
            Either::Right((Some(Err(e)), _)) => break Err(e.into()),

            // Stream is closed.
            Either::Right((None, _)) => break Ok(()),
        }
    }
}
