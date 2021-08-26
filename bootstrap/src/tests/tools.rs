// Copyright (c) 2021 MASSA LABS <info@massa.net>
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use super::mock_establisher::{ReadHalf, WriteHalf};
use crate::config::BootstrapConfig;
use bitvec::prelude::*;
use communication::network::{BootstrapPeers, NetworkCommand};
use consensus::{
    BootstrapableGraph, ConsensusCommand, ExportActiveBlock, ExportClique, ExportProofOfStake,
    ExportThreadCycleState, LedgerChange, LedgerData, LedgerExport, RollUpdate,
};
use crypto::hash::Hash;
use crypto::signature::{derive_public_key, generate_random_private_key, PrivateKey, PublicKey};
use models::{
    Address, Amount, Block, BlockHeader, BlockHeaderContent, BlockId, DeserializeCompact,
    Endorsement, EndorsementContent, Operation, OperationContent, SerializeCompact, Slot,
};
use std::str::FromStr;
use time::UTime;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc::Receiver,
    time::sleep,
};

pub const BASE_BOOTSTRAP_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(169, 202, 0, 10));

pub fn get_dummy_block_id(s: &str) -> BlockId {
    BlockId(Hash::hash(s.as_bytes()))
}

pub fn get_random_public_key() -> PublicKey {
    let priv_key = crypto::generate_random_private_key();
    crypto::derive_public_key(&priv_key)
}

pub fn get_random_address() -> Address {
    let priv_key = crypto::generate_random_private_key();
    let pub_key = crypto::derive_public_key(&priv_key);
    Address::from_public_key(&pub_key).unwrap()
}

pub fn get_dummy_signature(s: &str) -> crypto::signature::Signature {
    let priv_key = crypto::generate_random_private_key();
    crypto::sign(&Hash::hash(&s.as_bytes()), &priv_key).unwrap()
}

pub fn get_bootstrap_config(bootstrap_public_key: PublicKey) -> BootstrapConfig {
    // Init the serialization context with a default,
    // can be overwritten with a more specific one in the test.
    models::init_serialization_context(models::SerializationContext {
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
        max_block_size: 3 * 1024 * 1024,
        max_bootstrap_blocks: 100,
        max_bootstrap_cliques: 100,
        max_bootstrap_deps: 100,
        max_bootstrap_children: 100,
        max_ask_blocks_per_message: 10,
        max_operations_per_message: 1024,
        max_endorsements_per_message: 1024,
        max_bootstrap_message_size: 100000000,
        max_bootstrap_pos_entries: 1000,
        max_bootstrap_pos_cycles: 5,
        max_block_endorsments: 8,
    });

    BootstrapConfig {
        bind: Some("0.0.0.0:31244".parse().unwrap()),
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
        max_bootstrap_pos_entries: 1000,
        max_bootstrap_pos_cycles: 5,
        bootstrap_list: vec![(SocketAddr::new(BASE_BOOTSTRAP_IP, 16), bootstrap_public_key)],
        enable_clock_synchronization: true,
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

pub fn get_boot_state() -> (ExportProofOfStake, BootstrapableGraph) {
    let private_key = crypto::generate_random_private_key();
    let public_key = crypto::derive_public_key(&private_key);
    let address = Address::from_public_key(&public_key).unwrap();

    let mut ledger_subset = Vec::new();
    ledger_subset.push((
        address,
        LedgerData {
            balance: Amount::from_str("10").unwrap(),
        },
    ));

    let cycle_state = ExportThreadCycleState {
        cycle: 1,
        last_final_slot: Slot::new(1, 1),
        roll_count: vec![(get_random_address(), 123), (get_random_address(), 456)],
        cycle_updates: vec![
            (
                get_random_address(),
                RollUpdate {
                    roll_purchases: 147,
                    roll_sales: 44788,
                },
            ),
            (
                get_random_address(),
                RollUpdate {
                    roll_purchases: 8887,
                    roll_sales: 114,
                },
            ),
        ],
        rng_seed: bitvec![Lsb0, u8 ; 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1],
        production_stats: vec![(get_random_address(), 1, 2), (get_random_address(), 3, 4)],
    };
    let boot_pos = ExportProofOfStake {
        cycle_states: vec![
            vec![cycle_state.clone()].into_iter().collect(),
            vec![cycle_state.clone()].into_iter().collect(),
        ],
    };

    let block1 = ExportActiveBlock {
        block: Block {
            header: BlockHeader {
                content: BlockHeaderContent {
                    creator: get_random_public_key(),
                    slot: Slot::new(1, 1),
                    parents: vec![get_dummy_block_id("p1"), get_dummy_block_id("p2")],
                    operation_merkle_root: Hash::hash("op_hash".as_bytes()),
                    endorsements: vec![
                        Endorsement {
                            content: EndorsementContent {
                                sender_public_key: get_random_public_key(),
                                slot: Slot::new(1, 0),
                                index: 1,
                                endorsed_block: get_dummy_block_id("p1"),
                            },
                            signature: get_dummy_signature("dummy_sig_0"),
                        },
                        Endorsement {
                            content: EndorsementContent {
                                sender_public_key: get_random_public_key(),
                                slot: Slot::new(4, 1),
                                index: 3,
                                endorsed_block: get_dummy_block_id("p1"),
                            },
                            signature: get_dummy_signature("dummy_sig_00"),
                        },
                    ],
                },
                signature: get_dummy_signature("dummy_sig_1"),
            },
            operations: vec![
                Operation {
                    content: OperationContent {
                        sender_public_key: get_random_public_key(),
                        fee: Amount::from_str("1524878").unwrap(),
                        expire_period: 5787899,
                        op: models::OperationType::Transaction {
                            recipient_address: get_random_address(),
                            amount: Amount::from_str("1259787").unwrap(),
                        },
                    },
                    signature: get_dummy_signature("dummy_sig_2"),
                },
                Operation {
                    content: OperationContent {
                        sender_public_key: get_random_public_key(),
                        fee: Amount::from_str("878763222").unwrap(),
                        expire_period: 4557887,
                        op: models::OperationType::RollBuy { roll_count: 45544 },
                    },
                    signature: get_dummy_signature("dummy_sig_3"),
                },
                Operation {
                    content: OperationContent {
                        sender_public_key: get_random_public_key(),
                        fee: Amount::from_str("4545").unwrap(),
                        expire_period: 452524,
                        op: models::OperationType::RollSell {
                            roll_count: 4888787,
                        },
                    },
                    signature: get_dummy_signature("dummy_sig_4"),
                },
            ],
        },
        parents: vec![
            (get_dummy_block_id("b1"), 4777),
            (get_dummy_block_id("b2"), 8870),
        ],
        children: vec![
            vec![
                (get_dummy_block_id("b3"), 101),
                (get_dummy_block_id("b4"), 455),
            ],
            vec![(get_dummy_block_id("b3_2"), 889)],
        ],
        dependencies: vec![get_dummy_block_id("b5"), get_dummy_block_id("b6")],
        is_final: true,
        block_ledger_changes: vec![
            (
                get_random_address(),
                LedgerChange {
                    balance_increment: true,
                    balance_delta: Amount::from_str("157").unwrap(),
                },
            ),
            (
                get_random_address(),
                LedgerChange {
                    balance_increment: false,
                    balance_delta: Amount::from_str("44").unwrap(),
                },
            ),
            (
                get_random_address(),
                LedgerChange {
                    balance_increment: false,
                    balance_delta: Amount::from_str("878").unwrap(),
                },
            ),
        ],
        roll_updates: vec![
            (
                get_random_address(),
                RollUpdate {
                    roll_purchases: 778,
                    roll_sales: 54851,
                },
            ),
            (
                get_random_address(),
                RollUpdate {
                    roll_purchases: 788778,
                    roll_sales: 11451,
                },
            ),
        ],
        production_events: vec![
            (12, get_random_address(), true),
            (31, get_random_address(), false),
        ],
    };
    assert_eq!(
        ExportProofOfStake::from_bytes_compact(&boot_pos.to_bytes_compact().unwrap())
            .unwrap()
            .0
            .to_bytes_compact()
            .unwrap(),
        boot_pos.to_bytes_compact().unwrap(),
        "ExportProofOfStake serialization inconsistent"
    );

    let boot_graph = BootstrapableGraph {
        active_blocks: vec![(get_dummy_block_id("block1"), block1)],
        best_parents: vec![get_dummy_block_id("parent1"), get_dummy_block_id("parent2")],
        latest_final_blocks_periods: vec![
            (get_dummy_block_id("parent1"), 10),
            (get_dummy_block_id("parent2"), 10),
        ],
        gi_head: vec![
            (get_dummy_block_id("parent1"), vec![]),
            (get_dummy_block_id("parent2"), vec![]),
        ],
        max_cliques: vec![ExportClique {
            block_ids: vec![get_dummy_block_id("parent1"), get_dummy_block_id("parent2")],
            fitness: 123,
            is_blockclique: true,
        }],
        ledger: LedgerExport { ledger_subset },
    };
    assert_eq!(
        BootstrapableGraph::from_bytes_compact(&boot_graph.to_bytes_compact().unwrap())
            .unwrap()
            .0
            .to_bytes_compact()
            .unwrap(),
        boot_graph.to_bytes_compact().unwrap(),
        "BootstrapableGraph serialization inconsistent"
    );

    (boot_pos, boot_graph)
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
