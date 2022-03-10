//! On `NetworkWorker` receive a command behaviors implementation
//!
//! All following function take by default a reference to the `NetworkWorker`
//! that in order to apply required modification.
//!
//! All following functions are necessary internals (not public) or called by
//! the `manage_network_command` in the worker.
//!
//! ```ignore
//! async fn manage_network_command(&mut self, cmd: NetworkCommand) -> Result<(), NetworkError> {
//!     use crate::network_cmd_impl::*;
//!     match cmd {
//!         NetworkCommand::BanIp(ips) => on_ban_ip_cmd(self, ips).await?,
//!         NetworkCommand::Ban(node) => on_ban_cmd(self, node).await?,
//!         NetworkCommand::SendBlockHeader { node, header } => on_send_block_header_cmd(self, node, header).await?,
//!         NetworkCommand::AskForBlocks { list } => on_ask_bfor_block_cmd(self, list).await,
//!         NetworkCommand::SendBlock { node, block } => on_send_block_cmd(self, node, block).await?,
//!         NetworkCommand::GetPeers(response_tx) => on_get_peers_cmd(self, response_tx).await,
//!         NetworkCommand::GetBootstrapPeers(response_tx) => on_get_bootstrap_peers_cmd(self, response_tx).await,
//!         ...
//! ```
use crate::{network_worker::NetworkWorker, node_worker::NodeCommand};
use massa_hash::hash::Hash;
use massa_logging::massa_trace;
use massa_models::signed::Signable;
use massa_models::{
    composite::PubkeySig, node::NodeId, stats::NetworkStats, Block, BlockId, SignedEndorsement,
    SignedHeader, SignedOperation,
};
use massa_network_exports::{
    BootstrapPeers, ConnectionClosureReason, ConnectionId, NetworkError, Peer, Peers,
};
use massa_signature::{derive_public_key, sign};
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
/// See also [ban_connection_ids]
async fn ban_ips(worker: &mut NetworkWorker, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
    for ip in ips.iter() {
        worker.peer_info_db.peer_banned(ip)?;
    }
    let ids = worker
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
    ban_connection_ids(worker, ids).await;
    Ok(())
}

/// Ban the `node` corresponding to the NodeId from the `worker`
/// See also [ban_connection_ids]
async fn ban_node(worker: &mut NetworkWorker, node: NodeId) -> Result<(), NetworkError> {
    // get all connection IDs to ban
    let mut ids: HashSet<ConnectionId> = HashSet::new();

    // Note: if we can't find the node, there is no need to resend the close event,
    // since protocol will have already removed the node from it's list of active ones.
    if let Some((orig_conn_id, _)) = worker.active_nodes.get(&node) {
        if let Some((orig_ip, _)) = worker.active_connections.get(orig_conn_id) {
            worker.peer_info_db.peer_banned(orig_ip)?;
            for (target_conn_id, (target_ip, _)) in worker.active_connections.iter() {
                if target_ip == orig_ip {
                    ids.insert(*target_conn_id);
                }
            }
        }
    }
    ban_connection_ids(worker, ids).await;
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

pub async fn on_ban_ip_cmd(
    worker: &mut NetworkWorker,
    ips: Vec<IpAddr>,
) -> Result<(), NetworkError> {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::BanIp",
        { "ips": ips }
    );
    ban_ips(worker, ips).await
}

pub async fn on_ban_cmd(worker: &mut NetworkWorker, node: NodeId) -> Result<(), NetworkError> {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::Ban",
        { "node": node }
    );
    ban_node(worker, node).await
}

pub async fn on_send_block_header_cmd(
    worker: &mut NetworkWorker,
    node: NodeId,
    header: SignedHeader,
) -> Result<(), NetworkError> {
    massa_trace!("network_worker.manage_network_command send NodeCommand::SendBlockHeader", {"block_id": header.content.compute_id()?, "header": header, "node": node});
    worker
        .event
        .forward(
            &node,
            worker.active_nodes.get(&node),
            NodeCommand::SendBlockHeader(header),
        )
        .await;
    Ok(())
}

pub async fn on_ask_bfor_block_cmd(worker: &mut NetworkWorker, map: HashMap<NodeId, Vec<BlockId>>) {
    for (node, hash_list) in map.into_iter() {
        massa_trace!(
            "network_worker.manage_network_command receive NetworkCommand::AskForBlocks",
            { "hashlist": hash_list, "node": node }
        );
        worker
            .event
            .forward(
                &node,
                worker.active_nodes.get(&node),
                NodeCommand::AskForBlocks(hash_list.clone()),
            )
            .await;
    }
}

pub async fn on_send_block_cmd(
    worker: &mut NetworkWorker,
    node: NodeId,
    block: Block,
) -> Result<(), NetworkError> {
    massa_trace!(
        "network_worker.manage_network_command send NodeCommand::SendBlock",
        {"hash": block.header.content.compute_hash()?, "block": block, "node": node}
    );
    worker
        .event
        .forward(
            &node,
            worker.active_nodes.get(&node),
            NodeCommand::SendBlock(block),
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

pub async fn on_block_not_found_cmd(worker: &mut NetworkWorker, node: NodeId, block_id: BlockId) {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::BlockNotFound",
        { "block_id": block_id, "node": node }
    );
    worker
        .event
        .forward(
            &node,
            worker.active_nodes.get(&node),
            NodeCommand::BlockNotFound(block_id),
        )
        .await;
}

pub async fn on_send_operation_cmd(
    worker: &mut NetworkWorker,
    node: NodeId,
    operations: Vec<SignedOperation>,
) {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::SendOperations",
        { "node": node, "operations": operations }
    );
    worker
        .event
        .forward(
            &node,
            worker.active_nodes.get(&node),
            NodeCommand::SendOperations(operations),
        )
        .await;
}

pub async fn on_send_endorsements_cmd(
    worker: &mut NetworkWorker,
    node: NodeId,
    endorsements: Vec<SignedEndorsement>,
) {
    massa_trace!(
        "network_worker.manage_network_command receive NetworkCommand::SendEndorsements",
        { "node": node, "endorsements": endorsements }
    );
    worker
        .event
        .forward(
            &node,
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
    let signature = sign(&Hash::compute_from(&msg), &worker.private_key)?;
    let public_key = derive_public_key(&worker.private_key);
    if response_tx
        .send(PubkeySig {
            public_key,
            signature,
        })
        .is_err()
    {
        warn!("network: could not send NodeSignMessage response upstream");
    }
    Ok(())
}

pub async fn on_unban_cmd(
    worker: &mut NetworkWorker,
    ips: Vec<IpAddr>,
) -> Result<(), NetworkError> {
    worker.peer_info_db.unban(ips).await
}

pub async fn on_get_stats_cmd(
    worker: &mut NetworkWorker,
    response_tx: oneshot::Sender<NetworkStats>,
) {
    let res = NetworkStats {
        in_connection_count: worker.peer_info_db.active_in_nonbootstrap_connections as u64, // TODO: add bootstrap connections ... see #1312
        out_connection_count: worker.peer_info_db.active_out_nonbootstrap_connections as u64, // TODO: add bootstrap connections ... see #1312
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
