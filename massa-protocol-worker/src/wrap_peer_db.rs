use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    time::Duration,
};

use massa_protocol_exports::{PeerId, TransportType};

#[cfg(test)]
use std::sync::{Arc, RwLock};

#[cfg_attr(test, mockall_wrap::wrap, mockall::automock)]
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
}

impl Clone for Box<dyn PeerDBTrait> {
    fn clone(&self) -> Box<dyn PeerDBTrait> {
        self.clone_box()
    }
}
