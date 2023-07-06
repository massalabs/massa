// Copyright (c) 2023 MASSA LABS <info@massa.net>

use std::collections::HashSet;
use std::net::IpAddr;
use std::{collections::BTreeMap, str::FromStr};

use crate::error::GrpcError;
use crate::server::MassaPrivateGrpc;
use hyper::client::connect::Connect;
use massa_execution_exports::ExecutionQueryRequest;
use massa_hash::Hash;
use massa_models::config::CompactConfig;
use massa_models::error::ModelsError;
use massa_models::node::NodeId;
use massa_models::slot::Slot;
use massa_models::timeslots::get_latest_block_slot_at_timestamp;
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_proto_rs::massa::model::v1 as grpc_model;
use massa_proto_rs::massa::model::v1::KeyPair as GrpcKeyPair;
use massa_proto_rs::massa::model::v1::MipComponent::Keypair;
use massa_protocol_exports::{PeerConnectionType, PeerId};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use massa_versioning::keypair_factory::KeyPairFactory;
use massa_versioning::versioning_factory::{FactoryStrategy, VersioningFactory};
use tracing::warn;
// use massa_proto_rs::massa::model::v1 "add_to_bootstrap_blacklist"as grpc_model;

/// Add IP addresses to node bootstrap blacklist
pub(crate) fn add_to_bootstrap_blacklist(
    grpc: &MassaPrivateGrpc,
    request: tonic::Request<grpc_api::AddToBootstrapBlacklistRequest>,
) -> Result<grpc_api::AddToBootstrapBlacklistResponse, GrpcError> {
    let inner_req = request.into_inner();

    let ips = inner_req
        .ips
        .into_iter()
        .filter_map(|ip| match IpAddr::from_str(&ip) {
            Ok(ip_addr) => Some(ip_addr),
            Err(e) => {
                warn!("error when parsing address : {}", e);
                None
            }
        })
        .collect();

    if let Some(bs_list) = &grpc.bs_white_black_list {
        if let Err(e) = bs_list.add_ips_to_blacklist(ips) {
            warn!("error when adding ips to bootstrap blacklist : {}", e)
        }
    }

    Ok(grpc_api::AddToBootstrapBlacklistResponse {})
}
/// Add IP addresses to node bootstrap whitelist
pub(crate) fn add_to_bootstrap_whitelist(
    grpc: &MassaPrivateGrpc,
    request: tonic::Request<grpc_api::AddToBootstrapWhitelistRequest>,
) -> Result<grpc_api::AddToBootstrapWhitelistResponse, GrpcError> {
    let inner_req = request.into_inner();

    let ips = inner_req
        .ips
        .into_iter()
        .filter_map(|ip| match IpAddr::from_str(&ip) {
            Ok(ip_addr) => Some(ip_addr),
            Err(e) => {
                warn!("error when parsing address : {}", e);
                None
            }
        })
        .collect();

    if let Some(bs_list) = &grpc.bs_white_black_list {
        if let Err(e) = bs_list.add_ips_to_whitelist(ips) {
            warn!("error when adding ips to bootstrap whitelist : {}", e)
        }
    }

    Ok(grpc_api::AddToBootstrapWhitelistResponse {})
}
/// Add IP addresses to node peers whitelist. No confirmation to expect.
/// Note: If the ip was unknown it adds it to the known peers, otherwise it updates the peer type
pub(crate) fn add_to_peers_whitelist(
    _grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::AddToPeersWhitelistRequest>,
) -> Result<grpc_api::AddToPeersWhitelistResponse, GrpcError> {
    Err(GrpcError::Unimplemented(
        "add_to_peers_whitelist".to_string(),
    ))
}
/// Add staking secret keys to wallet
pub(crate) fn add_staking_secret_keys(
    grpc: &MassaPrivateGrpc,
    request: tonic::Request<grpc_api::AddStakingSecretKeysRequest>,
) -> Result<grpc_api::AddStakingSecretKeysResponse, GrpcError> {
    let secret_keys = request.into_inner().secret_keys;

    if secret_keys.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no secret key received".to_string(),
        ));
    }

    //TODO customize number of accepted parameters
    if secret_keys.len() as u32 > grpc.grpc_config.max_parameter_size {
        return Err(GrpcError::InvalidArgument(format!(
            "too many secret received. Only a maximum of {} secret keys are accepted per request",
            grpc.grpc_config.max_parameter_size
        )));
    }

    let keypairs = match secret_keys.iter().map(|x| KeyPair::from_str(x)).collect() {
        Ok(keypairs) => keypairs,
        Err(e) => return Err(GrpcError::InvalidArgument(e.to_string()).into()),
    };

    grpc.node_wallet.write().add_keypairs(keypairs)?;

    Ok(grpc_api::AddStakingSecretKeysResponse {})
}

/// Ban multiple nodes by their individual ids
pub(crate) fn ban_nodes_by_ids(
    grpc: &MassaPrivateGrpc,
    request: tonic::Request<grpc_api::BanNodesByIdsRequest>,
) -> Result<grpc_api::BanNodesByIdsResponse, GrpcError> {
    let node_ids = request.into_inner().node_ids;

    if node_ids.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no node id received".to_string(),
        ));
    }

    //TODO customize number of accepted parameters
    if node_ids.len() as u32 > grpc.grpc_config.max_parameter_size {
        return Err(GrpcError::InvalidArgument(format!(
            "too many node ids received. Only a maximum of {} node ids are accepted per request",
            grpc.grpc_config.max_parameter_size
        )));
    }

    //TODO: Change when unify node id and peer id
    let peer_ids = node_ids
        .into_iter()
        .map(|id| {
            NodeId::from_str(&id).map(|node_id| PeerId::from_public_key(node_id.get_public_key()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    grpc.protocol_controller.ban_peers(peer_ids)?;

    Ok(grpc_api::BanNodesByIdsResponse {})
}

/// Ban multiple nodes by their individual IP addresses
pub(crate) fn ban_nodes_by_ips(
    _grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::BanNodesByIpsRequest>,
) -> Result<grpc_api::BanNodesByIpsResponse, GrpcError> {
    Err(GrpcError::Unimplemented("ban_nodes_by_ips".to_string()))
}

// Create KeyPair
pub(crate) fn create_key_pair(
    grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::CreateKeyPairRequest>,
) -> Result<grpc_api::CreateKeyPairResponse, GrpcError> {
    let now = MassaTime::now().map_err(|e| GrpcError::TimeError(e))?;
    let keypair = grpc.keypair_factory.create(&(), FactoryStrategy::At(now))?;
    let public_key_serialized = serde_json::to_string(&keypair.get_public_key())
        .map_err(|e| GrpcError::ModelsError(ModelsError::SerializeError(e.to_string())))?;
    let keypair_serialized = serde_json::to_string(&keypair)
        .map_err(|e| GrpcError::ModelsError(ModelsError::SerializeError(e.to_string())))?;
    Ok(grpc_api::CreateKeyPairResponse {
        key_pair: Some(GrpcKeyPair {
            public_key: public_key_serialized,
            secret_key: keypair_serialized,
        }),
    })
}

/// Get node bootstrap blacklist IP addresses
pub(crate) fn get_bootstrap_blacklist(
    grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::GetBootstrapBlacklistRequest>,
) -> Result<grpc_api::GetBootstrapBlacklistResponse, GrpcError> {
    let list = {
        match grpc.bs_white_black_list {
            Some(ref bs_list) => bs_list
                .get_black_list()
                .unwrap_or(HashSet::new())
                .into_iter()
                .map(|ip| ip.to_string())
                .collect(),
            None => Vec::new(),
        }
    };
    Ok(grpc_api::GetBootstrapBlacklistResponse { ips: list })
}
/// Get node bootstrap whitelist IP addresses
pub(crate) fn get_bootstrap_whitelist(
    grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::GetBootstrapWhitelistRequest>,
) -> Result<grpc_api::GetBootstrapWhitelistResponse, GrpcError> {
    let list = {
        match grpc.bs_white_black_list {
            Some(ref bs_list) => bs_list
                .get_white_list()
                .unwrap_or(HashSet::new())
                .into_iter()
                .map(|ip| ip.to_string())
                .collect(),
            None => Vec::new(),
        }
    };

    Ok(grpc_api::GetBootstrapWhitelistResponse { ips: list })
}
// Get MIP store dump
pub(crate) fn get_mip_status(
    grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::GetMipStatusRequest>,
) -> Result<grpc_api::GetMipStatusResponse, GrpcError> {
    let mip_store_status_ = grpc.keypair_factory.mip_store.get_mip_status();
    let mip_store_status: Result<Vec<grpc_model::MipStatusEntry>, GrpcError> = mip_store_status_
        .iter()
        .map(|(mip_info, state_id_)| {
            let state_id = grpc_model::ComponentStateId::from(state_id_);
            Ok(grpc_model::MipStatusEntry {
                mip_info: Some(grpc_model::MipInfo::from(mip_info)),
                state_id: i32::from(state_id),
            })
        })
        .collect();

    Ok(grpc_api::GetMipStatusResponse {
        mipstatus_entries: mip_store_status?,
    })
}

/// Allow everyone to bootstrap from the node by removing bootstrap whitelist configuration file
pub(crate) fn allow_everyone_to_bootstrap(
    _grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::AllowEveryoneToBootstrapRequest>,
) -> Result<grpc_api::AllowEveryoneToBootstrapResponse, GrpcError> {
    Err(GrpcError::Unimplemented(
        "allow_everyone_to_bootstrap".to_string(),
    ))
}
/// Get node status
pub(crate) fn get_node_status(
    grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::GetNodeStatusRequest>,
) -> Result<grpc_api::GetNodeStatusResponse, GrpcError> {
    let config = CompactConfig::default();
    let now = MassaTime::now()?;
    let last_slot = get_latest_block_slot_at_timestamp(
        grpc.grpc_config.thread_count,
        grpc.grpc_config.t0,
        grpc.grpc_config.genesis_timestamp,
        now,
    )?;
    let execution_stats = grpc.execution_controller.get_stats();
    let consensus_stats = grpc.consensus_controller.get_stats()?;
    let (network_stats, peers) = grpc.protocol_controller.get_stats()?;
    let pool_stats = grpc_model::PoolStats {
        operations_count: grpc.pool_controller.get_denunciation_count() as u64,
        endorsements_count: grpc.pool_controller.get_endorsement_count() as u64,
    };

    let mut connected_nodes = peers
        .iter()
        .map(|(id, peer)| {
            let connection_type = match peer.1 {
                PeerConnectionType::IN => grpc_model::ConnectionType::Incoming,
                PeerConnectionType::OUT => grpc_model::ConnectionType::Outgoing,
            };

            grpc_model::ConnectedNode {
                node_id: NodeId::new(id.get_public_key()).to_string(),
                node_ip: peer.0.ip().to_string(),
                connection_type: connection_type as i32,
            }
        })
        .collect::<Vec<_>>();
    connected_nodes.sort_by(|a, b| a.node_ip.cmp(&b.node_ip));

    let current_cycle = last_slot
        .unwrap_or_else(|| Slot::new(0, 0))
        .get_cycle(grpc.grpc_config.periods_per_cycle);
    let cycle_duration = grpc
        .grpc_config
        .t0
        .checked_mul(grpc.grpc_config.periods_per_cycle)?;
    let current_cycle_time = if current_cycle == 0 {
        grpc.grpc_config.genesis_timestamp
    } else {
        cycle_duration
            .checked_mul(current_cycle)
            .and_then(|elapsed_time_before_current_cycle| {
                grpc.grpc_config
                    .genesis_timestamp
                    .checked_add(elapsed_time_before_current_cycle)
            })?
    };
    let next_cycle_time = current_cycle_time.checked_add(cycle_duration)?;
    let empty_request = ExecutionQueryRequest { requests: vec![] };
    let state = grpc.execution_controller.query_state(empty_request);
    let node_ip = grpc
        .protocol_config
        .routable_ip
        .map(|ip| ip.to_string())
        .unwrap_or("".to_string());

    let status = grpc_model::NodeStatus {
        node_id: grpc.node_id.to_string(),
        node_ip,
        version: grpc.version.to_string(),
        current_time: Some(now.into()),
        current_cycle: current_cycle.into(),
        current_cycle_time: Some(current_cycle_time.into()),
        next_cycle_time: Some(next_cycle_time.into()),
        connected_nodes,
        last_executed_final_slot: Some(state.final_cursor.into()),
        last_executed_speculative_slot: Some(state.candidate_cursor.into()),
        final_state_fingerprint: state.final_state_fingerprint.to_string(),
        consensus_stats: Some(consensus_stats.into()),
        pool_stats: Some(pool_stats),
        network_stats: Some(network_stats.into()),
        execution_stats: Some(execution_stats.into()),
        config: Some(config.into()),
    };

    Ok(grpc_api::GetNodeStatusResponse {
        status: Some(status),
    })
}
/// Get node peers whitelist IP addresses
pub(crate) fn get_peers_whitelist(
    _grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::GetPeersWhitelistRequest>,
) -> Result<grpc_api::GetPeersWhitelistResponse, GrpcError> {
    Err(GrpcError::Unimplemented("get_peers_whitelist".to_string()))
}
/// Remove from bootstrap blacklist given IP addresses
pub(crate) fn remove_from_bootstrap_blacklist(
    grpc: &MassaPrivateGrpc,
    request: tonic::Request<grpc_api::RemoveFromBootstrapBlacklistRequest>,
) -> Result<grpc_api::RemoveFromBootstrapBlacklistResponse, GrpcError> {
    let inner_req = request.into_inner();
    let ips = inner_req
        .ips
        .into_iter()
        .filter_map(|ip| match IpAddr::from_str(&ip) {
            Ok(ip_addr) => Some(ip_addr),
            Err(e) => {
                warn!("error when parsing address : {}", e);
                None
            }
        })
        .collect();

    if let Some(bs_list) = &grpc.bs_white_black_list {
        if let Err(e) = bs_list.remove_ips_from_blacklist(ips) {
            warn!("error when removing ips to bootstrap blacklist : {}", e)
        }
    }

    Ok(grpc_api::RemoveFromBootstrapBlacklistResponse {})
}
/// Remove from bootstrap whitelist given IP addresses
pub(crate) fn remove_from_bootstrap_whitelist(
    grpc: &MassaPrivateGrpc,
    request: tonic::Request<grpc_api::RemoveFromBootstrapWhitelistRequest>,
) -> Result<grpc_api::RemoveFromBootstrapWhitelistResponse, GrpcError> {
    let inner_req = request.into_inner();
    let ips = inner_req
        .ips
        .into_iter()
        .filter_map(|ip| match IpAddr::from_str(&ip) {
            Ok(ip_addr) => Some(ip_addr),
            Err(e) => {
                warn!("error when parsing address : {}", e);
                None
            }
        })
        .collect();

    if let Some(bs_list) = &grpc.bs_white_black_list {
        if let Err(e) = bs_list.remove_ips_from_whitelist(ips) {
            warn!("error when removing ips to bootstrap whitelist : {}", e)
        }
    }

    Ok(grpc_api::RemoveFromBootstrapWhitelistResponse {})
}
/// Remove from peers whitelist given IP addresses
pub(crate) fn remove_from_peers_whitelist(
    _grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::RemoveFromPeersWhitelistRequest>,
) -> Result<grpc_api::RemoveFromPeersWhitelistResponse, GrpcError> {
    Err(GrpcError::Unimplemented(
        "remove_from_peers_whitelist".to_string(),
    ))
}
/// Remove addresses from staking
pub(crate) fn remove_staking_addresses(
    _grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::RemoveStakingAddressesRequest>,
) -> Result<grpc_api::RemoveStakingAddressesResponse, GrpcError> {
    Err(GrpcError::Unimplemented(
        "remove_staking_addresses".to_string(),
    ))
}
/// Sign messages with node's key
pub(crate) fn sign_messages(
    grpc: &MassaPrivateGrpc,
    request: tonic::Request<grpc_api::SignMessagesRequest>,
) -> Result<grpc_api::SignMessagesResponse, GrpcError> {
    let messages = request.into_inner().messages;

    if messages.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no message received".to_string(),
        ));
    }

    //TODO customize number of accepted parameters
    if messages.len() as u32 > grpc.grpc_config.max_parameter_size {
        return Err(GrpcError::InvalidArgument(format!(
            "too many messages received. Only a maximum of {} messages are accepted per request",
            grpc.grpc_config.max_parameter_size
        )));
    }

    let keypair = grpc.grpc_config.keypair.clone();
    let signatures = messages
        .into_iter()
        .map(|message| {
            keypair
                .sign(&Hash::compute_from(&message))
                .map(|signature| signature.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(grpc_api::SignMessagesResponse {
        public_key: keypair.get_public_key().to_string(),
        signatures,
    })
}
/// Shutdown the node gracefully
pub(crate) fn shutdown_gracefully(
    _grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::ShutdownGracefullyRequest>,
) -> Result<grpc_api::ShutdownGracefullyResponse, GrpcError> {
    Err(GrpcError::Unimplemented("shutdown_gracefully".to_string()))
}

/// Unban multiple nodes by their individual ids
pub(crate) fn unban_nodes_by_ids(
    grpc: &MassaPrivateGrpc,
    request: tonic::Request<grpc_api::UnbanNodesByIdsRequest>,
) -> Result<grpc_api::UnbanNodesByIdsResponse, GrpcError> {
    let node_ids = request.into_inner().node_ids;

    if node_ids.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no node id received".to_string(),
        ));
    }

    //TODO customize number of accepted parameters
    if node_ids.len() as u32 > grpc.grpc_config.max_parameter_size {
        return Err(GrpcError::InvalidArgument(format!(
            "too many node ids received. Only a maximum of {} node ids are accepted per request",
            grpc.grpc_config.max_parameter_size
        )));
    }

    //TODO: Change when unify node id and peer id
    let peer_ids = node_ids
        .into_iter()
        .map(|id| {
            NodeId::from_str(&id).map(|node_id| PeerId::from_public_key(node_id.get_public_key()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    grpc.protocol_controller.unban_peers(peer_ids)?;

    Ok(grpc_api::UnbanNodesByIdsResponse {})
}

/// Unban multiple nodes by their individual IP addresses
pub(crate) fn unban_nodes_by_ips(
    _grpc: &MassaPrivateGrpc,
    _request: tonic::Request<grpc_api::UnbanNodesByIpsRequest>,
) -> Result<grpc_api::UnbanNodesByIpsResponse, GrpcError> {
    Err(GrpcError::Unimplemented("unban_nodes_by_ips".to_string()))
}
