use crate::handlers::peer_handler::models::{ConnectionMetadata, PeerInfo};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    time::Duration,
};

use massa_protocol_exports::{PeerId, TransportType};

#[cfg_attr(test, mockall::automock)]
pub trait PeerDBTrait: Send + Sync {
    fn ban_peer(&mut self, peer_id: &PeerId);
    fn unban_peer(&mut self, peer_id: &PeerId);
    fn clone_box(&self) -> Box<dyn PeerDBTrait>;
    fn get_oldest_peer(
        &self,
        cooldown: Duration,
        in_test: &HashSet<SocketAddr>,
    ) -> Option<SocketAddr>;
    fn get_rand_peers_to_send(
        &self,
        nb_peers: usize,
    ) -> Vec<(PeerId, HashMap<SocketAddr, TransportType>)>;
    fn get_banned_peer_count(&self) -> u64;
    fn get_known_peer_count(&self) -> u64;
    fn get_peers(&self) -> &HashMap<PeerId, PeerInfo>;
    fn get_peers_mut(&mut self) -> &mut HashMap<PeerId, PeerInfo>;
    fn get_connection_metadata_or_default(&self, addr: &SocketAddr) -> ConnectionMetadata;
    fn set_try_connect_success_or_insert(&mut self, addr: &SocketAddr);
    fn set_try_connect_failure_or_insert(&mut self, addr: &SocketAddr);
    fn set_try_connect_test_success_or_insert(&mut self, addr: &SocketAddr);
    fn set_try_connect_test_failure_or_insert(&mut self, addr: &SocketAddr);
    fn insert_peer_in_test(&mut self, addr: &SocketAddr) -> bool;
    fn remove_peer_in_test(&mut self, addr: &SocketAddr) -> bool;
    fn get_peers_in_test(&self) -> &HashSet<SocketAddr>;
    fn insert_tested_address(&mut self, addr: &SocketAddr, time: massa_time::MassaTime);
    fn get_tested_addresses(&self) -> &HashMap<SocketAddr, massa_time::MassaTime>;
}

impl Clone for Box<dyn PeerDBTrait> {
    fn clone(&self) -> Box<dyn PeerDBTrait> {
        self.clone_box()
    }
}
