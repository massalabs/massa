//! On `NetworkWorker` receive a command behaviors implementation
//!
//! All following function take by default a reference to the `NetworkWorker`
//! that in order to apply required modification.
//!
//! All following functions are necessary internals (not public) or called by
//! the `manage_network_command` in the worker.
//!
//! ```text
//! async fn manage_network_command(&mut self, cmd: NetworkCommand) -> Result<(), NetworkError> {
//!     use crate::network_cmd_impl::*;
//!     match cmd {
//!         NetworkCommand::NodeBanByIps(ips) => on_node_ban_by_ips_cmd(self, ips).await?,
//!         NetworkCommand::NodeBanByIds(ids) => on_node_ban_by_ids_cmd(self, ids).await?,
//!         NetworkCommand::SendBlockHeader { node, header } => on_send_block_header_cmd(self, node, header).await?,
//!         NetworkCommand::AskForBlocks { list } => on_ask_for_block_cmd(self, list).await,
//!         NetworkCommand::SendBlock { node, block } => on_send_block_cmd(self, node, block).await?,
//!         NetworkCommand::GetPeers(response_tx) => on_get_peers_cmd(self, response_tx).await,
//!         NetworkCommand::GetBootstrapPeers(response_tx) => on_get_bootstrap_peers_cmd(self, response_tx).await,
//!         ...
//! ```
use crate::network_worker::NetworkWorker;
use futures::{stream::FuturesUnordered, StreamExt};
use massa_hash::Hash;
use massa_logging::massa_trace;
use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    composite::PubkeySig,
    endorsement::SecureShareEndorsement,
    node::NodeId,
    operation::{OperationPrefixIds, SecureShareOperation},
    stats::NetworkStats,
};
use massa_network_exports::{
    AskForBlocksInfo, BlockInfoReply, BootstrapPeers, ConnectionClosureReason, ConnectionId,
    NetworkError, NodeCommand, Peer, Peers,
};
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
};
use tokio::sync::oneshot;
use tracing::warn;

/// Remove the `ids` from the `worker`
/// - clean `worker.running_handshakes`
/// - send `NodeCommand::Close` to the active nodes
async fn ban_connection_ids(worker: &mut NetworkWorker, ids: HashSet<ConnectionId>) {
    for ban_conn_id in ids.iter() {
        // remove the connectionId entry in running_handshakes
        worker.running_handshakes.remove(ban_conn_id);
    }
    for (conn_id, node_command_tx) in worker.active_nodes.values() {
        if ids.contains(conn_id) {
            let res = node_command_tx
                .send(NodeCommand::Close(ConnectionClosureReason::Banned))
                .await;
            if res.is_err() {
                massa_trace!(
                    "network.network_worker.manage_network_command", {"err": NetworkError::ChannelError(
                        "close node command send failed".into(),
                    ).to_string()}
                );
            }
        };
    }
}

/// Ban the connections corresponding to `ips` from the `worker`
/// See also `ban_connection_ids`
async fn node_ban_by_ips(worker: &mut NetworkWorker, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
    for ip in ips.iter() {
        worker.peer_info_db.peer_banned(ip)?;
    }
    let connexion_ids = worker
        .active_connections
        .iter()
        .filter_map(|(conn_id, (ip, _))| {
            if ips.contains(ip) {
                Some(conn_id)
            } else {
                None
            }
        })
        .copied()
        .collect::<HashSet<_>>();
    ban_connection_ids(worker, connexion_ids).await;
    Ok(())
}

/// Ban the connections corresponding to node `ids` from the `worker`
/// See also `ban_connection_ids`
async fn node_ban_by_ids(worker: &mut NetworkWorker, ids: Vec<NodeId>) -> Result<(), NetworkError> {
    // get all connection IDs to ban
    let connection_ids_to_ban = ids
        .iter()
        .map(|id| get_connection_ids(worker, id))
        .filter(|res| res.is_ok())
        .flat_map(|res| res.unwrap())
        .collect::<HashSet<_>>();

    if connection_ids_to_ban.is_empty() {
        let log = format!(
            "no connection to ban found when executing node_ban_by_ids for ids: {:?}",
            ids
        );
        warn!("{}", &log);
        return Err(NetworkError::GeneralProtocolError(log));
    }

    ban_connection_ids(worker, connection_ids_to_ban).await;
    Ok(())
}

/// For each peer get all node id associated to this peer ip.
async fn get_peers(worker: &mut NetworkWorker, response_tx: oneshot::Sender<Peers>) {
    let peers: HashMap<IpAddr, Peer> = worker
        .peer_info_db
        .get_peers()
        .iter()
        .map(|(peer_ip_addr, peer)| {
            (
                *peer_ip_addr,
                Peer {
                    peer_info: *peer,
                    active_nodes: worker
                        .active_connections
                        .iter()
                        .filter(|(_, (ip_addr, _))| &peer.ip == ip_addr)
                        .filter_map(|(out_conn_id, (_, out_going))| {
                            worker
                                .active_nodes
                                .iter()
                                .filter_map(|(node_id, (conn_id, _))| {
                                    if out_conn_id == conn_id {
                                        Some(node_id)
                                    } else {
                                        None
                                    }
                                })
                                .next()
                                .map(|node_id| (*node_id, *out_going))
                        })
                        .collect(),
                },
            )
        })
        .collect();

    // HashMap<NodeId, (ConnectionId, mpsc::Sender<NodeCommand>)
    if response_tx
        .send(Peers {
            peers,
            our_node_id: worker.self_node_id,
        })
        .is_err()
    {
        warn!("network: could not send GetPeersChannelError upstream");
    }
}

pub async fn on_node_ban_by_ips_cmd(
    worker: &mut NetworkWorker,
    ips: Vec<IpAddr>,
) -> Result<(), NetworkError> {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::NodeBanByIps",
        { "ips": ips }
    );
    node_ban_by_ips(worker, ips).await
}

pub async fn on_node_ban_by_ids_cmd(
    worker: &mut NetworkWorker,
    ids: Vec<NodeId>,
) -> Result<(), NetworkError> {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::NodeBanByIds",
        { "ids": ids }
    );
    node_ban_by_ids(worker, ids).await
}

pub async fn on_send_block_header_cmd(
    worker: &mut NetworkWorker,
    node: NodeId,
    header: SecuredHeader,
) -> Result<(), NetworkError> {
    massa_trace!("network_worker.manage_network_command send NodeCommand::SendBlockHeader", {"block_id": header.id, "node": node});
    worker
        .event
        .forward(
            node,
            worker.active_nodes.get(&node),
            NodeCommand::SendBlockHeader(header),
        )
        .await;
    Ok(())
}

pub async fn on_ask_for_block_cmd(
    worker: &mut NetworkWorker,
    map: HashMap<NodeId, Vec<(BlockId, AskForBlocksInfo)>>,
) {
    for (node, hash_list) in map.into_iter() {
        massa_trace!(
            "network_worker.manage_network_command receive NetworkCommand::AskForBlocks",
            { "hashlist": hash_list, "node": node }
        );
        worker
            .event
            .forward(
                node,
                worker.active_nodes.get(&node),
                NodeCommand::AskForBlocks(hash_list),
            )
            .await;
    }
}

pub async fn on_send_block_info_cmd(
    worker: &mut NetworkWorker,
    node: NodeId,
    info: Vec<(BlockId, BlockInfoReply)>,
) -> Result<(), NetworkError> {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::SendBlockInfo",
        { "node": node }
    );
    worker
        .event
        .forward(
            node,
            worker.active_nodes.get(&node),
            NodeCommand::ReplyForBlocks(info),
        )
        .await;
    Ok(())
}

pub async fn on_get_peers_cmd(worker: &mut NetworkWorker, response_tx: oneshot::Sender<Peers>) {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::GetPeers",
        {}
    );
    get_peers(worker, response_tx).await;
}

pub async fn on_get_bootstrap_peers_cmd(
    worker: &mut NetworkWorker,
    response_tx: oneshot::Sender<BootstrapPeers>,
) {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::GetBootstrapPeers",
        {}
    );
    let peer_list = worker.peer_info_db.get_advertisable_peer_ips();
    if response_tx.send(BootstrapPeers(peer_list)).is_err() {
        warn!("network: could not send GetBootstrapPeers response upstream");
    }
}

pub async fn on_send_endorsements_cmd(
    worker: &mut NetworkWorker,
    node: NodeId,
    endorsements: Vec<SecureShareEndorsement>,
) {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::SendEndorsements",
        { "node": node, "endorsements": endorsements }
    );
    worker
        .event
        .forward(
            node,
            worker.active_nodes.get(&node),
            NodeCommand::SendEndorsements(endorsements),
        )
        .await;
}

pub async fn on_node_sign_message_cmd(
    worker: &mut NetworkWorker,
    msg: Vec<u8>,
    response_tx: oneshot::Sender<PubkeySig>,
) -> Result<(), NetworkError> {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::NodeSignMessage",
        { "mdg": msg }
    );
    let signature = worker.keypair.sign(&Hash::compute_from(&msg))?;
    if response_tx
        .send(PubkeySig {
            public_key: worker.keypair.get_public_key(),
            signature,
        })
        .is_err()
    {
        warn!("network: could not send NodeSignMessage response upstream");
    }
    Ok(())
}

pub async fn on_node_unban_by_ids_cmd(
    worker: &mut NetworkWorker,
    ids: Vec<NodeId>,
) -> Result<(), NetworkError> {
    let ips_to_unban = ids
        .iter()
        .flat_map(|id| get_ip(worker, id))
        .collect::<Vec<_>>();
    worker.peer_info_db.unban(ips_to_unban)
}

pub async fn on_node_unban_by_ips_cmd(
    worker: &mut NetworkWorker,
    ips: Vec<IpAddr>,
) -> Result<(), NetworkError> {
    worker.peer_info_db.unban(ips)
}

pub async fn on_whitelist_cmd(
    worker: &mut NetworkWorker,
    ips: Vec<IpAddr>,
) -> Result<(), NetworkError> {
    worker.peer_info_db.whitelist(ips).await
}

pub async fn on_remove_from_whitelist_cmd(
    worker: &mut NetworkWorker,
    ips: Vec<IpAddr>,
) -> Result<(), NetworkError> {
    worker.peer_info_db.remove_from_whitelist(ips).await
}

pub async fn on_get_stats_cmd(
    worker: &mut NetworkWorker,
    response_tx: oneshot::Sender<NetworkStats>,
) {
    let res = NetworkStats {
        in_connection_count: worker.peer_info_db.get_in_connection_count(),
        out_connection_count: worker.peer_info_db.get_out_connection_count(),
        known_peer_count: worker.peer_info_db.peers.len() as u64,
        banned_peer_count: worker
            .peer_info_db
            .peers
            .iter()
            .filter(|(_, p)| p.banned)
            .fold(0, |acc, _| acc + 1),
        active_node_count: worker.active_nodes.len() as u64,
    };
    if response_tx.send(res).is_err() {
        warn!("network: could not send NodeSignMessage response upstream");
    }
}

/// Network worker received the command `NetworkCommand::SendOperations` from
/// the controller. Happen when the program has received a new set of operation
/// or run a kind of "send operations" loop.
///
/// todo: precise the documentation in followup
///
/// Forward to the node worker to be propagate in the network.
pub async fn on_send_operations_cmd(
    worker: &mut NetworkWorker,
    to_node: NodeId,
    operations: Vec<SecureShareOperation>,
) {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::SendOperations",
        { "node": to_node, "operations": operations }
    );
    worker
        .event
        .forward(
            to_node,
            worker.active_nodes.get(&to_node),
            NodeCommand::SendOperations(operations),
        )
        .await;
}

/// On the command `[massa_network_exports::NetworkCommand::SendOperationAnnouncements]` is called,
/// Forward (and split) the command to the `NodeWorker` and propagate to the network
pub async fn on_send_operation_batches_cmd(
    worker: &mut NetworkWorker,
    to_node: NodeId,
    batch: OperationPrefixIds,
) {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::SendOperationAnnouncements",
        { "batch": batch }
    );
    let mut futs = FuturesUnordered::new();
    let fut = worker.event.forward(
        to_node,
        worker.active_nodes.get(&to_node),
        NodeCommand::SendOperationAnnouncements(batch),
    );
    futs.push(fut);
    while futs.next().await.is_some() {}
}

/// Network worker received the command `NetworkCommand::AskForOperations` from
/// the controller. Happen when the program run a kind of "ask operations" loop
/// or received a new batch.
///
/// # See also
/// `[massa_models::operation::OperationBatchItem]`
/// `[massa_models::operation::OperationBatchBuffer]`
/// todo: add the link to the function that process the buffer
///
/// # What it does
/// When the command `[massa_network_exports::NetworkCommand::AskForOperations]` is called,
/// Forward the command to the `NodeWorker` and propagate to the network
pub async fn on_ask_for_operations_cmd(
    worker: &mut NetworkWorker,
    to_node: NodeId,
    wishlist: OperationPrefixIds,
) {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::SendOperationAnnouncements",
        { "wishlist": wishlist }
    );
    worker
        .event
        .forward(
            to_node,
            worker.active_nodes.get(&to_node),
            NodeCommand::AskForOperations(wishlist),
        )
        .await;
}

fn get_connection_ids(
    worker: &mut NetworkWorker,
    node: &NodeId,
) -> Result<HashSet<ConnectionId>, NetworkError> {
    let mut ids: HashSet<ConnectionId> = HashSet::new();
    if let Some((orig_conn_id, _)) = worker.active_nodes.get(node) {
        if let Some((orig_ip, _)) = worker.active_connections.get(orig_conn_id) {
            worker.peer_info_db.peer_banned(orig_ip)?;
            for (target_conn_id, (target_ip, _)) in worker.active_connections.iter() {
                if target_ip == orig_ip {
                    ids.insert(*target_conn_id);
                }
            }
        }
    }

    Ok(ids)
}

fn get_ip(worker: &mut NetworkWorker, node: &NodeId) -> Option<IpAddr> {
    if let Some((orig_conn_id, _)) = worker.active_nodes.get(node) {
        if let Some((orig_ip, _)) = worker.active_connections.get(orig_conn_id) {
            for (_, (target_ip, _)) in worker.active_connections.iter() {
                if target_ip == orig_ip {
                    return Some(*target_ip);
                }
            }
        }
    }
    None
}
