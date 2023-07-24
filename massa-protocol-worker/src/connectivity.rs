use crossbeam::channel::tick;
use crossbeam::select;
use massa_channel::{receiver::MassaReceiver, sender::MassaSender};
use massa_consensus_exports::ConsensusController;
use massa_metrics::MassaMetrics;
use massa_models::stats::NetworkStats;
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::{PeerCategoryInfo, PeerId, ProtocolConfig, ProtocolError};
use massa_storage::Storage;
use massa_versioning::versioning::MipStore;
use parking_lot::RwLock;
use peernet::peer::PeerConnectionType;
use std::net::SocketAddr;
use std::sync::Arc;
use std::{collections::HashMap, net::IpAddr};
use std::{thread::JoinHandle, time::Duration};
use tracing::{info, warn};

use crate::handlers::peer_handler::models::{ConnectionMetadata, PeerDB};
use crate::{
    handlers::peer_handler::models::{InitialPeers, PeerState, SharedPeerDB},
    worker::ProtocolChannels,
};
use crate::{handlers::peer_handler::PeerManagementHandler, messages::MessagesHandler};
use crate::{
    handlers::{
        block_handler::{cache::BlockCache, BlockHandler},
        endorsement_handler::{cache::EndorsementCache, EndorsementHandler},
        operation_handler::{cache::OperationCache, OperationHandler},
        peer_handler::models::PeerMessageTuple,
    },
    wrap_network::NetworkController,
};

#[derive(Clone)]
pub enum ConnectivityCommand {
    Stop,
    GetStats {
        #[allow(clippy::type_complexity)]
        responder: MassaSender<(
            NetworkStats,
            HashMap<PeerId, (SocketAddr, PeerConnectionType)>,
        )>,
    },
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn start_connectivity_thread(
    peer_id: PeerId,
    selector_controller: Box<dyn SelectorController>,
    mut network_controller: Box<dyn NetworkController>,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    channel_blocks: (
        MassaSender<PeerMessageTuple>,
        MassaReceiver<PeerMessageTuple>,
    ),
    channel_endorsements: (
        MassaSender<PeerMessageTuple>,
        MassaReceiver<PeerMessageTuple>,
    ),
    channel_operations: (
        MassaSender<PeerMessageTuple>,
        MassaReceiver<PeerMessageTuple>,
    ),
    channel_peers: (
        MassaSender<PeerMessageTuple>,
        MassaReceiver<PeerMessageTuple>,
    ),
    initial_peers: InitialPeers,
    peer_db: SharedPeerDB,
    storage: Storage,
    protocol_channels: ProtocolChannels,
    messages_handler: MessagesHandler,
    peer_categories: HashMap<String, (Vec<IpAddr>, PeerCategoryInfo)>,
    _default_category: PeerCategoryInfo,
    config: ProtocolConfig,
    mip_store: MipStore,
    massa_metrics: MassaMetrics,
) -> Result<(MassaSender<ConnectivityCommand>, JoinHandle<()>), ProtocolError> {
    let handle = std::thread::Builder::new()
    .name("protocol-connectivity".to_string())
    .spawn({
        let sender_endorsements_propagation_ext = protocol_channels.endorsement_handler_propagation.0.clone();
        let sender_blocks_retrieval_ext = protocol_channels.block_handler_retrieval.0.clone();
        let sender_blocks_propagation_ext = protocol_channels.block_handler_propagation.0.clone();
        let sender_operations_propagation_ext = protocol_channels.operation_handler_propagation.0.clone();
        move || {
            for (addr, transport) in &config.listeners {
                network_controller
                    .start_listener(*transport, *addr)
                    .unwrap_or_else(|_| panic!(
                        "Failed to start listener {:?} of transport {:?} in protocol",
                        addr, transport
                    ));
            }

            // Little hack to be sure that listeners are started before trying to connect to peers
            std::thread::sleep(Duration::from_millis(100));

            // Create cache outside of the op handler because it could be used by other handlers
            let total_in_slots = config.peers_categories.values().map(|v| v.max_in_connections).sum::<usize>() + config.default_category_info.max_in_connections + 1;
            let total_out_slots = config.peers_categories.values().map(| v| v.target_out_connections).sum::<usize>() + config.default_category_info.target_out_connections + 1;
            let operation_cache = Arc::new(RwLock::new(OperationCache::new(
                config.max_known_ops_size.try_into().unwrap(),
                config.max_node_known_ops_size.try_into().unwrap()
            )));
            let endorsement_cache = Arc::new(RwLock::new(EndorsementCache::new(
                config.max_known_endorsements_size.try_into().unwrap(),
                (total_in_slots + total_out_slots).try_into().unwrap()
            )));

            let block_cache = Arc::new(RwLock::new(BlockCache::new(
                config.max_known_blocks_size.try_into().unwrap(),
                config.max_node_known_blocks_size.try_into().unwrap(),
            )));

            // Start handlers
            let mut peer_management_handler = PeerManagementHandler::new(
                initial_peers,
                peer_id,
                peer_db.clone(),
                channel_peers,
                protocol_channels.peer_management_handler,
                messages_handler,
                network_controller.get_active_connections(),
                peer_categories.iter().map(|(key, value)|(key.clone(), (value.0.clone(), value.1.target_out_connections))).collect(),
                config.default_category_info.target_out_connections,
                &config,
            );

            let mut operation_handler = OperationHandler::new(
                pool_controller.clone(),
                storage.clone_without_refs(),
                config.clone(),
                operation_cache.clone(),
                network_controller.get_active_connections(),
                channel_operations.1,
                protocol_channels.operation_handler_retrieval.0.clone(),
                protocol_channels.operation_handler_retrieval.1.clone(),
                sender_operations_propagation_ext.clone(),
                protocol_channels.operation_handler_propagation.1.clone(),
                peer_management_handler.sender.command_sender.clone(),
                massa_metrics.clone(),
            );
            let mut endorsement_handler = EndorsementHandler::new(
                pool_controller.clone(),
                selector_controller.clone(),
                endorsement_cache.clone(),
                storage.clone_without_refs(),
                config.clone(),
                network_controller.get_active_connections(),
                channel_endorsements.1,
                protocol_channels.endorsement_handler_retrieval.0,
                protocol_channels.endorsement_handler_retrieval.1,
                sender_endorsements_propagation_ext.clone(),
                protocol_channels.endorsement_handler_propagation.1.clone(),
                peer_management_handler.sender.command_sender.clone(),
                massa_metrics.clone(),
            );
            let mut block_handler = BlockHandler::new(
                network_controller.get_active_connections(),
                selector_controller,
                consensus_controller,
                pool_controller,
                channel_blocks.1,
                sender_blocks_retrieval_ext,
                protocol_channels.block_handler_retrieval.1.clone(),
                protocol_channels.block_handler_propagation.1.clone(),
                sender_blocks_propagation_ext,
                sender_operations_propagation_ext,
                sender_endorsements_propagation_ext,
                peer_management_handler.sender.command_sender.clone(),
                config.clone(),
                endorsement_cache,
                operation_cache,
                block_cache,
                storage.clone_without_refs(),
                mip_store,
                massa_metrics.clone(),
            );

            let tick_metrics = tick(massa_metrics.tick_delay);
            let tick_try_connect = tick(config.try_connection_timer.to_duration());

            //Try to connect to peers
            loop {
                select! {
                    recv(protocol_channels.connectivity_thread.1) -> msg => {
                        // update channel metrics
                        protocol_channels.connectivity_thread.1.update_metrics();
                        match msg {
                            Ok(ConnectivityCommand::Stop) => {
                                println!("Stopping protocol");
                                drop(network_controller);
                                println!("Stoppeed network controller");
                                operation_handler.stop();
                                println!("Stopped operation handler");
                                endorsement_handler.stop();
                                println!("Stopped endorsement handler");
                                block_handler.stop();
                                println!("Stopped block handler");
                                peer_management_handler.stop();
                                println!("Stopped peer handler");
                                break;
                            },
                            Ok(ConnectivityCommand::GetStats { responder }) => {
                                let active_node_count = network_controller.get_active_connections().get_peer_ids_connected().len() as u64;
                                let in_connection_count = network_controller.get_active_connections().get_nb_in_connections() as u64;
                                let out_connection_count = network_controller.get_active_connections().get_nb_out_connections() as u64;
                                let (banned_peer_count, known_peer_count) = {
                                    let peer_db_read = peer_db.read();
                                    (peer_db_read.get_banned_peer_count(), peer_db_read.peers.len() as u64)
                                };
                                let stats = NetworkStats {
                                    active_node_count,
                                    in_connection_count,
                                    out_connection_count,
                                    banned_peer_count,
                                    known_peer_count,
                                };
                                let peers: HashMap<PeerId, (SocketAddr, PeerConnectionType)> = network_controller.get_active_connections().get_peers_connected().into_iter().map(|(peer_id, peer)| {
                                    (peer_id, (peer.0, peer.1))
                                }).collect();
                                responder.try_send((stats, peers)).unwrap_or_else(|_| warn!("Failed to send stats to responder"));
                            }
                            Err(_) => {
                                warn!("Channel to connectivity thread is closed. Stopping the protocol");
                                break;
                            }
                        }
                    },
                    recv(tick_metrics) -> _ => {
                        massa_metrics.inc_peernet_total_bytes_receive(network_controller.get_total_bytes_received());

                        massa_metrics.inc_peernet_total_bytes_sent(network_controller.get_total_bytes_sent());

                        let active_conn = network_controller.get_active_connections();
                        massa_metrics.set_active_connections(active_conn.get_nb_in_connections(), active_conn.get_nb_out_connections());
                        let peers_map = active_conn.get_peers_connections_bandwidth();
                        massa_metrics.update_peers_tx_rx(peers_map);
                    },
                    recv(tick_try_connect) -> _ => {
                        let active_conn = network_controller.get_active_connections();
                        let peers_connected = active_conn.get_peers_connected();
                        let peers_connection_queue = active_conn.get_peer_ids_connection_queue();
                        let mut slots_per_category: Vec<(String, usize)> = peer_categories.iter().map(|(category, category_infos)| {
                            (category.clone(), category_infos.1.target_out_connections.saturating_sub(peers_connected.iter().filter(|(_, peer)| {
                                if peer.1 == PeerConnectionType::OUT && let Some(peer_category) = &peer.2 {
                                    category == peer_category
                                } else {
                                    false
                                }
                            }).count()))
                        }).collect();
                        let mut slot_default_category = config.default_category_info.target_out_connections.saturating_sub(peers_connected.iter().filter(|(_, peer)| {
                            peer.1 == PeerConnectionType::OUT && peer.2.is_none()
                        }).count());

                        // Get all the addresses we can cannot to, without any filter or priorisation done yet
                        let mut addresses_can_connect  = Vec::new();
                        {
                            let peer_db_read = peer_db.read();
                            for (peer_id, peer_info) in &peer_db_read.peers {
                                if peers_connected.contains_key(peer_id) {
                                    continue;
                                }

                                if peer_info.state == PeerState::Trusted {
                                    if let Some(ref last_announce) = peer_info.last_announce {
                                        if last_announce.listeners.is_empty() {
                                            continue;
                                        }

                                        for (addr, _) in last_announce.listeners.iter() {
                                            if peers_connection_queue.contains(addr) {
                                                continue;
                                            }

                                            let connection_metadata = peer_db_read.try_connect_history.get(addr).cloned().unwrap_or(ConnectionMetadata::default());

                                            // check if the peer last connect attempt has not been too recent
                                            if let ConnectionMetadata { last_try: Some(lt), .. } = connection_metadata {
                                                let last_try = lt.estimate_instant().expect("Time went backward");
                                                if last_try.elapsed() < config.try_connection_timer_same_peer.to_duration() {
                                                    continue;
                                                }
                                            }

                                            if config.listeners.iter().any(|(local_addr, _transport)| addr == local_addr) {
                                                continue;
                                            }

                                            let canonical_ip = addr.ip().to_canonical();
                                            let mut allowed_local_ips = false;
                                            // Check if the peer is in a category and we didn't reached out target yet
                                            let mut category_found = None;
                                            for (name, (ips, cat)) in &peer_categories {
                                                if ips.contains(&canonical_ip) {
                                                    category_found = Some(name);
                                                    allowed_local_ips = cat.allow_local_peers;
                                                }
                                            }
                                            if !canonical_ip.is_global() && !allowed_local_ips {
                                                continue;
                                            }

                                            addresses_can_connect.push((*addr, connection_metadata, category_found));
                                            break;
                                        }
                                    }
                                }
                            }
                        }

                        // Sort addresses using the metadata
                        addresses_can_connect.sort_by(|a, b| a.1.cmp(&b.1));

                        // Connect to the given addresses, trying to fill all the slots available
                        let mut addresses_connected = vec![];
                        for (addr, _, category) in addresses_can_connect.iter() {
                            if addresses_connected.contains(addr) {
                                continue;
                            }

                            // Connect to the peer
                            match category {

                                // In case has a special category
                                Some(cat) => {
                                    for (name, category_infos) in &mut slots_per_category {
                                        if name == *cat && *category_infos > 0 {
                                            // In case the connection succeeds, we take a place in a slot
                                            if try_connect_peer(*addr, &mut network_controller, &peer_db, &config).is_err() {
                                                *category_infos -= 1;
                                                addresses_connected.push(*addr);
                                            }
                                        }
                                    }
                                }

                                // Default category
                                None if slot_default_category > 0 => {
                                    // In case the connection succeeds, we take a place in a slot
                                    if try_connect_peer(*addr, &mut network_controller, &peer_db, &config).is_err() {
                                        slot_default_category -= 1;
                                        addresses_connected.push(*addr);
                                    }
                                }
                                None => continue,
                            }

                            // IF all slots are filled, stop
                            if slot_default_category == 0 && slots_per_category.iter().all(|(_, slots)| *slots == 0) {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }).expect("OS failed to start connectivity thread");

    // Start controller
    Ok((protocol_channels.connectivity_thread.0, handle))
}

// Attempt to connect to peer
fn try_connect_peer(
    addr: SocketAddr,
    network_controller: &mut Box<dyn NetworkController>,
    peer_db: &Arc<RwLock<PeerDB>>,
    config: &ProtocolConfig,
) -> Result<(), ProtocolError> {
    info!("Trying to connect to addr {}", addr);

    let conn_res = network_controller.try_connect(addr, config.timeout_connection.to_duration());
    {
        let mut peer_db_write = peer_db.write();
        if let Some(md) = peer_db_write.try_connect_history.get_mut(&addr) {
            if conn_res.is_ok() {
                md.new_success();
            } else {
                md.new_failure();
            }
        } else {
            let mut md = ConnectionMetadata::default();
            if conn_res.is_ok() {
                md.new_success();
            } else {
                md.new_failure();
            }
            peer_db_write.try_connect_history.insert(addr, md);
        }
    }
    if let Err(ref err) = conn_res {
        warn!("Failed to connect to peer {:?}: {:?}", addr, err);
    }
    conn_res
}
