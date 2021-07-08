use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use super::mock_establisher::{ReadHalf, WriteHalf};
use communication::network::{BootstrapPeers, NetworkCommand};
use consensus::{BootsrapableGraph, ConsensusCommand, LedgerExport};
use crypto::signature::{derive_public_key, generate_random_private_key, PrivateKey, PublicKey};
use models::BlockId;
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
        bootstrap_addr: Some(SocketAddr::new(BASE_BOOTSTRAP_IP, 16)),
        bootstrap_public_key,
        bind: Some("0.0.0.0:31234".parse().unwrap()),
        connect_timeout: 200.into(),
        retry_delay: 200.into(),
        max_bootstrap_blocks: 100,
        max_bootstrap_cliques: 100,
        max_bootstrap_deps: 100,
        max_bootstrap_children: 100,
        max_ping: UTime::from(500),
        max_bootstrap_message_size: 100000000,
        read_timeout: 1000.into(),
        write_timeout: 1000.into(),
    }
}

pub fn get_keys() -> (PrivateKey, PublicKey) {
    let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);
    (private_key, public_key)
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

pub async fn wait_network_command<F, T>(
    network_command_receiver: &mut Receiver<NetworkCommand>,
    timeout: UTime,
    filter_map: F,
) -> Option<T>
where
    F: Fn(NetworkCommand) -> Option<T>,
{
    let timer = sleep(timeout.into());
    tokio::pin!(timer);
    loop {
        tokio::select! {
            cmd = network_command_receiver.recv() => match cmd {
                Some(orig_evt) => if let Some(res_evt) = filter_map(orig_evt) { return Some(res_evt); },
                _ => panic!("network event channel died")
            },
            _ = &mut timer => return None
        }
    }
}

pub fn get_boot_graph() -> BootsrapableGraph {
    BootsrapableGraph {
        active_blocks: Default::default(),
        best_parents: vec![
            BlockId::for_tests("parent1").unwrap(),
            BlockId::for_tests("parent2").unwrap(),
        ],
        latest_final_blocks_periods: vec![
            (BlockId::for_tests("parent1").unwrap(), 10),
            (BlockId::for_tests("parent2").unwrap(), 10),
        ],
        gi_head: Default::default(),
        max_cliques: vec![Vec::new()],
        ledger: LedgerExport::new(2),
    }
}

pub fn get_peers() -> BootstrapPeers {
    BootstrapPeers(vec![
        "82.245.123.77".parse().unwrap(),
        "82.220.123.78".parse().unwrap(),
    ])
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
