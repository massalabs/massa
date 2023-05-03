// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::settings::{BootstrapConfig, IpType};
use bitvec::vec::BitVec;
use massa_async_pool::test_exports::{create_async_pool, get_random_message};
use massa_async_pool::{AsyncPoolChanges, Change};
use massa_consensus_exports::{
    bootstrapable_graph::{
        BootstrapableGraph, BootstrapableGraphDeserializer, BootstrapableGraphSerializer,
    },
    export_active_block::{ExportActiveBlock, ExportActiveBlockSerializer},
};
use massa_executed_ops::{
    ExecutedDenunciations, ExecutedDenunciationsChanges, ExecutedDenunciationsConfig, ExecutedOps,
    ExecutedOpsConfig,
};
use massa_final_state::test_exports::create_final_state;
use massa_final_state::{FinalState, FinalStateConfig};
use massa_hash::Hash;
use massa_ledger_exports::{LedgerChanges, LedgerEntry, SetUpdateOrDelete};
use massa_ledger_worker::test_exports::create_final_ledger;
use massa_models::block::BlockDeserializerArgs;
use massa_models::bytecode::Bytecode;
use massa_models::config::{
    BOOTSTRAP_RANDOMNESS_SIZE_BYTES, CONSENSUS_BOOTSTRAP_PART_SIZE, ENDORSEMENT_COUNT,
    MAX_ADVERTISE_LENGTH, MAX_ASYNC_MESSAGE_DATA, MAX_ASYNC_POOL_LENGTH,
    MAX_BOOTSTRAP_ASYNC_POOL_CHANGES, MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_ERROR_LENGTH,
    MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_CONSENSUS_BLOCKS_IDS, MAX_DATASTORE_ENTRY_COUNT,
    MAX_DATASTORE_KEY_LENGTH, MAX_DATASTORE_VALUE_LENGTH, MAX_DEFERRED_CREDITS_LENGTH,
    MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_DENUNCIATION_CHANGES_LENGTH,
    MAX_EXECUTED_OPS_CHANGES_LENGTH, MAX_EXECUTED_OPS_LENGTH, MAX_FUNCTION_NAME_LENGTH,
    MAX_LEDGER_CHANGES_COUNT, MAX_OPERATIONS_PER_BLOCK, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
    MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH, MAX_PARAMETERS_SIZE,
    MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH, MIP_STORE_STATS_BLOCK_CONSIDERED,
    MIP_STORE_STATS_COUNTERS_MAX, PERIODS_PER_CYCLE, THREAD_COUNT,
};
use massa_models::denunciation::DenunciationIndex;
use massa_models::node::NodeId;
use massa_models::{
    address::Address,
    amount::Amount,
    block::Block,
    block::BlockSerializer,
    block_header::{BlockHeader, BlockHeaderSerializer},
    block_id::BlockId,
    endorsement::Endorsement,
    endorsement::EndorsementSerializer,
    operation::OperationId,
    prehash::PreHashMap,
    secure_share::Id,
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pos_exports::{CycleInfo, DeferredCredits, PoSChanges, PoSFinalState, ProductionStats};
use massa_protocol_exports::{BootstrapPeers, PeerId, TransportType};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use rand::Rng;
use std::collections::{HashMap, HashSet, VecDeque};
use std::str::FromStr;
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

// Use loop-back address. use port 0 to auto-assign a port
pub const BASE_BOOTSTRAP_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

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
    let balance = Amount::from_raw(rng.gen::<u64>());
    let bytecode_bytes: Vec<u8> = get_some_random_bytes();
    let bytecode = Bytecode(bytecode_bytes);
    let mut datastore = BTreeMap::new();
    for _ in 0usize..rng.gen_range(0..10) {
        let key = get_some_random_bytes();
        let value = get_some_random_bytes();
        datastore.insert(key, value);
    }
    LedgerEntry {
        balance,
        bytecode,
        datastore,
    }
}

pub fn get_random_ledger_changes(r_limit: u64) -> LedgerChanges {
    let mut changes = LedgerChanges::default();
    for _ in 0..r_limit {
        changes.0.insert(
            get_random_address(),
            SetUpdateOrDelete::Set(LedgerEntry {
                balance: Amount::from_raw(r_limit),
                bytecode: Bytecode::default(),
                datastore: BTreeMap::default(),
            }),
        );
    }
    changes
}

/// generates random PoS cycles info
fn get_random_pos_cycles_info(
    r_limit: u64,
    opt_seed: bool,
) -> (
    BTreeMap<Address, u64>,
    PreHashMap<Address, ProductionStats>,
    BitVec<u8>,
) {
    let mut rng = rand::thread_rng();
    let mut roll_counts = BTreeMap::default();
    let mut production_stats = PreHashMap::default();
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
    }
    // note: extra seed is used in the changes test to compensate for the update loop skipping the first change
    if opt_seed {
        rng_seed.push(rng.gen_range(0..2) == 1);
    }
    rng_seed.push(rng.gen_range(0..2) == 1);
    (roll_counts, production_stats, rng_seed)
}

/// generates random PoS deferred credits
fn get_random_deferred_credits(r_limit: u64) -> DeferredCredits {
    let mut deferred_credits = DeferredCredits::new_with_hash();

    for i in 0u64..r_limit {
        let mut credits = PreHashMap::default();
        for j in 0u64..r_limit {
            credits.insert(get_random_address(), Amount::from_raw(j));
        }
        deferred_credits.credits.insert(
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
fn get_random_pos_state(r_limit: u64, pos: PoSFinalState) -> PoSFinalState {
    let mut cycle_history = VecDeque::new();
    let (roll_counts, production_stats, rng_seed) = get_random_pos_cycles_info(r_limit, true);
    let mut cycle = CycleInfo::new_with_hash(0, false, roll_counts, rng_seed, production_stats);
    cycle.final_state_hash_snapshot = Some(Hash::from_bytes(&[0; 32]));
    cycle_history.push_back(cycle);
    let mut deferred_credits = DeferredCredits::new_with_hash();
    deferred_credits.extend(get_random_deferred_credits(r_limit));
    PoSFinalState {
        cycle_history,
        deferred_credits,
        ..pos
    }
}

/// generates random PoS changes
pub fn get_random_pos_changes(r_limit: u64) -> PoSChanges {
    let deferred_credits = get_random_deferred_credits(r_limit);
    let (roll_counts, production_stats, seed_bits) = get_random_pos_cycles_info(r_limit, false);
    PoSChanges {
        seed_bits,
        roll_changes: roll_counts.into_iter().collect(),
        production_stats,
        deferred_credits,
    }
}

pub fn get_random_async_pool_changes(r_limit: u64) -> AsyncPoolChanges {
    let mut changes = AsyncPoolChanges::default();
    for _ in 0..(r_limit / 2) {
        let message = get_random_message(Some(Amount::from_str("10").unwrap()));
        changes.0.push(Change::Add(message.compute_id(), message));
    }
    for _ in (r_limit / 2)..r_limit {
        let message = get_random_message(Some(Amount::from_str("1_000_000").unwrap()));
        changes.0.push(Change::Add(message.compute_id(), message));
    }
    changes
}

pub fn get_random_executed_ops(
    _r_limit: u64,
    slot: Slot,
    config: ExecutedOpsConfig,
) -> ExecutedOps {
    let mut executed_ops = ExecutedOps::new(config.clone());
    executed_ops.apply_changes(get_random_executed_ops_changes(10), slot);
    executed_ops
}

pub fn get_random_executed_ops_changes(r_limit: u64) -> PreHashMap<OperationId, (bool, Slot)> {
    let mut ops_changes = PreHashMap::default();
    for i in 0..r_limit {
        ops_changes.insert(
            OperationId::new(Hash::compute_from(&get_some_random_bytes())),
            (
                true,
                Slot {
                    period: i + 10,
                    thread: 0,
                },
            ),
        );
    }
    ops_changes
}

pub fn get_random_executed_de(
    _r_limit: u64,
    slot: Slot,
    config: ExecutedDenunciationsConfig,
) -> ExecutedDenunciations {
    let mut executed_de = ExecutedDenunciations::new(config);
    executed_de.apply_changes(get_random_executed_de_changes(10), slot);
    executed_de
}

pub fn get_random_executed_de_changes(r_limit: u64) -> ExecutedDenunciationsChanges {
    let mut de_changes = HashSet::default();

    for i in 0..r_limit {
        if i % 2 == 0 {
            de_changes.insert(DenunciationIndex::BlockHeader {
                slot: Slot::new(i + 2, 0),
            });
        } else {
            de_changes.insert(DenunciationIndex::Endorsement {
                slot: Slot::new(i + 2, 0),
                index: i as u32,
            });
        }
    }

    de_changes
}

/// generates a random bootstrap state for the final state
pub fn get_random_final_state_bootstrap(
    pos: PoSFinalState,
    config: FinalStateConfig,
) -> FinalState {
    let r_limit: u64 = 50;

    let mut sorted_ledger = HashMap::new();
    let mut messages = AsyncPoolChanges::default();
    for _ in 0..r_limit {
        let message = get_random_message(None);
        messages.0.push(Change::Add(message.compute_id(), message));
    }
    for _ in 0..r_limit {
        sorted_ledger.insert(get_random_address(), get_random_ledger_entry());
    }
    // insert the last possible address to prevent the last cursor to move when testing the changes
    // The magic number at idx 0 is to account for address variant leader. At time of writing,
    // the highest value for encoding this variant in serialized form is `1`.
    let mut bytes = [255; 33];
    bytes[0] = 1;
    sorted_ledger.insert(
        Address::from_prefixed_bytes(&bytes).unwrap(),
        get_random_ledger_entry(),
    );

    let slot = Slot::new(0, 0);
    let final_ledger = create_final_ledger(config.ledger_config.clone(), sorted_ledger);
    let mut async_pool = create_async_pool(config.async_pool_config.clone(), BTreeMap::new());
    async_pool.apply_changes_unchecked(&messages);

    create_final_state(
        config.clone(),
        slot,
        Box::new(final_ledger),
        async_pool,
        VecDeque::new(),
        get_random_pos_state(r_limit, pos),
        get_random_executed_ops(r_limit, slot, config.executed_ops_config),
        get_random_executed_de(r_limit, slot, config.executed_denunciations_config),
    )
}

pub fn get_dummy_block_id(s: &str) -> BlockId {
    BlockId(Hash::compute_from(s.as_bytes()))
}

pub fn get_random_address() -> Address {
    let priv_key = KeyPair::generate();
    Address::from_public_key(&priv_key.get_public_key())
}

pub fn get_bootstrap_config(bootstrap_public_key: NodeId) -> BootstrapConfig {
    BootstrapConfig {
        listen_addr: Some("0.0.0.0:31244".parse().unwrap()),
        bootstrap_protocol: IpType::Both,
        bootstrap_timeout: 120000.into(),
        connect_timeout: 200.into(),
        retry_delay: 200.into(),
        max_ping: MassaTime::from_millis(500),
        read_timeout: 1000.into(),
        write_timeout: 1000.into(),
        read_error_timeout: 200.into(),
        write_error_timeout: 200.into(),
        max_listeners_per_peer: 100,
        bootstrap_list: vec![(
            SocketAddr::new(BASE_BOOTSTRAP_IP, 8069),
            bootstrap_public_key,
        )],
        keep_ledger: false,
        bootstrap_whitelist_path: PathBuf::from(
            "../massa-node/base_config/bootstrap_whitelist.json",
        ),
        bootstrap_blacklist_path: PathBuf::from(
            "../massa-node/base_config/bootstrap_blacklist.json",
        ),
        max_clock_delta: MassaTime::from_millis(1000),
        cache_duration: 10000.into(),
        max_simultaneous_bootstraps: 2,
        ip_list_max_size: 10,
        per_ip_min_interval: 10000.into(),
        max_bytes_read_write: std::f64::INFINITY,
        max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
        randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
        thread_count: THREAD_COUNT,
        periods_per_cycle: PERIODS_PER_CYCLE,
        endorsement_count: ENDORSEMENT_COUNT,
        max_advertise_length: MAX_ADVERTISE_LENGTH,
        max_bootstrap_blocks_length: MAX_BOOTSTRAP_BLOCKS,
        max_bootstrap_error_length: MAX_BOOTSTRAP_ERROR_LENGTH,
        max_bootstrap_final_state_parts_size: MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
        max_async_pool_changes: MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
        max_async_pool_length: MAX_ASYNC_POOL_LENGTH,
        max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        max_datastore_entry_count: MAX_DATASTORE_ENTRY_COUNT,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_ledger_changes_count: MAX_LEDGER_CHANGES_COUNT,
        max_parameters_size: MAX_PARAMETERS_SIZE,
        max_changes_slot_count: 1000,
        max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
        max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
        max_credits_length: MAX_DEFERRED_CREDITS_LENGTH,
        max_executed_ops_length: MAX_EXECUTED_OPS_LENGTH,
        max_ops_changes_length: MAX_EXECUTED_OPS_CHANGES_LENGTH,
        consensus_bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
        max_consensus_block_ids: MAX_CONSENSUS_BLOCKS_IDS,
        mip_store_stats_block_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        mip_store_stats_counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        max_denunciation_changes_length: MAX_DENUNCIATION_CHANGES_LENGTH,
    }
}

/// asserts that two `BootstrapableGraph` are equal
pub fn assert_eq_bootstrap_graph(v1: &BootstrapableGraph, v2: &BootstrapableGraph) {
    assert_eq!(
        v1.final_blocks.len(),
        v2.final_blocks.len(),
        "length mismatch"
    );
    let serializer = ExportActiveBlockSerializer::new();
    let mut data1: Vec<u8> = Vec::new();
    let mut data2: Vec<u8> = Vec::new();
    for (item1, item2) in v1.final_blocks.iter().zip(v2.final_blocks.iter()) {
        serializer.serialize(item1, &mut data1).unwrap();
        serializer.serialize(item2, &mut data2).unwrap();
    }
    assert_eq!(data1, data2, "BootstrapableGraph mismatch")
}

pub fn get_boot_state() -> BootstrapableGraph {
    let keypair = KeyPair::generate();

    let block = Block::new_verifiable(
        Block {
            header: BlockHeader::new_verifiable(
                BlockHeader {
                    // associated slot
                    // all header endorsements are supposed to point towards this one
                    slot: Slot::new(1, 0),
                    parents: vec![get_dummy_block_id("p1"); THREAD_COUNT as usize],
                    operation_merkle_root: Hash::compute_from("op_hash".as_bytes()),
                    endorsements: vec![
                        Endorsement::new_verifiable(
                            Endorsement {
                                slot: Slot::new(1, 0),
                                index: 1,
                                endorsed_block: get_dummy_block_id("p1"),
                            },
                            EndorsementSerializer::new(),
                            &keypair,
                        )
                        .unwrap(),
                        Endorsement::new_verifiable(
                            Endorsement {
                                slot: Slot::new(1, 0),
                                index: 3,
                                endorsed_block: get_dummy_block_id("p1"),
                            },
                            EndorsementSerializer::new(),
                            &keypair,
                        )
                        .unwrap(),
                    ],
                    denunciations: vec![],
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

    // TODO: We currently lost information. Need to use shared storage
    let block1 = ExportActiveBlock {
        block,
        parents: vec![(get_dummy_block_id("b1"), 4777); THREAD_COUNT as usize],
        is_final: true,
    };

    let boot_graph = BootstrapableGraph {
        final_blocks: vec![block1],
    };

    let bootstrapable_graph_serializer = BootstrapableGraphSerializer::new();
    let args = BlockDeserializerArgs {
        thread_count: THREAD_COUNT,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        endorsement_count: ENDORSEMENT_COUNT,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        last_start_period: Some(0),
    };
    let bootstrapable_graph_deserializer =
        BootstrapableGraphDeserializer::new(args, MAX_BOOTSTRAP_BLOCKS);

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

pub fn get_peers(keypair: &KeyPair) -> BootstrapPeers {
    let mut listeners1 = HashMap::default();
    listeners1.insert("82.245.123.77:8080".parse().unwrap(), TransportType::Tcp);

    let mut listeners2 = HashMap::default();
    listeners2.insert("82.220.123.78:8080".parse().unwrap(), TransportType::Tcp);
    BootstrapPeers(vec![
        (
            PeerId::from_bytes(keypair.get_public_key().to_bytes()).unwrap(),
            listeners1,
        ),
        (
            PeerId::from_bytes(keypair.get_public_key().to_bytes()).unwrap(),
            listeners2,
        ),
    ])
}

/// The bootstrap server function `get_state` has to be async due to main.rs `tokio::select!` usage to
/// cancel on `ctrl-c`
/// If get state can be cancelled with ctrl-c without an async context, this can be removed
pub fn make_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("network-provided-runtime")
        .build()
        .expect("failed to create runtime")
}
