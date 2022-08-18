// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::mock_establisher::Duplex;
use crate::settings::BootstrapConfig;
use bitvec::vec::BitVec;
use massa_async_pool::test_exports::{create_async_pool, get_random_message};
use massa_consensus_exports::commands::ConsensusCommand;
use massa_final_state::test_exports::create_final_state;
use massa_final_state::{ExecutedOps, FinalState};
use massa_graph::{export_active_block::ExportActiveBlock, BootstrapableGraph};
use massa_graph::{BootstrapableGraphDeserializer, BootstrapableGraphSerializer};
use massa_hash::Hash;
use massa_ledger_exports::LedgerEntry;
use massa_ledger_worker::test_exports::create_final_ledger;
use massa_models::constants::default::{
    MAX_LEDGER_CHANGES_PER_SLOT, MAX_PRODUCTION_EVENTS_PER_BLOCK,
};
use massa_models::constants::{
    MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_CHILDREN, MAX_BOOTSTRAP_CLIQUES, MAX_BOOTSTRAP_DEPS,
    MAX_BOOTSTRAP_MESSAGE_SIZE, MAX_BOOTSTRAP_POS_ENTRIES, MAX_OPERATIONS_PER_BLOCK, THREAD_COUNT,
};
use massa_models::prehash::Map;
use massa_models::wrapped::WrappedContent;
use massa_models::{
    clique::Clique, Address, Amount, Block, BlockHeader, BlockHeaderSerializer, BlockId,
    Endorsement, Slot,
};
use massa_models::{BlockSerializer, EndorsementSerializer};
use massa_network_exports::{BootstrapPeers, NetworkCommand};
use massa_pos_exports::{CycleInfo, DeferredCredits, PoSFinalState, ProductionStats};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_signature::{KeyPair, PublicKey, Signature};
use massa_time::MassaTime;
use rand::Rng;
use std::collections::{HashMap, VecDeque};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::{sync::mpsc::Receiver, time::sleep};

pub const BASE_BOOTSTRAP_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(169, 202, 0, 10));

/// generates a small random number of bytes
fn get_some_random_bytes() -> Vec<u8> {
    let mut rng = rand::thread_rng();
    (0usize..rng.gen_range(1..10))
        .map(|_| rand::random::<u8>())
        .collect()
}

/// generates a random ledger entry
fn get_random_ledger_entry() -> LedgerEntry {
    let mut rng = rand::thread_rng();
    let parallel_balance = Amount::from_raw(rng.gen::<u64>());
    let sequential_balance = Amount::from_raw(rng.gen::<u64>());
    let bytecode: Vec<u8> = get_some_random_bytes();
    let mut datastore = BTreeMap::new();
    for _ in 0usize..rng.gen_range(0..10) {
        let key = get_some_random_bytes();
        let value = get_some_random_bytes();
        datastore.insert(key, value);
    }
    LedgerEntry {
        sequential_balance,
        parallel_balance,
        bytecode,
        datastore,
    }
}

/// generates random PoS cycles info
fn get_random_pos_cycles_info(
    r_limit: u64,
) -> (Map<Address, u64>, Map<Address, ProductionStats>, BitVec<u8>) {
    let mut rng = rand::thread_rng();
    let mut roll_counts = Map::default();
    let mut production_stats = Map::default();
    let mut rng_seed: BitVec<u8> = BitVec::default();

    for i in 0u64..r_limit {
        roll_counts.insert(get_random_address(), i);
        production_stats.insert(
            get_random_address(),
            ProductionStats {
                block_success_count: i * 3,
                block_failure_count: i,
            },
        );
        rng_seed.push(rng.gen_range(0..2) == 1);
    }
    (roll_counts, production_stats, rng_seed)
}

/// generates random PoS deferred credits
fn get_random_deferred_credits(r_limit: u64) -> DeferredCredits {
    let mut deferred_credits = DeferredCredits::default();

    for i in 0u64..r_limit {
        let mut credits = Map::default();
        for j in 0u64..r_limit {
            credits.insert(get_random_address(), Amount::from_raw(j));
        }
        deferred_credits.0.insert(
            Slot {
                period: i,
                thread: 0,
            },
            credits,
        );
    }
    deferred_credits
}

/// generates a random PoS final state
fn get_random_pos_state() -> PoSFinalState {
    let mut rng = rand::thread_rng();
    let mut cycle_history = VecDeque::new();
    let r_limit: u64 = rng.gen_range(20..30);
    println!("R_LIMIT = {}", r_limit);
    for i in 0u64..r_limit {
        let (roll_counts, production_stats, rng_seed) = get_random_pos_cycles_info(r_limit);
        cycle_history.push_front(CycleInfo {
            cycle: i as u64,
            roll_counts,
            complete: true,
            rng_seed,
            production_stats,
        });
    }
    let deferred_credits = get_random_deferred_credits(r_limit);
    PoSFinalState {
        cycle_history,
        deferred_credits,
        selector: None,
    }
}

/// generates a random bootstrap state for the final state
pub fn get_random_final_state_bootstrap(thread_count: u8) -> FinalState {
    let mut rng = rand::thread_rng();

    let mut sorted_ledger = HashMap::new();
    let mut messages = BTreeMap::new();
    for _ in 0usize..rng.gen_range(3..10) {
        let message = get_random_message();
        messages.insert(message.compute_id(), message);
    }
    for _ in 0usize..rng.gen_range(5..10) {
        sorted_ledger.insert(get_random_address(), get_random_ledger_entry());
    }

    let slot = Slot::new(rng.gen::<u64>(), rng.gen_range(0..thread_count));
    let final_ledger = create_final_ledger(Some(sorted_ledger), Default::default());
    let async_pool = create_async_pool(Default::default(), messages);
    create_final_state(
        Default::default(),
        slot,
        Box::new(final_ledger),
        async_pool,
        VecDeque::new(),
        get_random_pos_state(),
        ExecutedOps::default(),
    )
}

pub fn get_dummy_block_id(s: &str) -> BlockId {
    BlockId(Hash::compute_from(s.as_bytes()))
}

pub fn get_random_public_key() -> PublicKey {
    let priv_key = KeyPair::generate();
    priv_key.get_public_key()
}

pub fn get_random_address() -> Address {
    let priv_key = KeyPair::generate();
    Address::from_public_key(&priv_key.get_public_key())
}

pub fn get_dummy_signature(s: &str) -> Signature {
    let priv_key = KeyPair::generate();
    priv_key.sign(&Hash::compute_from(s.as_bytes())).unwrap()
}

pub fn get_bootstrap_config(bootstrap_public_key: PublicKey) -> BootstrapConfig {
    BootstrapConfig {
        bind: Some("0.0.0.0:31244".parse().unwrap()),
        connect_timeout: 200.into(),
        retry_delay: 200.into(),
        max_ping: MassaTime::from(500),
        read_timeout: 1000.into(),
        write_timeout: 1000.into(),
        read_error_timeout: 200.into(),
        write_error_timeout: 200.into(),
        bootstrap_list: vec![(SocketAddr::new(BASE_BOOTSTRAP_IP, 16), bootstrap_public_key)],
        enable_clock_synchronization: true,
        cache_duration: 10000.into(),
        max_simultaneous_bootstraps: 2,
        ip_list_max_size: 10,
        per_ip_min_interval: 10000.into(),
        max_bytes_read_write: std::f64::INFINITY,
        max_bootstrap_message_size: MAX_BOOTSTRAP_MESSAGE_SIZE,
    }
}

pub async fn wait_consensus_command<F, T>(
    consensus_command_receiver: &mut Receiver<ConsensusCommand>,
    timeout: MassaTime,
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
    timeout: MassaTime,
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

/// asserts that two `BootstrapableGraph` are equal
pub fn assert_eq_bootstrap_graph(v1: &BootstrapableGraph, v2: &BootstrapableGraph) {
    assert_eq!(
        v1.active_blocks.len(),
        v2.active_blocks.len(),
        "length mismatch"
    );
    println!("id = {:#?}", get_dummy_block_id("block1"));
    println!(
        "blocks1 = {:#?}",
        v1.active_blocks.iter().map(|b| b.0).collect::<Vec<_>>()
    );
    println!(
        "blocks2 = {:#?}",
        v2.active_blocks.iter().map(|b| b.0).collect::<Vec<_>>()
    );
    for (id1, itm1) in v1.active_blocks.iter() {
        let itm2 = v2.active_blocks.get(id1).unwrap();
        assert_eq!(
            itm1.block.serialized_data, itm2.block.serialized_data,
            "block mismatch"
        );

        assert_eq!(itm1.children, itm2.children, "children mismatch");
        assert_eq!(
            itm1.dependencies, itm2.dependencies,
            "dependencies mismatch"
        );
        assert_eq!(itm1.is_final, itm2.is_final, "is_final mismatch");
        assert_eq!(itm1.parents, itm2.parents, "parents mismatch");
    }
    assert_eq!(v1.best_parents, v2.best_parents, "best parents mismatch");
    assert_eq!(v1.gi_head, v2.gi_head, "gi_head mismatch");
    assert_eq!(
        v1.latest_final_blocks_periods, v2.latest_final_blocks_periods,
        "latest_final_blocks_periods mismatch"
    );
    assert_eq!(
        v1.max_cliques.len(),
        v2.max_cliques.len(),
        "max_cliques len mismatch"
    );
    for (itm1, itm2) in v1.max_cliques.iter().zip(v2.max_cliques.iter()) {
        assert_eq!(itm1.block_ids, itm2.block_ids, "block_ids mistmatch");
        assert_eq!(itm1.fitness, itm2.fitness, "fitness mistmatch");
        assert_eq!(
            itm1.is_blockclique, itm2.is_blockclique,
            "is_blockclique mistmatch"
        );
    }
}

pub fn get_boot_state() -> BootstrapableGraph {
    let keypair = KeyPair::generate();

    let block = Block::new_wrapped(
        Block {
            header: BlockHeader::new_wrapped(
                BlockHeader {
                    slot: Slot::new(1, 1),
                    parents: vec![get_dummy_block_id("p1"), get_dummy_block_id("p2")],
                    operation_merkle_root: Hash::compute_from("op_hash".as_bytes()),
                    endorsements: vec![
                        Endorsement::new_wrapped(
                            Endorsement {
                                slot: Slot::new(1, 0),
                                index: 1,
                                endorsed_block: get_dummy_block_id("p1"),
                            },
                            EndorsementSerializer::new(),
                            &keypair,
                        )
                        .unwrap(),
                        Endorsement::new_wrapped(
                            Endorsement {
                                slot: Slot::new(4, 1),
                                index: 3,
                                endorsed_block: get_dummy_block_id("p1"),
                            },
                            EndorsementSerializer::new(),
                            &keypair,
                        )
                        .unwrap(),
                    ],
                },
                BlockHeaderSerializer::new(),
                &keypair,
            )
            .unwrap(),
            operations: Default::default(),
        },
        BlockSerializer::new(),
        &keypair,
    )
    .unwrap();

    let block_id = block.id;

    //TODO: We currently lost information. Need to use shared storage
    let block1 = ExportActiveBlock {
        block,
        block_id,
        parents: vec![
            (get_dummy_block_id("b1"), 4777),
            (get_dummy_block_id("b2"), 8870),
        ],
        children: vec![
            vec![
                (get_dummy_block_id("b3"), 101),
                (get_dummy_block_id("b4"), 455),
            ]
            .into_iter()
            .collect(),
            vec![(get_dummy_block_id("b3_2"), 889)]
                .into_iter()
                .collect(),
        ],
        dependencies: vec![get_dummy_block_id("b5"), get_dummy_block_id("b6")]
            .into_iter()
            .collect(),
        is_final: true,
    };

    let boot_graph = BootstrapableGraph {
        active_blocks: vec![(block1.block_id, block1)].into_iter().collect(),
        best_parents: vec![
            (get_dummy_block_id("parent1"), 2),
            (get_dummy_block_id("parent2"), 3),
        ],
        latest_final_blocks_periods: vec![
            (get_dummy_block_id("parent1"), 10),
            (get_dummy_block_id("parent2"), 10),
        ],
        gi_head: vec![
            (get_dummy_block_id("parent1"), Default::default()),
            (get_dummy_block_id("parent2"), Default::default()),
        ]
        .into_iter()
        .collect(),
        max_cliques: vec![Clique {
            block_ids: vec![get_dummy_block_id("parent1"), get_dummy_block_id("parent2")]
                .into_iter()
                .collect(),
            fitness: 123,
            is_blockclique: true,
        }],
    };

    let bootstrapable_graph_serializer = BootstrapableGraphSerializer::new();
    let bootstrapable_graph_deserializer = BootstrapableGraphDeserializer::new(
        THREAD_COUNT,
        9,
        MAX_BOOTSTRAP_BLOCKS,
        MAX_BOOTSTRAP_CLIQUES,
        MAX_BOOTSTRAP_CHILDREN,
        MAX_BOOTSTRAP_DEPS,
        MAX_BOOTSTRAP_POS_ENTRIES,
        MAX_OPERATIONS_PER_BLOCK,
        MAX_LEDGER_CHANGES_PER_SLOT,
        MAX_PRODUCTION_EVENTS_PER_BLOCK,
    );

    let mut bootstrapable_graph_serialized = Vec::new();
    bootstrapable_graph_serializer
        .serialize(&boot_graph, &mut bootstrapable_graph_serialized)
        .unwrap();
    let (_, bootstrapable_graph_deserialized) = bootstrapable_graph_deserializer
        .deserialize::<DeserializeError>(&bootstrapable_graph_serialized)
        .unwrap();

    assert_eq_bootstrap_graph(&bootstrapable_graph_deserialized, &boot_graph);

    boot_graph
}

pub fn get_peers() -> BootstrapPeers {
    BootstrapPeers(vec![
        "82.245.123.77".parse().unwrap(),
        "82.220.123.78".parse().unwrap(),
    ])
}

pub async fn bridge_mock_streams(mut side1: Duplex, mut side2: Duplex) {
    let mut buf1 = vec![0u8; 1024];
    let mut buf2 = vec![0u8; 1024];
    loop {
        tokio::select! {
            res1 = side1.read(&mut buf1) => match res1 {
                Ok(n1) => {
                    if n1 == 0 {
                        return;
                    }
                    if side2.write_all(&buf1[..n1]).await.is_err() {
                        return;
                    }
                },
                Err(_err) => {
                    return;
                }
            },
            res2 = side2.read(&mut buf2) => match res2 {
                Ok(n2) => {
                    if n2 == 0 {
                        return;
                    }
                    if side1.write_all(&buf2[..n2]).await.is_err() {
                        return;
                    }
                },
                Err(_err) => {
                    return;
                }
            },
        }
    }
}
