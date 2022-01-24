// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::error::ApiError;
use crate::{Endpoints, Private, RpcServer, StopHandle, API};
use jsonrpc_core::BoxFuture;
use jsonrpc_http_server::tokio::sync::mpsc;
use massa_consensus_exports::{ConsensusCommandSender, ConsensusConfig};
use massa_execution::ExecutionCommandSender;
use massa_models::api::{
    APISettings, AddressInfo, BlockInfo, BlockSummary, EndorsementInfo, NodeStatus, OperationInfo,
    TimeInterval,
};
use massa_models::clique::Clique;
use massa_models::composite::PubkeySig;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::{Map, Set};
use massa_models::{Address, Amount, BlockId, EndorsementId, Operation, OperationId, Slot};
use massa_network::NetworkCommandSender;
use massa_signature::PrivateKey;
use std::net::{IpAddr, SocketAddr};

impl API<Private> {
    pub fn new(
        consensus_command_sender: ConsensusCommandSender,
        network_command_sender: NetworkCommandSender,
        execution_command_sender: ExecutionCommandSender,
        api_settings: &'static APISettings,
        consensus_settings: ConsensusConfig,
    ) -> (Self, mpsc::Receiver<()>) {
        let (stop_node_channel, rx) = mpsc::channel(1);
        (
            API(Private {
                consensus_command_sender,
                network_command_sender,
                execution_command_sender,
                consensus_config: consensus_settings,
                api_settings,
                stop_node_channel,
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

    fn add_staking_private_keys(&self, keys: Vec<PrivateKey>) -> BoxFuture<Result<(), ApiError>> {
        let cmd_sender = self.0.consensus_command_sender.clone();
        let closure = async move || Ok(cmd_sender.register_staking_private_keys(keys).await?);
        Box::pin(closure())
    }

    fn execute_read_only_request(
        &self,
        max_gas: u64,
        simulated_gas_price: Amount,
        bytecode: Vec<u8>,
        address: Option<Address>,
    ) -> BoxFuture<Result<ExecuteReadOnlyResponse, ApiError>> {
        let cmd_sender = self.0.execution_command_sender.clone();
        let closure = async move || {
            Ok(cmd_sender
                .execute_read_only_request(max_gas, simulated_gas_price, bytecode, address)
                .await?)
        };
        Box::pin(closure())
    }

    fn remove_staking_addresses(&self, keys: Vec<Address>) -> BoxFuture<Result<(), ApiError>> {
        let cmd_sender = self.0.consensus_command_sender.clone();
        let closure = async move || {
            Ok(cmd_sender
                .remove_staking_addresses(keys.into_iter().collect())
                .await?)
        };
        Box::pin(closure())
    }

    fn get_staking_addresses(&self) -> BoxFuture<Result<Set<Address>, ApiError>> {
        let cmd_sender = self.0.consensus_command_sender.clone();
        let closure = async move || Ok(cmd_sender.get_staking_addresses().await?);
        Box::pin(closure())
    }

    fn ban(&self, ips: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        let network_command_sender = self.0.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.ban_ip(ips).await?);
        Box::pin(closure())
    }

    fn unban(&self, ips: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        let network_command_sender = self.0.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.unban(ips).await?);
        Box::pin(closure())
    }

    fn get_status(&self) -> BoxFuture<Result<NodeStatus, ApiError>> {
        crate::wrong_api::<NodeStatus>()
    }

    fn get_cliques(&self) -> BoxFuture<Result<Vec<Clique>, ApiError>> {
        crate::wrong_api::<Vec<Clique>>()
    }

    fn get_stakers(&self) -> BoxFuture<Result<Map<Address, u64>, ApiError>> {
        crate::wrong_api::<Map<Address, u64>>()
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

    fn get_graph_interval(
        &self,
        _: TimeInterval,
    ) -> BoxFuture<Result<Vec<BlockSummary>, ApiError>> {
        crate::wrong_api::<Vec<BlockSummary>>()
    }

    fn get_addresses(&self, _: Vec<Address>) -> BoxFuture<Result<Vec<AddressInfo>, ApiError>> {
        crate::wrong_api::<Vec<AddressInfo>>()
    }

    fn send_operations(&self, _: Vec<Operation>) -> BoxFuture<Result<Vec<OperationId>, ApiError>> {
        crate::wrong_api::<Vec<OperationId>>()
    }

    fn get_sc_output_event_by_slot_range(
        &self,
        _: Slot,
        _: Slot,
    ) -> BoxFuture<Result<Vec<SCOutputEvent>, ApiError>> {
        crate::wrong_api::<Vec<SCOutputEvent>>()
    }

    fn get_sc_output_event_by_sc_address(
        &self,
        _: Address,
    ) -> BoxFuture<Result<Vec<SCOutputEvent>, ApiError>> {
        crate::wrong_api::<Vec<SCOutputEvent>>()
    }

    fn get_sc_output_event_by_caller_address(
        &self,
        _: Address,
    ) -> BoxFuture<Result<Vec<SCOutputEvent>, ApiError>> {
        crate::wrong_api::<Vec<SCOutputEvent>>()
    }
}
