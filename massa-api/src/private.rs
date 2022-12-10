//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::config::APIConfig;
use crate::error::ApiError;
use crate::{MassaRpcServer, Private, RpcServer, StopHandle, Value, API};

use async_trait::async_trait;
use jsonrpsee::core::{Error as JsonRpseeError, RpcResult};
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
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::BufReader;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::mpsc;

impl API<Private> {
    /// generate a new private API
    pub fn new(
        network_command_sender: NetworkCommandSender,
        execution_controller: Box<dyn ExecutionController>,
        api_settings: APIConfig,
        node_wallet: Arc<RwLock<Wallet>>,
    ) -> (Self, mpsc::Receiver<()>) {
        let (stop_node_channel, rx) = mpsc::channel(1);
        (
            API(Private {
                network_command_sender,
                execution_controller,
                api_settings,
                stop_node_channel,
                node_wallet,
            }),
            rx,
        )
    }
}

#[async_trait]
impl RpcServer for API<Private> {
    async fn serve(
        self,
        url: &SocketAddr,
        settings: &APIConfig,
    ) -> Result<StopHandle, JsonRpseeError> {
        crate::serve(self, url, settings).await
    }
}

#[doc(hidden)]
#[async_trait]
impl MassaRpcServer for API<Private> {
    async fn stop_node(&self) -> RpcResult<()> {
        let stop = self.0.stop_node_channel.clone();
        stop.send(())
            .await
            .map_err(|e| ApiError::SendChannelError(format!("error sending stop signal {}", e)))?;
        Ok(())
    }

    async fn node_sign_message(&self, message: Vec<u8>) -> RpcResult<PubkeySig> {
        let network_command_sender = self.0.network_command_sender.clone();
        match network_command_sender.node_sign_message(message).await {
            Ok(public_key_signature) => return Ok(public_key_signature),
            Err(e) => return Err(ApiError::from(e).into()),
        };
    }

    async fn add_staking_secret_keys(&self, secret_keys: Vec<String>) -> RpcResult<()> {
        let keypairs = match secret_keys.iter().map(|x| KeyPair::from_str(x)).collect() {
            Ok(keypairs) => keypairs,
            Err(e) => return Err(ApiError::BadRequest(e.to_string()).into()),
        };

        let node_wallet = self.0.node_wallet.clone();
        let mut w_wallet = node_wallet.write();
        match w_wallet.add_keypairs(keypairs) {
            Ok(_) => return Ok(()),
            Err(e) => return Err(ApiError::from(e).into()),
        };
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
        let mut w_wallet = node_wallet.write();
        match w_wallet.remove_addresses(&addresses) {
            Ok(()) => return Ok(()),
            Err(e) => return Err(ApiError::from(e).into()),
        };
    }

    async fn get_staking_addresses(&self) -> RpcResult<PreHashSet<Address>> {
        let node_wallet = self.0.node_wallet.clone();
        let addresses_set = node_wallet.write().get_wallet_address_list();
        Ok(addresses_set)
    }

    async fn node_ban_by_ip(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        let network_command_sender = self.0.network_command_sender.clone();
        match network_command_sender.node_ban_by_ips(ips).await {
            Ok(()) => return Ok(()),
            Err(e) => return Err(ApiError::from(e).into()),
        };
    }

    async fn node_ban_by_id(&self, ids: Vec<NodeId>) -> RpcResult<()> {
        let network_command_sender = self.0.network_command_sender.clone();
        match network_command_sender.node_ban_by_ids(ids).await {
            Ok(()) => return Ok(()),
            Err(e) => return Err(ApiError::from(e).into()),
        };
    }

    async fn node_unban_by_id(&self, ids: Vec<NodeId>) -> RpcResult<()> {
        let network_command_sender = self.0.network_command_sender.clone();
        match network_command_sender.node_unban_by_ids(ids).await {
            Ok(()) => return Ok(()),
            Err(e) => return Err(ApiError::from(e).into()),
        };
    }

    async fn node_unban_by_ip(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        let network_command_sender = self.0.network_command_sender.clone();
        match network_command_sender.node_unban_ips(ips).await {
            Ok(()) => return Ok(()),
            Err(e) => return Err(ApiError::from(e).into()),
        };
    }

    async fn get_status(&self) -> RpcResult<NodeStatus> {
        crate::wrong_api::<NodeStatus>()
    }

    async fn get_cliques(&self) -> RpcResult<Vec<Clique>> {
        crate::wrong_api::<Vec<Clique>>()
    }

    async fn get_stakers(&self) -> RpcResult<Vec<(Address, u64)>> {
        crate::wrong_api::<Vec<(Address, u64)>>()
    }

    async fn get_operations(&self, _: Vec<OperationId>) -> RpcResult<Vec<OperationInfo>> {
        crate::wrong_api::<Vec<OperationInfo>>()
    }

    async fn get_endorsements(&self, _: Vec<EndorsementId>) -> RpcResult<Vec<EndorsementInfo>> {
        crate::wrong_api::<Vec<EndorsementInfo>>()
    }

    async fn get_block(&self, _: BlockId) -> RpcResult<BlockInfo> {
        crate::wrong_api::<BlockInfo>()
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

    async fn send_operations(&self, _: Vec<OperationInput>) -> RpcResult<Vec<OperationId>> {
        crate::wrong_api::<Vec<OperationId>>()
    }

    async fn get_filtered_sc_output_event(&self, _: EventFilter) -> RpcResult<Vec<SCOutputEvent>> {
        crate::wrong_api::<Vec<SCOutputEvent>>()
    }

    async fn node_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        let network_command_sender = self.0.network_command_sender.clone();
        match network_command_sender.whitelist(ips).await {
            Ok(()) => return Ok(()),
            Err(e) => return Err(ApiError::from(e).into()),
        };
    }

    async fn node_remove_from_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        let network_command_sender = self.0.network_command_sender.clone();
        match network_command_sender.remove_from_whitelist(ips).await {
            Ok(()) => return Ok(()),
            Err(e) => return Err(ApiError::from(e).into()),
        };
    }

    async fn node_bootstrap_whitelist(&self) -> RpcResult<Vec<IpAddr>> {
        std::fs::read_to_string(self.0.api_settings.bootstrap_whitelist_file.clone())
            .map_err(|e| {
                ApiError::InternalServerError(format!(
                    "failed to read bootsrap whitelist configuration file: {}",
                    e
                ))
                .into()
            })
            .and_then(|bootsrap_whitelist_str| {
                serde_json::from_str(&bootsrap_whitelist_str).map_err(|e| {
                    ApiError::InternalServerError(format!(
                        "failed to parse bootsrap whitelist configuration file: {}",
                        e
                    ))
                    .into()
                })
            })
    }

    async fn node_add_to_bootstrap_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(self.0.api_settings.bootstrap_whitelist_file.clone())
            .map_err(|e| {
                ApiError::InternalServerError(format!(
                    "failed to read bootsrap whitelist configuration file: {}",
                    e
                ))
                .into()
            })
            .and_then(|bootsrap_whitelist_file| {
                let len = match bootsrap_whitelist_file.metadata() {
                    Ok(metadata) => metadata.len(),
                    Err(e) => {
                        return Err(ApiError::InternalServerError(format!(
                            "failed to read bootsrap whitelist configuration file metadata: {}",
                            e
                        ))
                        .into())
                    }
                };
                if len == 0 {
                    OpenOptions::new()
                        .write(true)
                        .open(self.0.api_settings.bootstrap_whitelist_file.clone())
                        .map_err(|e| {
                            ApiError::InternalServerError(format!(
                                "failed to create bootsrap whitelist configuration file: {}",
                                e
                            ))
                            .into()
                        })
                        .and_then(|whitelist_file| {
                            let ips_set: BTreeSet<IpAddr> = BTreeSet::from_iter(ips);
                            serde_json::to_writer_pretty(whitelist_file, &ips_set).map_err(|e| {
                                ApiError::InternalServerError(format!(
                                    "failed to write bootsrap whitelist configuration file: {}",
                                    e
                                ))
                                .into()
                            })
                        })
                } else {
                    serde_json::from_reader::<BufReader<File>, BTreeSet<IpAddr>>(BufReader::new(
                        bootsrap_whitelist_file,
                    ))
                    .map_err(|e| {
                        ApiError::InternalServerError(format!(
                            "failed to parse bootsrap whitelist configuration file: {}",
                            e
                        ))
                        .into()
                    })
                    .and_then(|mut whitelist_ips: BTreeSet<IpAddr>| {
                        whitelist_ips.extend(ips);
                        OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(self.0.api_settings.bootstrap_whitelist_file.clone())
                            .map_err(|e| {
                                ApiError::InternalServerError(format!(
                                    "failed to create bootsrap whitelist configuration file: {}",
                                    e
                                ))
                                .into()
                            })
                            .and_then(|whitelist_file| {
                                serde_json::to_writer_pretty(whitelist_file, &whitelist_ips)
                                    .map_err(|e| {
                                        ApiError::InternalServerError(format!(
                                    "failed to write bootsrap whitelist configuration file: {}",
                                    e
                                ))
                                        .into()
                                    })
                            })
                    })
                }
            })
    }

    async fn node_remove_from_bootstrap_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(self.0.api_settings.bootstrap_whitelist_file.clone())
            .map_err(|e| {
                ApiError::InternalServerError(format!(
                    "failed to read bootsrap whitelist configuration file: {}",
                    e
                ))
                .into()
            })
            .and_then(|bootsrap_whitelist_file| {
                let len = match bootsrap_whitelist_file.metadata() {
                    Ok(metadata) => metadata.len(),
                    Err(e) => {
                        return Err(ApiError::InternalServerError(format!(
                            "failed to read bootsrap whitelist configuration file metadata: {}",
                            e
                        ))
                        .into())
                    }
                };
                if len == 0 {
                    Err(ApiError::BadRequest(
                        "failed, bootsrap whitelist configuration file is empty".to_string(),
                    )
                    .into())
                } else {
                    serde_json::from_reader::<BufReader<File>, BTreeSet<IpAddr>>(BufReader::new(
                        bootsrap_whitelist_file,
                    ))
                    .map_err(|e| {
                        ApiError::InternalServerError(format!(
                            "failed to parse bootsrap whitelist configuration file: {}",
                            e
                        ))
                        .into()
                    })
                    .and_then(|mut whitelist_ips: BTreeSet<IpAddr>| {
                        ips.into_iter().for_each(|ip| {
                            whitelist_ips.remove(&ip);
                        });
                        OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(self.0.api_settings.bootstrap_whitelist_file.clone())
                            .map_err(|e| {
                                ApiError::InternalServerError(format!(
                                    "failed to create bootsrap whitelist configuration file: {}",
                                    e
                                ))
                                .into()
                            })
                            .and_then(|whitelist_file| {
                                serde_json::to_writer_pretty(whitelist_file, &whitelist_ips)
                                    .map_err(|e| {
                                        ApiError::InternalServerError(format!(
                                    "failed to write bootsrap whitelist configuration file: {}",
                                    e
                                ))
                                        .into()
                                    })
                            })
                    })
                }
            })
    }

    async fn node_bootstrap_blacklist(&self) -> RpcResult<Vec<IpAddr>> {
        std::fs::read_to_string(self.0.api_settings.bootstrap_blacklist_file.clone())
            .map_err(|e| {
                ApiError::InternalServerError(format!(
                    "failed to read bootsrap blacklist configuration file: {}",
                    e
                ))
                .into()
            })
            .and_then(|bootsrap_blacklist_str| {
                serde_json::from_str(&bootsrap_blacklist_str).map_err(|e| {
                    ApiError::InternalServerError(format!(
                        "failed to parse bootsrap blacklist configuration file: {}",
                        e
                    ))
                    .into()
                })
            })
    }

    async fn node_add_to_bootstrap_blacklist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(self.0.api_settings.bootstrap_blacklist_file.clone())
            .map_err(|e| {
                ApiError::InternalServerError(format!(
                    "failed to read bootsrap blacklist configuration file: {}",
                    e
                ))
                .into()
            })
            .and_then(|bootsrap_blacklist_file| {
                let len = match bootsrap_blacklist_file.metadata() {
                    Ok(metadata) => metadata.len(),
                    Err(e) => {
                        return Err(ApiError::InternalServerError(format!(
                            "failed to read bootsrap blacklist configuration file metadata: {}",
                            e
                        ))
                        .into())
                    }
                };
                if len == 0 {
                    OpenOptions::new()
                        .write(true)
                        .open(self.0.api_settings.bootstrap_blacklist_file.clone())
                        .map_err(|e| {
                            ApiError::InternalServerError(format!(
                                "failed to create bootsrap blacklist configuration file: {}",
                                e
                            ))
                            .into()
                        })
                        .and_then(|blacklist_file| {
                            let ips_set: BTreeSet<IpAddr> = BTreeSet::from_iter(ips);
                            serde_json::to_writer_pretty(blacklist_file, &ips_set).map_err(|e| {
                                ApiError::InternalServerError(format!(
                                    "failed to write bootsrap blacklist configuration file: {}",
                                    e
                                ))
                                .into()
                            })
                        })
                } else {
                    serde_json::from_reader::<BufReader<File>, BTreeSet<IpAddr>>(BufReader::new(
                        bootsrap_blacklist_file,
                    ))
                    .map_err(|e| {
                        ApiError::InternalServerError(format!(
                            "failed to parse bootsrap blacklist configuration file: {}",
                            e
                        ))
                        .into()
                    })
                    .and_then(|mut blacklist_ips: BTreeSet<IpAddr>| {
                        blacklist_ips.extend(ips);
                        OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(self.0.api_settings.bootstrap_blacklist_file.clone())
                            .map_err(|e| {
                                ApiError::InternalServerError(format!(
                                    "failed to create bootsrap blacklist configuration file: {}",
                                    e
                                ))
                                .into()
                            })
                            .and_then(|blacklist_file| {
                                serde_json::to_writer_pretty(blacklist_file, &blacklist_ips)
                                    .map_err(|e| {
                                        ApiError::InternalServerError(format!(
                                        "failed to write bootsrap blacklist configuration file: {}",
                                        e
                                    ))
                                        .into()
                                    })
                            })
                    })
                }
            })
    }

    async fn node_remove_from_bootstrap_blacklist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        OpenOptions::new()
            .read(true)
            .open(self.0.api_settings.bootstrap_blacklist_file.clone())
            .map_err(|e| {
                ApiError::InternalServerError(format!(
                    "failed to read bootsrap blacklist configuration file: {}",
                    e
                ))
                .into()
            })
            .and_then(|bootsrap_blacklist_file| {
                let len = match bootsrap_blacklist_file.metadata() {
                    Ok(metadata) => metadata.len(),
                    Err(e) => {
                        return Err(ApiError::InternalServerError(format!(
                            "failed to read bootsrap blacklist configuration file metadata: {}",
                            e
                        ))
                        .into())
                    }
                };
                if len == 0 {
                    Err(ApiError::BadRequest(
                        "failed, bootsrap blacklist configuration file is empty".to_string(),
                    )
                    .into())
                } else {
                    serde_json::from_reader::<BufReader<File>, BTreeSet<IpAddr>>(BufReader::new(
                        bootsrap_blacklist_file,
                    ))
                    .map_err(|e| {
                        ApiError::InternalServerError(format!(
                            "failed to parse bootsrap blacklist configuration file: {}",
                            e
                        ))
                        .into()
                    })
                    .and_then(|mut blacklist_ips: BTreeSet<IpAddr>| {
                        ips.into_iter().for_each(|ip| {
                            blacklist_ips.remove(&ip);
                        });
                        OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(self.0.api_settings.bootstrap_blacklist_file.clone())
                            .map_err(|e| {
                                ApiError::InternalServerError(format!(
                                    "failed to create bootsrap blacklist configuration file: {}",
                                    e
                                ))
                                .into()
                            })
                            .and_then(|blacklist_file| {
                                serde_json::to_writer_pretty(blacklist_file, &blacklist_ips)
                                    .map_err(|e| {
                                        ApiError::InternalServerError(format!(
                                    "failed to write bootsrap blacklist configuration file: {}",
                                    e
                                ))
                                        .into()
                                    })
                            })
                    })
                }
            })
    }

    async fn get_openrpc_spec(&self) -> RpcResult<Value> {
        crate::wrong_api::<Value>()
    }
}
