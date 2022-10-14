//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::config::APIConfig;
use crate::error::ApiError;
use crate::{Endpoints, Private, RpcServer, StopHandle, API};

use jsonrpc_core::BoxFuture;
use jsonrpc_http_server::tokio::sync::mpsc;

use massa_consensus_exports::{ConsensusCommandSender, ConsensusConfig};
use massa_execution_exports::ExecutionController;
use massa_models::api::{
    AddressInfo, BlockInfo, BlockSummary, DatastoreEntryInput, DatastoreEntryOutput,
    EndorsementInfo, EventFilter, NodeStatus, OperationInfo, OperationInput,
    ReadOnlyBytecodeExecution, ReadOnlyCall, TimeInterval,
};
use massa_models::clique::Clique;
use massa_models::composite::PubkeySig;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::node::NodeId;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::PreHashSet;
use massa_models::{
    address::Address,
    block::{Block, BlockId},
    endorsement::EndorsementId,
    operation::OperationId,
    slot::Slot,
};
use massa_network_exports::NetworkCommandSender;
use massa_signature::KeyPair;
use massa_wallet::Wallet;

use parking_lot::RwLock;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

impl API<Private> {
    /// generate a new private API
    pub fn new(
        consensus_command_sender: ConsensusCommandSender,
        network_command_sender: NetworkCommandSender,
        execution_controller: Box<dyn ExecutionController>,
        api_settings: APIConfig,
        consensus_settings: ConsensusConfig,
        node_wallet: Arc<RwLock<Wallet>>,
    ) -> (Self, mpsc::Receiver<()>) {
        let (stop_node_channel, rx) = mpsc::channel(1);
        (
            API(Private {
                consensus_command_sender,
                network_command_sender,
                execution_controller,
                consensus_config: consensus_settings,
                api_settings,
                stop_node_channel,
                node_wallet,
            }),
            rx,
        )
    }
}

impl RpcServer for API<Private> {
    fn serve(self, url: &SocketAddr) -> StopHandle {
        crate::serve(self, url)
    }
}

#[doc(hidden)]
impl Endpoints for API<Private> {
    fn stop_node(&self) -> BoxFuture<Result<(), ApiError>> {
        let stop = self.0.stop_node_channel.clone();
        let closure = async move || {
            stop.send(()).await.map_err(|e| {
                ApiError::SendChannelError(format!("error sending stop signal {}", e))
            })?;
            Ok(())
        };
        Box::pin(closure())
    }

    fn node_sign_message(&self, message: Vec<u8>) -> BoxFuture<Result<PubkeySig, ApiError>> {
        let network_command_sender = self.0.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.node_sign_message(message).await?);
        Box::pin(closure())
    }

    fn add_staking_secret_keys(&self, secret_keys: Vec<String>) -> BoxFuture<Result<(), ApiError>> {
        let keypairs = match secret_keys.iter().map(|x| KeyPair::from_str(x)).collect() {
            Ok(keypairs) => keypairs,
            Err(e) => {
                let closure = async move || Err(ApiError::BadRequest(e.to_string()));
                return Box::pin(closure());
            }
        };
        let node_wallet = self.0.node_wallet.clone();
        let closure = async move || {
            let mut w_wallet = node_wallet.write();
            w_wallet.add_keypairs(keypairs)?;
            Ok(())
        };
        Box::pin(closure())
    }

    fn execute_read_only_bytecode(
        &self,
        _reqs: Vec<ReadOnlyBytecodeExecution>,
    ) -> BoxFuture<Result<Vec<ExecuteReadOnlyResponse>, ApiError>> {
        crate::wrong_api::<_>()
    }

    fn execute_read_only_call(
        &self,
        _reqs: Vec<ReadOnlyCall>,
    ) -> BoxFuture<Result<Vec<ExecuteReadOnlyResponse>, ApiError>> {
        crate::wrong_api::<_>()
    }

    fn remove_staking_addresses(&self, addresses: Vec<Address>) -> BoxFuture<Result<(), ApiError>> {
        let node_wallet = self.0.node_wallet.clone();
        let closure = async move || {
            let mut w_wallet = node_wallet.write();
            w_wallet.remove_addresses(&addresses)?;
            Ok(())
        };
        Box::pin(closure())
    }

    fn get_staking_addresses(&self) -> BoxFuture<Result<PreHashSet<Address>, ApiError>> {
        let node_wallet = self.0.node_wallet.clone();
        let closure = async move || Ok(node_wallet.write().get_wallet_address_list());
        Box::pin(closure())
    }

    fn node_ban_by_ip(&self, ips: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        let network_command_sender = self.0.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.node_ban_by_ips(ips).await?);
        Box::pin(closure())
    }

    fn node_ban_by_id(&self, ids: Vec<NodeId>) -> BoxFuture<Result<(), ApiError>> {
        let network_command_sender = self.0.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.node_ban_by_ids(ids).await?);
        Box::pin(closure())
    }

    fn node_unban_by_id(&self, ids: Vec<NodeId>) -> BoxFuture<Result<(), ApiError>> {
        let network_command_sender = self.0.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.node_unban_by_ids(ids).await?);
        Box::pin(closure())
    }

    fn node_unban_by_ip(&self, ips: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        let network_command_sender = self.0.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.node_unban_ips(ips).await?);
        Box::pin(closure())
    }

    fn get_status(&self) -> BoxFuture<Result<NodeStatus, ApiError>> {
        crate::wrong_api::<NodeStatus>()
    }

    fn get_cliques(&self) -> BoxFuture<Result<Vec<Clique>, ApiError>> {
        crate::wrong_api::<Vec<Clique>>()
    }

    fn get_stakers(&self) -> BoxFuture<Result<Vec<(Address, u64)>, ApiError>> {
        crate::wrong_api::<Vec<(Address, u64)>>()
    }

    fn get_operations(
        &self,
        _: Vec<OperationId>,
    ) -> BoxFuture<Result<Vec<OperationInfo>, ApiError>> {
        crate::wrong_api::<Vec<OperationInfo>>()
    }

    fn get_endorsements(
        &self,
        _: Vec<EndorsementId>,
    ) -> BoxFuture<Result<Vec<EndorsementInfo>, ApiError>> {
        crate::wrong_api::<Vec<EndorsementInfo>>()
    }

    fn get_block(&self, _: BlockId) -> BoxFuture<Result<BlockInfo, ApiError>> {
        crate::wrong_api::<BlockInfo>()
    }

    fn get_blockclique_block_by_slot(&self, _: Slot) -> BoxFuture<Result<Option<Block>, ApiError>> {
        crate::wrong_api::<Option<Block>>()
    }

    fn get_graph_interval(
        &self,
        _: TimeInterval,
    ) -> BoxFuture<Result<Vec<BlockSummary>, ApiError>> {
        crate::wrong_api::<Vec<BlockSummary>>()
    }

    fn get_datastore_entries(
        &self,
        _: Vec<DatastoreEntryInput>,
    ) -> BoxFuture<Result<Vec<DatastoreEntryOutput>, ApiError>> {
        crate::wrong_api()
    }

    fn get_addresses(&self, _: Vec<Address>) -> BoxFuture<Result<Vec<AddressInfo>, ApiError>> {
        crate::wrong_api::<Vec<AddressInfo>>()
    }

    fn send_operations(
        &self,
        _: Vec<OperationInput>,
    ) -> BoxFuture<Result<Vec<OperationId>, ApiError>> {
        crate::wrong_api::<Vec<OperationId>>()
    }

    fn get_filtered_sc_output_event(
        &self,
        _: EventFilter,
    ) -> BoxFuture<Result<Vec<SCOutputEvent>, ApiError>> {
        crate::wrong_api::<Vec<SCOutputEvent>>()
    }

    fn node_whitelist(&self, ips: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        let network_command_sender = self.0.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.whitelist(ips).await?);
        Box::pin(closure())
    }

    fn node_remove_from_whitelist(&self, ips: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        let network_command_sender = self.0.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.remove_from_whitelist(ips).await?);
        Box::pin(closure())
    }
}
