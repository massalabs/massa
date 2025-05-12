//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{MassaRpcServer, Private, RpcServer, StopHandle, Value, API};

use async_trait::async_trait;
use jsonrpsee::core::{client::Error as JsonRpseeError, RpcResult};
use massa_api_exports::{
    address::{AddressFilter, AddressInfo},
    block::{BlockInfo, BlockSummary},
    config::APIConfig,
    datastore::{DatastoreEntryInput, DatastoreEntryOutput},
    endorsement::EndorsementInfo,
    error::ApiError,
    execution::{
        DeferredCallResponse, DeferredCallsQuoteRequest, DeferredCallsQuoteResponse,
        DeferredCallsSlotResponse, ExecuteReadOnlyResponse, ReadOnlyBytecodeExecution,
        ReadOnlyCall, Transfer,
    },
    node::NodeStatus,
    operation::{OperationInfo, OperationInput},
    page::{PageRequest, PagedVec},
    ListType, ScrudOperation, TimeInterval,
};
use massa_execution_exports::ExecutionController;
use massa_hash::Hash;
use massa_models::{
    address::Address, block::Block, block_id::BlockId, clique::Clique, composite::PubkeySig,
    endorsement::EndorsementId, execution::EventFilter, node::NodeId, operation::OperationId,
    output_event::SCOutputEvent, prehash::PreHashSet, slot::Slot,
};
use massa_protocol_exports::{PeerId, ProtocolController};
use massa_signature::KeyPair;
use massa_wallet::Wallet;
use parking_lot::RwLock;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::{collections::BTreeSet, sync::Mutex};
use std::{
    fs::{remove_file, OpenOptions},
    sync::Condvar,
};

impl API<Private> {
    /// generate a new private API
    pub fn new(
        protocol_controller: Box<dyn ProtocolController>,
        execution_controller: Box<dyn ExecutionController>,
        api_settings: APIConfig,
        stop_cv: Arc<(Mutex<bool>, Condvar)>,
        node_wallet: Arc<RwLock<Wallet>>,
    ) -> Self {
        API(Private {
            protocol_controller,
            execution_controller,
            api_settings,
            stop_cv,
            node_wallet,
        })
    }
}

#[async_trait]
impl RpcServer for API<Private> {
    async fn serve(
        self,
        url: &SocketAddr,
        settings: &APIConfig,
    ) -> Result<StopHandle, JsonRpseeError> {
        crate::serve(self.into_rpc(), url, settings).await
    }
}

#[doc(hidden)]
#[async_trait]
impl MassaRpcServer for API<Private> {
    fn stop_node(&self) -> RpcResult<()> {
        *self.0.stop_cv.0.lock().expect("twice-locked in-thread") = true;
        self.0.stop_cv.1.notify_all();
        Ok(())
    }

    async fn node_sign_message(&self, message: Vec<u8>) -> RpcResult<PubkeySig> {
        let signature = match self
            .0
            .api_settings
            .keypair
            .sign(&Hash::compute_from(&message))
        {
            Ok(signature) => signature,
            Err(e) => {
                return Err(
                    ApiError::InconsistencyError(format!("error signing message: {}", e)).into(),
                );
            }
        };
        Ok(PubkeySig {
            public_key: self.0.api_settings.keypair.get_public_key(),
            signature,
        })
    }

    async fn add_staking_secret_keys(&self, secret_keys: Vec<String>) -> RpcResult<()> {
        let keypairs = match secret_keys.iter().map(|x| KeyPair::from_str(x)).collect() {
            Ok(keypairs) => keypairs,
            Err(e) => return Err(ApiError::BadRequest(e.to_string()).into()),
        };

        let node_wallet = self.0.node_wallet.clone();
        let mut w_wallet = node_wallet.write();
        w_wallet
            .add_keypairs(keypairs)
            .map(|_| ())
            .map_err(|e| ApiError::WalletError(e).into())
    }

    async fn execute_read_only_bytecode(
        &self,
        _reqs: Vec<ReadOnlyBytecodeExecution>,
    ) -> RpcResult<Vec<ExecuteReadOnlyResponse>> {
        crate::wrong_api::<_>()
    }

    async fn execute_read_only_call(
        &self,
        _reqs: Vec<ReadOnlyCall>,
    ) -> RpcResult<Vec<ExecuteReadOnlyResponse>> {
        crate::wrong_api::<_>()
    }

    async fn remove_staking_addresses(&self, addresses: Vec<Address>) -> RpcResult<()> {
        let node_wallet = self.0.node_wallet.clone();

        let changed = {
            node_wallet
                .write()
                .remove_addresses(&addresses)
                .map_err(ApiError::WalletError)?
        };

        if changed {
            node_wallet.read().save().map_err(ApiError::WalletError)?;
        }
        Ok(())
    }

    async fn get_staking_addresses(&self) -> RpcResult<PreHashSet<Address>> {
        let node_wallet = self.0.node_wallet.clone();
        let w_wallet = node_wallet.read();
        Ok(w_wallet.get_wallet_address_list())
    }

    async fn node_ban_by_ip(&self, _ips: Vec<IpAddr>) -> RpcResult<()> {
        //TODO: Reinvoke
        // let network_command_sender = self.0.network_command_sender.clone();
        // network_command_sender
        //    .node_ban_by_ips(ips)
        //    .await
        //    .map_err(|e| ApiError::NetworkError(e).into())
        return Err(
            ApiError::BadRequest("This request is currently not available".to_string()).into(),
        );
    }

    async fn node_ban_by_id(&self, ids: Vec<NodeId>) -> RpcResult<()> {
        let protocol_controller = self.0.protocol_controller.clone();
        //TODO: Change when unify node id and peer id
        let peer_ids = ids
            .into_iter()
            .map(|id| PeerId::from_public_key(id.get_public_key()))
            .collect();
        protocol_controller
            .ban_peers(peer_ids)
            .map_err(|e| ApiError::ProtocolError(e.to_string()).into())
    }

    async fn node_unban_by_id(&self, ids: Vec<NodeId>) -> RpcResult<()> {
        let protocol_controller = self.0.protocol_controller.clone();
        //TODO: Change when unify node id and peer id
        let peer_ids = ids
            .into_iter()
            .map(|id| PeerId::from_public_key(id.get_public_key()))
            .collect();
        protocol_controller
            .unban_peers(peer_ids)
            .map_err(|e| ApiError::ProtocolError(e.to_string()).into())
    }

    async fn node_unban_by_ip(&self, _ips: Vec<IpAddr>) -> RpcResult<()> {
        //TODO: Reinvoke
        // let network_command_sender = self.0.network_command_sender.clone();
        // network_command_sender
        //     .node_unban_ips(ips)
        //     .await
        //     .map_err(|e| ApiError::NetworkError(e).into())
        return Err(
            ApiError::BadRequest("This request is currently not available".to_string()).into(),
        );
    }

    async fn get_slots_transfers(&self, _: Vec<Slot>) -> RpcResult<Vec<Vec<Transfer>>> {
        crate::wrong_api::<Vec<Vec<Transfer>>>()
    }

    async fn get_status(&self) -> RpcResult<NodeStatus> {
        crate::wrong_api::<NodeStatus>()
    }

    async fn get_cliques(&self) -> RpcResult<Vec<Clique>> {
        crate::wrong_api::<Vec<Clique>>()
    }

    async fn get_stakers(&self, _: Option<PageRequest>) -> RpcResult<PagedVec<(Address, u64)>> {
        crate::wrong_api::<PagedVec<(Address, u64)>>()
    }

    async fn get_operations(&self, _: Vec<OperationId>) -> RpcResult<Vec<OperationInfo>> {
        crate::wrong_api::<Vec<OperationInfo>>()
    }

    async fn get_endorsements(&self, _: Vec<EndorsementId>) -> RpcResult<Vec<EndorsementInfo>> {
        crate::wrong_api::<Vec<EndorsementInfo>>()
    }

    async fn get_blocks(&self, _: Vec<BlockId>) -> RpcResult<Vec<BlockInfo>> {
        crate::wrong_api::<Vec<BlockInfo>>()
    }

    async fn get_blockclique_block_by_slot(&self, _: Slot) -> RpcResult<Option<Block>> {
        crate::wrong_api::<Option<Block>>()
    }

    async fn get_graph_interval(&self, _: TimeInterval) -> RpcResult<Vec<BlockSummary>> {
        crate::wrong_api::<Vec<BlockSummary>>()
    }

    async fn get_datastore_entries(
        &self,
        _: Vec<DatastoreEntryInput>,
    ) -> RpcResult<Vec<DatastoreEntryOutput>> {
        crate::wrong_api()
    }

    async fn get_addresses(&self, _: Vec<Address>) -> RpcResult<Vec<AddressInfo>> {
        crate::wrong_api::<Vec<AddressInfo>>()
    }

    async fn get_addresses_bytecode(&self, _: Vec<AddressFilter>) -> RpcResult<Vec<Vec<u8>>> {
        crate::wrong_api::<Vec<Vec<u8>>>()
    }

    async fn send_operations(&self, _: Vec<OperationInput>) -> RpcResult<Vec<OperationId>> {
        crate::wrong_api::<Vec<OperationId>>()
    }

    async fn get_filtered_sc_output_event(&self, _: EventFilter) -> RpcResult<Vec<SCOutputEvent>> {
        crate::wrong_api::<Vec<SCOutputEvent>>()
    }

    async fn node_peers_whitelist(&self) -> RpcResult<Vec<IpAddr>> {
        //TODO: Reinvoke
        // let network_command_sender = self.0.network_command_sender.clone();
        // match network_command_sender.get_peers().await {
        //     Ok(peers) => Ok(peers.peers.into_keys().sorted().collect::<Vec<IpAddr>>()),
        //     Err(e) => Err(ApiError::NetworkError(e).into()),
        // }
        return Err(
            ApiError::BadRequest("This request is currently not available".to_string()).into(),
        );
    }

    async fn node_add_to_peers_whitelist(&self, _ips: Vec<IpAddr>) -> RpcResult<()> {
        //TODO: Readd in network refactoring
        // let network_command_sender = self.0.network_command_sender.clone();
        // network_command_sender
        //     .add_to_whitelist(ips)
        //     .await
        //     .map_err(|e| ApiError::NetworkError(e).into())
        return Err(
            ApiError::BadRequest("This request is currently not available".to_string()).into(),
        );
    }

    async fn node_remove_from_peers_whitelist(&self, _ips: Vec<IpAddr>) -> RpcResult<()> {
        //TODO: Reinvoke
        //TODO: Readd in network refactoring
        // let network_command_sender = self.0.network_command_sender.clone();
        // network_command_sender
        //     .remove_from_whitelist(ips)
        //     .await
        //     .map_err(|e| ApiError::NetworkError(e).into())
        return Err(
            ApiError::BadRequest("This request is currently not available".to_string()).into(),
        );
    }

    async fn node_bootstrap_whitelist(&self) -> RpcResult<Vec<IpAddr>> {
        read_ips_from_jsonfile(
            self.0.api_settings.bootstrap_whitelist_path.clone(),
            &ListType::Whitelist,
        )
    }

    async fn node_bootstrap_whitelist_allow_all(&self) -> RpcResult<()> {
        remove_file(self.0.api_settings.bootstrap_whitelist_path.clone()).map_err(|e| {
            ApiError::InternalServerError(format!(
                "failed to delete bootstrap whitelist configuration file: {}",
                e
            ))
            .into()
        })
    }

    async fn node_add_to_bootstrap_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        run_scrud_operation(
            self.0.api_settings.bootstrap_whitelist_path.clone(),
            ips,
            ListType::Whitelist,
            ScrudOperation::Create,
        )
    }

    async fn node_remove_from_bootstrap_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        run_scrud_operation(
            self.0.api_settings.bootstrap_whitelist_path.clone(),
            ips,
            ListType::Whitelist,
            ScrudOperation::Delete,
        )
    }

    async fn node_bootstrap_blacklist(&self) -> RpcResult<Vec<IpAddr>> {
        read_ips_from_jsonfile(
            self.0.api_settings.bootstrap_blacklist_path.clone(),
            &ListType::Blacklist,
        )
    }

    async fn node_add_to_bootstrap_blacklist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        run_scrud_operation(
            self.0.api_settings.bootstrap_blacklist_path.clone(),
            ips,
            ListType::Blacklist,
            ScrudOperation::Create,
        )
    }

    async fn node_remove_from_bootstrap_blacklist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        run_scrud_operation(
            self.0.api_settings.bootstrap_blacklist_path.clone(),
            ips,
            ListType::Blacklist,
            ScrudOperation::Delete,
        )
    }

    async fn get_deferred_call_quote(
        &self,
        _req: Vec<DeferredCallsQuoteRequest>,
    ) -> RpcResult<Vec<DeferredCallsQuoteResponse>> {
        crate::wrong_api::<Vec<DeferredCallsQuoteResponse>>()
    }
    async fn get_deferred_call_info(
        &self,
        _arg: Vec<String>,
    ) -> RpcResult<Vec<DeferredCallResponse>> {
        crate::wrong_api::<Vec<DeferredCallResponse>>()
    }

    async fn get_deferred_call_ids_by_slot(
        &self,
        _slot: Vec<Slot>,
    ) -> RpcResult<Vec<DeferredCallsSlotResponse>> {
        crate::wrong_api::<Vec<DeferredCallsSlotResponse>>()
    }

    async fn get_openrpc_spec(&self) -> RpcResult<Value> {
        crate::wrong_api::<Value>()
    }
}

/// Run Search, Create, Read, Update, Delete operation on bootstrap list of IP(s)
fn run_scrud_operation(
    bootstrap_list_file: PathBuf,
    ips: Vec<IpAddr>,
    list_type: ListType,
    scrud_operation: ScrudOperation,
) -> RpcResult<()> {
    match scrud_operation {
        ScrudOperation::Create => get_file_len(bootstrap_list_file.clone(), &list_type, true)
            .and_then(|length| {
                if length == 0 {
                    write_ips_to_jsonfile(bootstrap_list_file, BTreeSet::from_iter(ips), &list_type)
                } else {
                    read_ips_from_jsonfile(bootstrap_list_file.clone(), &list_type)
                        .map(BTreeSet::from_iter)
                        .and_then(|mut list_ips: BTreeSet<IpAddr>| {
                            list_ips.extend(ips);
                            write_ips_to_jsonfile(bootstrap_list_file, list_ips, &list_type)
                        })
                }
            }),
        ScrudOperation::Delete => get_file_len(bootstrap_list_file.clone(), &list_type, false)
            .and_then(|length| {
                if length == 0 {
                    Err(ApiError::InternalServerError(format!(
                        "failed, bootstrap {} configuration file is empty",
                        list_type
                    ))
                    .into())
                } else {
                    read_ips_from_jsonfile(bootstrap_list_file.clone(), &list_type)
                        .map(BTreeSet::from_iter)
                        .and_then(|mut list_ips: BTreeSet<IpAddr>| {
                            if list_ips.is_empty() {
                                return Err(ApiError::InternalServerError(format!(
                                    "failed to execute delete operation, bootstrap {} is empty",
                                    list_type
                                ))
                                .into());
                            }
                            ips.into_iter().for_each(|ip| {
                                list_ips.remove(&ip);
                            });
                            write_ips_to_jsonfile(bootstrap_list_file, list_ips, &list_type)
                        })
                }
            }),
        _ => Err(ApiError::BadRequest(format!(
            "failed operation {} is not supported on {}",
            list_type, scrud_operation
        ))
        .into()),
    }
}

/// Get length of the given file if it exists(or create it if requested)
fn get_file_len(
    bootstrap_list_file: PathBuf,
    list_type: &ListType,
    create: bool,
) -> RpcResult<u64> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .open(bootstrap_list_file)
        .map_err(|e| {
            ApiError::InternalServerError(format!(
                "failed to read bootstrap {} configuration file: {}",
                list_type, e
            ))
            .into()
        })
        .and_then(|file| match file.metadata() {
            Ok(metadata) => Ok(metadata.len()),
            Err(e) => Err(ApiError::InternalServerError(format!(
                "failed to read bootstrap {} configuration file metadata: {}",
                list_type, e
            ))
            .into()),
        })
}

/// Read bootstrap list IP(s) from json file
fn read_ips_from_jsonfile(
    bootstrap_list_file: PathBuf,
    list_type: &ListType,
) -> RpcResult<Vec<IpAddr>> {
    std::fs::read_to_string(bootstrap_list_file)
        .map_err(|e| {
            ApiError::InternalServerError(format!(
                "failed to read bootstrap {} configuration file: {}",
                list_type, e
            ))
            .into()
        })
        .and_then(|bootstrap_list_str| {
            serde_json::from_str(&bootstrap_list_str).map_err(|e| {
                ApiError::InternalServerError(format!(
                    "failed to parse bootstrap {} configuration file: {}",
                    list_type, e
                ))
                .into()
            })
        })
}

/// Write bootstrap list IP(s) from json file
fn write_ips_to_jsonfile(
    bootstrap_list_file: PathBuf,
    ips: BTreeSet<IpAddr>,
    list_type: &ListType,
) -> RpcResult<()> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(bootstrap_list_file)
        .map_err(|e| {
            ApiError::InternalServerError(format!(
                "failed to create bootstrap {} configuration file: {}",
                list_type, e
            ))
            .into()
        })
        .and_then(|file| {
            serde_json::to_writer_pretty(file, &ips).map_err(|e| {
                ApiError::InternalServerError(format!(
                    "failed to write bootstrap {} configuration file: {}",
                    list_type, e
                ))
                .into()
            })
        })
}
