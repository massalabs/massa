use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr},
};

use super::mock_establisher::{ReadHalf, WriteHalf};
use consensus::{BoostrapableGraph, ConsensusCommand};
use crypto::{
    hash::Hash,
    signature::{PrivateKey, PublicKey, SignatureEngine},
};
use models::SerializationContext;
use time::UTime;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc::Receiver,
    time::sleep,
};

use crate::config::BootstrapConfig;

pub const BASE_BOOTSTRAP_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(169, 202, 0, 10));

pub fn get_bootstrap_config(bootstrap_public_key: PublicKey) -> BootstrapConfig {
    BootstrapConfig {
        bootstrap_ip: BASE_BOOTSTRAP_IP,
        bootstrap_port: 16,
        bootstrap_public_key,
        listen_port: Some(format!("0.0.0.0:{}", 10).parse().unwrap()),
        connect_timeout: 200.into(),
        bootstrap_time_after_genesis: 100.into(),
        retry_delay: 200.into(),
    }
}

pub fn get_keys() -> (PrivateKey, PublicKey) {
    let secp = SignatureEngine::new();
    let private_key = SignatureEngine::generate_random_private_key();
    let public_key = secp.derive_public_key(&private_key);
    (private_key, public_key)
}

pub fn get_serialization_context() -> SerializationContext {
    SerializationContext {
        max_block_size: 1024 * 1024,
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
    }
}

pub async fn wait_consensus_command<F, T>(
    consensus_command_receiver: &mut Receiver<ConsensusCommand>,
    timeout: UTime,
    filter_map: F,
) -> Option<T>
where
    F: Fn(ConsensusCommand) -> Option<T>,
{
    let timer = sleep(timeout.into());
    tokio::pin!(timer);
    loop {
        tokio::select! {
            cmd = consensus_command_receiver.recv() => match cmd {
                Some(orig_evt) => if let Some(res_evt) = filter_map(orig_evt) { return Some(res_evt); },
                _ => panic!("network event channel died")
            },
            _ = &mut timer => return None
        }
    }
}

pub fn get_boot_graph() -> BoostrapableGraph {
    BoostrapableGraph {
        active_blocks: Default::default(),
        best_parents: vec![
            Hash::hash(&"parent1".as_bytes()),
            Hash::hash(&"parent2".as_bytes()),
        ],
        latest_final_blocks_periods: vec![
            (Hash::hash(&"parent1".as_bytes()), 10),
            (Hash::hash(&"parent2".as_bytes()), 10),
        ],
        gi_head: Default::default(),
        max_cliques: vec![HashSet::new()],
    }
}

pub async fn bridge_mock_streams(mut read_side: ReadHalf, mut write_side: WriteHalf) {
    let mut buf = vec![0; 1024];
    loop {
        let n = read_side
            .read(&mut buf)
            .await
            .expect("could not read read_side in bridge");
        if n == 0 {
            return;
        }
        if write_side.write_all(&buf[0..n]).await.is_err() {
            return;
        }
    }
}
