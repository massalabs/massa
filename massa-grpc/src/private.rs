// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaGrpc;
use massa_proto_rs::massa::api::v1 as grpc_api;
// use massa_proto_rs::massa::model::v1 as grpc_model;

/// Add IP addresses to node bootstrap blacklist
pub(crate) fn add_to_bootstrap_blacklist(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::AddToBootstrapBlacklistRequest>,
) -> Result<tonic::Response<grpc_api::AddToBootstrapBlacklistResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Add IP addresses to node bootstrap whitelist
pub(crate) fn add_to_bootstrap_whitelist(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::AddToBootstrapWhitelistRequest>,
) -> Result<tonic::Response<grpc_api::AddToBootstrapWhitelistResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Add IP addresses to node peers whitelist. No confirmation to expect.
/// Note: If the ip was unknown it adds it to the known peers, otherwise it updates the peer type
pub(crate) fn add_to_peers_whitelist(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::AddToPeersWhitelistRequest>,
) -> Result<tonic::Response<grpc_api::AddToPeersWhitelistResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Add staking secret keys to wallet
pub(crate) fn add_staking_secret_keys(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::AddStakingSecretKeysRequest>,
) -> Result<tonic::Response<grpc_api::AddStakingSecretKeysResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Get node bootstrap blacklist IP addresses
pub(crate) fn get_bootstrap_blacklist(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::GetBootstrapBlacklistRequest>,
) -> Result<tonic::Response<grpc_api::GetBootstrapBlacklistResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Get node bootstrap whitelist IP addresses
pub(crate) fn get_bootstrap_whitelist(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::GetBootstrapWhitelistRequest>,
) -> Result<tonic::Response<grpc_api::GetBootstrapWhitelistResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Allow everyone to bootstrap from the node by removing bootstrap whitelist configuration file
pub(crate) fn allow_everyone_to_bootstrap(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::AllowEveryoneToBootstrapRequest>,
) -> Result<tonic::Response<grpc_api::AllowEveryoneToBootstrapResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Get node status
pub(crate) fn get_node_status(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::GetNodeStatusRequest>,
) -> Result<tonic::Response<grpc_api::GetNodeStatusResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Get node peers whitelist IP addresses
pub(crate) fn get_peers_whitelist(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::GetPeersWhitelistRequest>,
) -> Result<tonic::Response<grpc_api::GetPeersWhitelistResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Remove from bootstrap blacklist given IP addresses
pub(crate) fn remove_from_bootstrap_blacklist(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::RemoveFromBootstrapBlacklistRequest>,
) -> Result<tonic::Response<grpc_api::RemoveFromBootstrapBlacklistResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Remove from bootstrap whitelist given IP addresses
pub(crate) fn remove_from_bootstrap_whitelist(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::RemoveFromBootstrapWhitelistRequest>,
) -> Result<tonic::Response<grpc_api::RemoveFromBootstrapWhitelistResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Remove from peers whitelist given IP addresses
pub(crate) fn remove_from_peers_whitelist(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::RemoveFromPeersWhitelistRequest>,
) -> Result<tonic::Response<grpc_api::RemoveFromPeersWhitelistResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Remove addresses from staking
pub(crate) fn remove_staking_addresses(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::RemoveStakingAddressesRequest>,
) -> Result<tonic::Response<grpc_api::RemoveStakingAddressesResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Sign messages with node's key
pub(crate) fn sign_messages(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::SignMessagesRequest>,
) -> Result<tonic::Response<grpc_api::SignMessagesResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
/// Shutdown the node gracefully
pub(crate) fn shutdown_gracefully(
    _grpc: &MassaGrpc,
    _request: tonic::Request<grpc_api::ShutdownGracefullyRequest>,
) -> Result<tonic::Response<grpc_api::ShutdownGracefullyResponse>, GrpcError> {
    unimplemented!("not implemented yet")
}
