// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::settings::{BootstrapConfig, IpType};
use crate::{BootstrapClientMessage, BootstrapServerMessage};
use bitvec::vec::BitVec;
use massa_async_pool::AsyncPoolChanges;
use massa_async_pool::{test_exports::get_random_message, AsyncPool};
use massa_consensus_exports::{
    bootstrapable_graph::BootstrapableGraph, export_active_block::ExportActiveBlock,
};
use massa_db_exports::{DBBatch, ShareableMassaDBController, StreamBatch};
use massa_deferred_calls::DeferredCallRegistry;
use massa_executed_ops::{
    ExecutedDenunciations, ExecutedDenunciationsChanges, ExecutedDenunciationsConfig, ExecutedOps,
    ExecutedOpsConfig,
};
use massa_final_state::test_exports::create_final_state;
use massa_final_state::{FinalState, FinalStateConfig, FinalStateController};
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_ledger_exports::{LedgerEntry, SetUpdateOrDelete};
use massa_ledger_worker::test_exports::create_final_ledger;
use massa_models::bytecode::Bytecode;
use massa_models::config::{
    BOOTSTRAP_RANDOMNESS_SIZE_BYTES, CHAINID, CONSENSUS_BOOTSTRAP_PART_SIZE, ENDORSEMENT_COUNT,
    MAX_ADVERTISE_LENGTH, MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_ERROR_LENGTH,
    MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE,
    MAX_CONSENSUS_BLOCKS_IDS, MAX_DATASTORE_ENTRY_COUNT, MAX_DATASTORE_KEY_LENGTH,
    MAX_DATASTORE_VALUE_LENGTH, MAX_DEFERRED_CREDITS_LENGTH, MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
    MAX_DENUNCIATION_CHANGES_LENGTH, MAX_EXECUTED_OPS_CHANGES_LENGTH, MAX_EXECUTED_OPS_LENGTH,
    MAX_FUNCTION_NAME_LENGTH, MAX_LEDGER_CHANGES_COUNT, MAX_LISTENERS_PER_PEER,
    MAX_OPERATIONS_PER_BLOCK, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
    MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH, MAX_PARAMETERS_SIZE,
    MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH, MIP_STORE_STATS_BLOCK_CONSIDERED,
    PERIODS_PER_CYCLE, THREAD_COUNT,
};
use massa_models::denunciation::DenunciationIndex;
use massa_models::node::NodeId;
use massa_models::prehash::{CapacityAllocator, PreHashSet};
use massa_models::streaming_step::StreamingStep;
use massa_models::version::Version;
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
use massa_pos_exports::{DeferredCredits, PoSChanges, PoSFinalState, ProductionStats};
use massa_protocol_exports::{BootstrapPeers, PeerId, TransportType};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use num::rational::Ratio;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::unreachable;
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

/// generates random PoS cycles info
fn get_random_pos_cycles_info(
    r_limit: u64,
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
    rng_seed.push(rng.gen_range(0..2) == 1);
    (roll_counts, production_stats, rng_seed)
}

/// generates random PoS deferred credits
fn get_random_deferred_credits(r_limit: u64) -> DeferredCredits {
    let mut deferred_credits = DeferredCredits::new();

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
fn get_random_pos_state(r_limit: u64, mut pos: PoSFinalState) -> PoSFinalState {
    let (roll_counts, production_stats, _rng_seed) = get_random_pos_cycles_info(r_limit);
    let mut deferred_credits = DeferredCredits::new();
    deferred_credits.extend(get_random_deferred_credits(r_limit));

    // Do not add seed_bits to changes, as we create the initial cycle just after
    let changes = PoSChanges {
        seed_bits: Default::default(),
        roll_changes: roll_counts.into_iter().collect(),
        production_stats,
        deferred_credits,
    };

    let mut batch = DBBatch::new();

    pos.create_initial_cycle(&mut batch);

    pos.db.write().write_batch(batch, Default::default(), None);

    let mut batch = DBBatch::new();

    pos.apply_changes_to_batch(changes, Slot::new(0, 0), false, &mut batch)
        .expect("Critical: Error while applying changes to pos_state");

    pos.db.write().write_batch(batch, Default::default(), None);

    pos
}

pub fn get_random_executed_ops(
    _r_limit: u64,
    slot: Slot,
    config: ExecutedOpsConfig,
    db: ShareableMassaDBController,
) -> ExecutedOps {
    let mut executed_ops = ExecutedOps::new(config, db.clone());
    let mut batch = DBBatch::new();
    executed_ops.apply_changes_to_batch(get_random_executed_ops_changes(10), slot, &mut batch);
    db.write().write_batch(batch, Default::default(), None);
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
    db: ShareableMassaDBController,
) -> ExecutedDenunciations {
    let mut executed_de = ExecutedDenunciations::new(config, db);
    let mut batch = DBBatch::new();
    executed_de.apply_changes_to_batch(get_random_executed_de_changes(10), slot, &mut batch);

    executed_de
        .db
        .write()
        .write_batch(batch, Default::default(), None);

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
    db: ShareableMassaDBController,
) -> FinalState {
    let r_limit: u64 = 50;

    let mut sorted_ledger = HashMap::new();
    let mut messages = AsyncPoolChanges::default();
    for _ in 0..r_limit {
        let message = get_random_message(None, config.thread_count);
        messages
            .0
            .insert(message.compute_id(), SetUpdateOrDelete::Set(message));
    }
    for _ in 0..r_limit {
        sorted_ledger.insert(get_random_address(), get_random_ledger_entry());
    }
    let slot = Slot::new(0, 0);
    let final_ledger = create_final_ledger(db.clone(), config.ledger_config.clone(), sorted_ledger);

    let mut async_pool = AsyncPool::new(config.async_pool_config.clone(), db.clone());
    let mut batch = DBBatch::new();
    let versioning_batch = DBBatch::new();

    async_pool.apply_changes_to_batch(&messages, &mut batch);
    async_pool
        .db
        .write()
        .write_batch(batch, versioning_batch, None);

    let deferred_call_registry = DeferredCallRegistry::new(db.clone());

    let executed_ops = get_random_executed_ops(
        r_limit,
        slot,
        config.executed_ops_config.clone(),
        db.clone(),
    );

    let executed_denunciations = get_random_executed_de(
        r_limit,
        slot,
        config.executed_denunciations_config.clone(),
        db.clone(),
    );

    let pos_state = get_random_pos_state(r_limit, pos);

    let mip_store = MipStore::try_from((
        [],
        MipStatsConfig {
            block_count_considered: 10,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        },
    ))
    .unwrap();

    let mut final_state = create_final_state(
        config,
        Box::new(final_ledger),
        async_pool,
        deferred_call_registry,
        pos_state,
        executed_ops,
        executed_denunciations,
        mip_store,
        db,
    );
    let mut batch = DBBatch::new();
    final_state.init_execution_trail_hash_to_batch(&mut batch);
    final_state
        .db
        .write()
        .write_batch(batch, Default::default(), None);
    final_state
}

pub fn get_random_address() -> Address {
    let priv_key = KeyPair::generate(0).unwrap();
    Address::from_public_key(&priv_key.get_public_key())
}

pub fn get_bootstrap_config(bootstrap_public_key: NodeId) -> BootstrapConfig {
    BootstrapConfig {
        listen_addr: Some("0.0.0.0:31244".parse().unwrap()),
        bootstrap_protocol: IpType::Both,
        bootstrap_timeout: MassaTime::from_millis(120000),
        connect_timeout: MassaTime::from_millis(200),
        retry_delay: MassaTime::from_millis(200),
        max_ping: MassaTime::from_millis(500),
        read_timeout: MassaTime::from_millis(1000),
        write_timeout: MassaTime::from_millis(1000),
        read_error_timeout: MassaTime::from_millis(200),
        write_error_timeout: MassaTime::from_millis(200),
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
        cache_duration: MassaTime::from_millis(10000),
        max_simultaneous_bootstraps: 2,
        ip_list_max_size: 10,
        per_ip_min_interval: MassaTime::from_millis(10000),
        rate_limit: std::u64::MAX,
        max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
        randomness_size_bytes: BOOTSTRAP_RANDOMNESS_SIZE_BYTES,
        thread_count: THREAD_COUNT,
        periods_per_cycle: PERIODS_PER_CYCLE,
        endorsement_count: ENDORSEMENT_COUNT,
        max_advertise_length: MAX_ADVERTISE_LENGTH,
        max_bootstrap_blocks_length: MAX_BOOTSTRAP_BLOCKS,
        max_bootstrap_error_length: MAX_BOOTSTRAP_ERROR_LENGTH,
        max_final_state_elements_size: MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
        max_versioning_elements_size: MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE,
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
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        max_denunciation_changes_length: MAX_DENUNCIATION_CHANGES_LENGTH,
        chain_id: *CHAINID,
    }
}

fn gen_export_active_blocks<R: Rng>(rng: &mut R) -> ExportActiveBlock {
    let keypair = KeyPair::generate(0).unwrap();
    let block = gen_random_block(&keypair, rng)
        .new_verifiable(BlockSerializer::new(), &keypair, *CHAINID)
        .unwrap();
    let parents = (0..32)
        .map(|_| (gen_random_block_id(rng), rng.gen()))
        .collect();
    ExportActiveBlock {
        block,
        parents,
        is_final: rng.gen_bool(0.5),
    }
}

fn gen_random_stream_batch<T, R>(max: u32, change_id: T, rng: &mut R) -> StreamBatch<T>
where
    T: PartialOrd + Ord + PartialEq + Eq + Clone + std::fmt::Debug,
    R: Rng,
{
    let batch_size = rng.gen_range(0..max);
    let new_elements_size = rng.gen_range(0..batch_size);
    let mut new_elements = BTreeMap::new();
    let mut size = 0;
    loop {
        let key = gen_random_vector(MAX_DATASTORE_KEY_LENGTH as usize, rng);
        let val = gen_random_vector(MAX_DATASTORE_VALUE_LENGTH as usize, rng);
        size += key.len() as u64;
        size += val.len() as u64;
        if size > new_elements_size.into() {
            break;
        }
        new_elements.insert(key, val);
    }
    let mut updates_on_previous_elements = BTreeMap::new();
    loop {
        let key = gen_random_vector(MAX_DATASTORE_KEY_LENGTH as usize, rng);
        let mut s = key.len();
        let val = if rng.gen_bool(0.5) {
            let v = gen_random_vector(MAX_DATASTORE_VALUE_LENGTH as usize, rng);
            s += v.len();
            Some(v)
        } else {
            None
        };
        size += s as u64;
        if size > batch_size.into() {
            break;
        }
        updates_on_previous_elements.insert(key, val);
    }
    StreamBatch {
        new_elements,
        updates_on_previous_elements,
        change_id,
    }
}

fn gen_random_string<R: Rng>(max: usize, rng: &mut R) -> String {
    let res = gen_random_vector(max, rng);
    res.into_iter()
        .map(|c| char::from_u32(((c as u32) % (122 - 65)) + 65).unwrap())
        .collect()
}

fn gen_random_vector<R: Rng>(max: usize, rng: &mut R) -> Vec<u8> {
    let nb = rng.gen_range(0..=max);
    let mut res = vec![0; nb];
    rng.fill_bytes(&mut res);
    res
}

fn stream_batch_equal<T: PartialOrd + Ord + PartialEq + Eq + Clone + std::fmt::Debug>(
    s1: &StreamBatch<T>,
    s2: &StreamBatch<T>,
) -> bool {
    let mut new_elements_equal = true;
    for (key1, val1) in s1.new_elements.iter() {
        if let Some(val2) = s2.new_elements.get(key1) {
            new_elements_equal &= val1 == val2;
        } else {
            new_elements_equal = false;
            break;
        }
    }
    let mut update_prevels_equal = true;
    for (key1, val1) in s1.updates_on_previous_elements.iter() {
        if let Some(val2) = s2.updates_on_previous_elements.get(key1) {
            update_prevels_equal &= val1 == val2;
        } else {
            update_prevels_equal = false;
            break;
        }
    }
    new_elements_equal && update_prevels_equal && (s1.change_id == s2.change_id)
}

fn gen_random_hash<R: Rng>(rng: &mut R) -> Hash {
    let bytes: [u8; HASH_SIZE_BYTES] = rng.gen();
    Hash::from_bytes(&bytes)
}

fn gen_random_slot<R: Rng>(rng: &mut R) -> Slot {
    Slot {
        period: rng.gen(),
        thread: rng.gen_range(0..32),
    }
}

fn gen_random_block_id<R: Rng>(rng: &mut R) -> BlockId {
    BlockId::generate_from_hash(gen_random_hash(rng))
}

fn gen_random_block<R: Rng>(keypair: &KeyPair, rng: &mut R) -> Block {
    let slot = gen_random_slot(rng);
    let parents: Vec<BlockId> = (0..32).map(|_| gen_random_block_id(rng)).collect();
    let mut endorsements = vec![];
    for index in 0..rng.gen_range(1..ENDORSEMENT_COUNT) {
        let endorsement = Endorsement {
            index,
            slot,
            endorsed_block: parents[slot.thread as usize],
        };

        let endorsement = endorsement
            .new_verifiable(EndorsementSerializer::new(), keypair, *CHAINID)
            .unwrap();
        endorsements.push(endorsement);
    }

    let denunciations = vec![];
    // for index in 0..rng.gen_range(0..(MAX_DENUNCIATIONS_PER_BLOCK_HEADER as usize)) {
    //    // TODO    Denunciations generation
    // }

    let header = BlockHeader {
        current_version: rng.gen(),
        announced_version: rng.gen(),
        slot,
        parents,
        operation_merkle_root: gen_random_hash(rng),
        endorsements,
        denunciations,
    }
    .new_verifiable(BlockHeaderSerializer::new(), keypair, *CHAINID)
    .unwrap();
    let mut operations = vec![];
    for _ in 0..rng.gen_range(0..MAX_OPERATIONS_PER_BLOCK) {
        let op = OperationId::new(gen_random_hash(rng));
        operations.push(op);
    }
    Block { header, operations }
}

pub fn gen_random_streaming_step<T, R: Rng>(rng: &mut R, content: T) -> StreamingStep<T> {
    match rng.gen_range(0..3) {
        0 => StreamingStep::Started,
        1 => StreamingStep::Ongoing(content),
        2 => StreamingStep::Finished(if rng.gen_bool(0.8) {
            Some(content)
        } else {
            None
        }),
        _ => unreachable!(),
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(PartialEq, Debug)]
pub enum BootstrapServerMessageFaultyPart {
    MaxBootstrapErrorLengthOverflow = 0,
    MaxBootstrapPeersOverflow = 1,
    MaxListenersPerPeerOverflow = 2,
    SlotThreadOverflow = 3,
    MaxStateDatastoreKeyLengthOverflow = 4,
    MaxStateDatastoreValueLengthOverflow = 5,
    MaxStateNewElementsSizeOverflow = 6,
    StateChangeIdThreadOverflow = 7,
    MaxVersioningNewElementsSizeOverflow = 8,
    MaxVersioningDatastoreKeyLengthOverflow = 9,
    MaxVersioningDatastoreValueLengthOverflow = 10,
    VersioningChangeIdThreadOverflow = 11,
    MaxBootstrapBlocksOverflow = 12,
    MaxParentsPerBlockOverflow = 13,
    BlockSlotThreadOverflow = 14,
    MaxEndorsementsPerBlockOverflow = 15,
    MaxDenunciationsPerBlockOverflow = 16,
    MaxOperationsPerBlockOverflow = 17,
    MaxOutdatedIdsOverflow = 18,
    LastSlotBeforeDowntimeThreadOverflow = 19,
}

impl BootstrapServerMessageFaultyPart {
    pub fn from_u8(v_u8: u8) -> Self {
        match v_u8 {
            0 => Self::MaxBootstrapErrorLengthOverflow,
            1 => Self::MaxBootstrapPeersOverflow,
            2 => Self::MaxListenersPerPeerOverflow,
            3 => Self::SlotThreadOverflow,
            4 => Self::MaxStateDatastoreKeyLengthOverflow,
            5 => Self::MaxStateDatastoreValueLengthOverflow,
            6 => Self::MaxStateNewElementsSizeOverflow,
            7 => Self::StateChangeIdThreadOverflow,
            8 => Self::MaxVersioningNewElementsSizeOverflow,
            9 => Self::MaxVersioningDatastoreKeyLengthOverflow,
            10 => Self::MaxVersioningDatastoreValueLengthOverflow,
            11 => Self::VersioningChangeIdThreadOverflow,
            12 => Self::MaxBootstrapBlocksOverflow,
            13 => Self::MaxParentsPerBlockOverflow,
            14 => Self::BlockSlotThreadOverflow,
            15 => Self::MaxEndorsementsPerBlockOverflow,
            16 => Self::MaxDenunciationsPerBlockOverflow,
            17 => Self::MaxOperationsPerBlockOverflow,
            18 => Self::MaxOutdatedIdsOverflow,
            19 => Self::LastSlotBeforeDowntimeThreadOverflow,
            _ => panic!("invalid value"),
        }
    }
}

impl BootstrapServerMessage {
    pub fn generate<R: Rng>(rng: &mut R) -> Self {
        let variant = rng.gen_range(0..6);
        match variant {
            0 => {
                let t: u64 = rng.gen();
                // Taken from INSTANCE_LEN in version.rs
                let vi: String = (0..4)
                    .map(|_| char::from_u32(rng.gen_range(65..91)).unwrap())
                    .collect();
                let major: u32 = rng.gen();
                let minor: u32 = rng.gen();
                let version =
                    Version::from_str(format!("{}.{}.{}", vi, major, minor).as_str()).unwrap();
                let server_time = MassaTime::from_millis(t);
                BootstrapServerMessage::BootstrapTime {
                    server_time,
                    version,
                }
            }
            1 => {
                let peer_nb = rng.gen_range(0..100);
                let mut peers_list = vec![];
                for _ in 0..peer_nb {
                    peers_list.push((
                        PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key()),
                        HashMap::new(),
                    ));
                }
                let peers = BootstrapPeers(peers_list);
                BootstrapServerMessage::BootstrapPeers { peers }
            }
            2 => {
                let slot = gen_random_slot(rng);
                let state_part =
                    gen_random_stream_batch(MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, slot, rng);
                let slot = gen_random_slot(rng);
                let versioning_part =
                    gen_random_stream_batch(MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE, slot, rng);
                let mut final_blocks = vec![];
                let block_nb = rng.gen_range(5..100); //MAX_BOOTSTRAP_BLOCKS);
                for _ in 0..block_nb {
                    final_blocks.push(gen_export_active_blocks(rng));
                }
                let nb = rng.gen_range(0..MAX_BOOTSTRAP_BLOCKS);
                let mut consensus_outdated_ids = PreHashSet::default();
                for _ in 0..nb {
                    consensus_outdated_ids.insert(gen_random_block_id(rng));
                }
                let consensus_part = BootstrapableGraph { final_blocks };
                let last_start_period = rng.gen();
                let last_slot_before_downtime = if rng.gen_bool(0.5) {
                    Some(Some(gen_random_slot(rng)))
                } else {
                    None
                };
                let slot = gen_random_slot(rng);
                BootstrapServerMessage::BootstrapPart {
                    slot,
                    state_part,
                    versioning_part,
                    consensus_part,
                    consensus_outdated_ids,
                    last_start_period,
                    last_slot_before_downtime,
                }
            }
            3 => BootstrapServerMessage::BootstrapFinished,
            4 => BootstrapServerMessage::SlotTooOld,
            5 => BootstrapServerMessage::BootstrapError {
                error: gen_random_string(MAX_BOOTSTRAP_ERROR_LENGTH as usize, rng),
            },
            _ => unreachable!(),
        }
    }

    // Generate a message with errors added inside the message.
    // Has to be generated quickly, and have a previsible error case based on the
    // "faulty_part" input number.
    pub fn generate_faulty<R: Rng>(
        rng: &mut R,
        faulty_part: BootstrapServerMessageFaultyPart,
    ) -> Self {
        // Error
        if faulty_part == BootstrapServerMessageFaultyPart::MaxBootstrapErrorLengthOverflow {
            let mut res = vec![0; (MAX_BOOTSTRAP_ERROR_LENGTH as usize) + 10];
            rng.fill_bytes(&mut res);
            let error = res
                .into_iter()
                .map(|c| char::from_u32(((c as u32) % (122 - 65)) + 65).unwrap())
                .collect();
            BootstrapServerMessage::BootstrapError { error }

        // BootstrapPeers too many peers
        } else if faulty_part == BootstrapServerMessageFaultyPart::MaxBootstrapPeersOverflow {
            // Too many peers sent in the message
            let mut peers = vec![];
            for _ in 0..(MAX_ADVERTISE_LENGTH + 1) {
                let peer_id =
                    PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());
                let listeners = HashMap::new();
                peers.push((peer_id, listeners));
            }
            BootstrapServerMessage::BootstrapPeers {
                peers: BootstrapPeers(peers),
            }

        // BootstrapPeers too many listeners per peer
        } else if faulty_part == BootstrapServerMessageFaultyPart::MaxListenersPerPeerOverflow {
            // Too many listeners inside a peer
            let mut peers = vec![];
            let peer_id = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());
            let mut listeners = HashMap::new();
            assert!(
                MAX_LISTENERS_PER_PEER < (u16::MAX as u64),
                "Max listener per peer too high, adapt code to fit"
            );
            for n in 0..(MAX_LISTENERS_PER_PEER + 1) {
                let addr = format!("1.2.3.4:{n}").parse().unwrap();
                let ttype = TransportType::Tcp;
                listeners.insert(addr, ttype);
            }
            peers.push((peer_id, listeners));
            BootstrapServerMessage::BootstrapPeers {
                peers: BootstrapPeers(peers),
            }

        // BootstrapServerMessage::BootstrapPart
        } else {
            let mut slot = gen_random_slot(rng);
            if faulty_part == BootstrapServerMessageFaultyPart::SlotThreadOverflow {
                slot.thread = THREAD_COUNT;
            }
            let mut new_elements = BTreeMap::new();
            let mut new_elements_size: usize = 0;
            let key_len = if faulty_part
                == BootstrapServerMessageFaultyPart::MaxStateDatastoreKeyLengthOverflow
            {
                (MAX_DATASTORE_KEY_LENGTH as usize) + 1
            } else {
                MAX_DATASTORE_KEY_LENGTH as usize
            };
            let value_len = if faulty_part
                == BootstrapServerMessageFaultyPart::MaxStateDatastoreValueLengthOverflow
            {
                (MAX_DATASTORE_VALUE_LENGTH as usize) + 1
            } else {
                MAX_DATASTORE_VALUE_LENGTH as usize
            };
            let new_elements_size_max = if faulty_part
                == BootstrapServerMessageFaultyPart::MaxStateNewElementsSizeOverflow
            {
                // (MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE as usize) + key_len + value_len
                (MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE * 2) as usize
            } else {
                5
            };
            while new_elements_size
                .saturating_sub(key_len)
                .saturating_sub(value_len)
                < new_elements_size_max
            {
                let mut key = vec![0; key_len];
                rng.fill_bytes(&mut key);
                let mut value = vec![0; value_len];
                rng.fill_bytes(&mut value);
                new_elements.insert(key, value);
                new_elements_size += key_len + value_len;
            }
            if faulty_part == BootstrapServerMessageFaultyPart::MaxStateNewElementsSizeOverflow {
                assert!(
                    new_elements_size > (MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE as usize),
                    "Error in the code of the test for faulty_part 4"
                );
            }
            // No limit on the size of this except the u64 boundery
            let updates_on_previous_elements = BTreeMap::new();
            let mut change_id = gen_random_slot(rng);
            if faulty_part == BootstrapServerMessageFaultyPart::StateChangeIdThreadOverflow {
                change_id.thread = THREAD_COUNT;
            }
            let state_part = StreamBatch {
                new_elements,
                updates_on_previous_elements,
                change_id,
            };

            let mut new_elements = BTreeMap::new();
            let mut new_elements_size: usize = 0;
            let new_elements_size_max = if faulty_part
                == BootstrapServerMessageFaultyPart::MaxVersioningNewElementsSizeOverflow
            {
                (MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE + 1) as usize
            } else {
                5
            };
            let key_len = if faulty_part
                == BootstrapServerMessageFaultyPart::MaxVersioningDatastoreKeyLengthOverflow
            {
                (MAX_DATASTORE_KEY_LENGTH as usize) + 1
            } else {
                MAX_DATASTORE_KEY_LENGTH as usize
            };
            let value_len = if faulty_part
                == BootstrapServerMessageFaultyPart::MaxVersioningDatastoreValueLengthOverflow
            {
                (MAX_DATASTORE_VALUE_LENGTH as usize) + 1
            } else {
                MAX_DATASTORE_VALUE_LENGTH as usize
            };
            while new_elements_size
                .saturating_sub(key_len)
                .saturating_sub(value_len)
                < new_elements_size_max
            {
                let mut key = vec![0; key_len];
                rng.fill_bytes(&mut key);
                let mut value = vec![0; value_len];
                rng.fill_bytes(&mut value);
                new_elements.insert(key, value);
                new_elements_size += key_len + value_len;
            }
            // No limit on the size of this except the u64 boundery
            let updates_on_previous_elements = BTreeMap::new();
            let mut change_id = gen_random_slot(rng);
            if faulty_part == BootstrapServerMessageFaultyPart::VersioningChangeIdThreadOverflow {
                change_id.thread = THREAD_COUNT;
            }
            let versioning_part = StreamBatch {
                new_elements,
                updates_on_previous_elements,
                change_id,
            };

            // Consensus part
            // Generate all blocks, as the count is high, to reduce testing time,
            // Pre-generate everything that will go inside the block (it's not a problem)
            // That they are all the same as we care only about serialization / deserialization
            let block_nb =
                if faulty_part == BootstrapServerMessageFaultyPart::MaxBootstrapBlocksOverflow {
                    MAX_BOOTSTRAP_BLOCKS + 1
                } else {
                    5
                };

            let mut final_blocks = vec![];

            let mut block_slot = gen_random_slot(rng);
            let keypair = KeyPair::generate(0).unwrap();

            let mut parents_nb =
                if faulty_part == BootstrapServerMessageFaultyPart::MaxParentsPerBlockOverflow {
                    THREAD_COUNT + 1
                } else {
                    THREAD_COUNT
                };

            if faulty_part == BootstrapServerMessageFaultyPart::BlockSlotThreadOverflow {
                block_slot.thread = THREAD_COUNT;
                parents_nb = THREAD_COUNT + 1;
            }

            let parents: Vec<BlockId> = (0..parents_nb).map(|_| gen_random_block_id(rng)).collect();

            // Generate block header
            let mut endorsements = vec![];
            let nb_endorsements = if faulty_part
                == BootstrapServerMessageFaultyPart::MaxEndorsementsPerBlockOverflow
            {
                ENDORSEMENT_COUNT + 1
            } else {
                5
            };

            for index in 0..nb_endorsements {
                let endorsement = Endorsement {
                    index,
                    slot: block_slot,
                    endorsed_block: parents[block_slot.thread as usize],
                };

                let endorsement = endorsement
                    .new_verifiable(EndorsementSerializer::new(), &keypair, *CHAINID)
                    .unwrap();
                endorsements.push(endorsement);
            }

            let _nb_denunciations = if faulty_part
                == BootstrapServerMessageFaultyPart::MaxDenunciationsPerBlockOverflow
            {
                MAX_DENUNCIATIONS_PER_BLOCK_HEADER + 1
            } else {
                5
            } as usize;

            let denunciations = vec![];
            // for index in 0..rng.gen_range(0..nb_denunciations) {
            //    // TODO    Denunciations generation
            // }

            let nb_operations =
                if faulty_part == BootstrapServerMessageFaultyPart::MaxOperationsPerBlockOverflow {
                    MAX_OPERATIONS_PER_BLOCK + 1
                } else {
                    5
                };

            let mut operations = vec![];
            for _ in 0..nb_operations {
                let op = OperationId::new(gen_random_hash(rng));
                operations.push(op);
            }

            let header = BlockHeader {
                current_version: rng.gen(),
                announced_version: rng.gen(),
                slot: block_slot,
                parents: parents.clone(),
                operation_merkle_root: gen_random_hash(rng),
                endorsements: endorsements.clone(),
                denunciations,
            }
            .new_verifiable(BlockHeaderSerializer::new(), &keypair, *CHAINID)
            .unwrap();

            let block = Block {
                header,
                operations: operations.clone(),
            };
            let export_active_block = ExportActiveBlock {
                parents: parents
                    .iter()
                    .enumerate()
                    .map(|(n, id)| (*id, n as u64))
                    .collect(),
                is_final: false,
                block: block
                    .new_verifiable(BlockSerializer::new(), &keypair, *CHAINID)
                    .unwrap(),
            };
            for _ in 0..block_nb {
                final_blocks.push(export_active_block.clone());
            }
            let outdated_ids =
                if faulty_part == BootstrapServerMessageFaultyPart::MaxOutdatedIdsOverflow {
                    MAX_BOOTSTRAP_BLOCKS + 1
                } else {
                    5
                };
            let mut consensus_outdated_ids = PreHashSet::default();
            for _ in 0..outdated_ids {
                consensus_outdated_ids.insert(gen_random_block_id(rng));
            }
            let last_start_period = rng.gen();
            let last_slot_before_downtime = if faulty_part
                == BootstrapServerMessageFaultyPart::LastSlotBeforeDowntimeThreadOverflow
            {
                let mut slot = gen_random_slot(rng);
                slot.thread = THREAD_COUNT;
                Some(Some(slot))
            } else {
                None
            };
            BootstrapServerMessage::BootstrapPart {
                slot,
                state_part,
                versioning_part,
                consensus_part: BootstrapableGraph { final_blocks },
                consensus_outdated_ids,
                last_start_period,
                last_slot_before_downtime,
            }
        }
    }

    pub fn equals(&self, other: &BootstrapServerMessage) -> bool {
        match (self, other) {
            (
                BootstrapServerMessage::BootstrapTime {
                    server_time: t1,
                    version: v1,
                },
                BootstrapServerMessage::BootstrapTime {
                    server_time: t2,
                    version: v2,
                },
            ) => (t1 == t2) && (v1 == v2),
            (
                BootstrapServerMessage::BootstrapPeers { peers: p1 },
                BootstrapServerMessage::BootstrapPeers { peers: p2 },
            ) => p1 == p2,
            (
                BootstrapServerMessage::BootstrapPart {
                    slot: s1,
                    state_part: state1,
                    versioning_part: v1,
                    consensus_part: c1,
                    consensus_outdated_ids: co1,
                    last_start_period: lp1,
                    last_slot_before_downtime: ls1,
                },
                BootstrapServerMessage::BootstrapPart {
                    slot: s2,
                    state_part: state2,
                    versioning_part: v2,
                    consensus_part: c2,
                    consensus_outdated_ids: co2,
                    last_start_period: lp2,
                    last_slot_before_downtime: ls2,
                },
            ) => {
                let state_equal = stream_batch_equal(state1, state2);
                let versionning_equal = stream_batch_equal(v1, v2);
                let mut consensus_equal = true;
                if c1.final_blocks.len() != c2.final_blocks.len() {
                    return false;
                }
                for (n, active_block1) in c1.final_blocks.iter().enumerate() {
                    let active_block2 = c2.final_blocks.get(n).unwrap();
                    consensus_equal &= active_block1.parents == active_block2.parents;
                    consensus_equal &= active_block1.is_final == active_block2.is_final;
                    consensus_equal &=
                        active_block1.block.serialized_data == active_block2.block.serialized_data;
                }
                (s1 == s2)
                    && state_equal
                    && versionning_equal
                    && consensus_equal
                    && (co1 == co2)
                    && (lp1 == lp2)
                    && (ls1 == ls2)
            }
            (
                BootstrapServerMessage::BootstrapFinished,
                BootstrapServerMessage::BootstrapFinished,
            ) => true,
            (BootstrapServerMessage::SlotTooOld, BootstrapServerMessage::SlotTooOld) => true,
            (
                BootstrapServerMessage::BootstrapError { error: e1 },
                BootstrapServerMessage::BootstrapError { error: e2 },
            ) => e1 == e2,
            _ => false,
        }
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(PartialEq, Clone, Debug)]
pub enum BootstrapClientMessageFaultyPart {
    LastSlotThreadOverflow = 0,
    LastStateStepMaxKeyOverflow = 1,
    LastVersioningStepMaxKeyOverflow = 2,
    LastConsensusStepMaxBlockOverflow = 3,
}

impl BootstrapClientMessageFaultyPart {
    pub fn from_u8(v_u8: u8) -> Self {
        match v_u8 {
            0 => Self::LastSlotThreadOverflow,
            1 => Self::LastStateStepMaxKeyOverflow,
            2 => Self::LastVersioningStepMaxKeyOverflow,
            3 => Self::LastConsensusStepMaxBlockOverflow,
            _ => panic!("invalid value"),
        }
    }
}

impl BootstrapClientMessage {
    // Checks that two messages are equal or not
    pub fn equals(&self, other: &BootstrapClientMessage) -> bool {
        match (self, other) {
            (
                BootstrapClientMessage::AskBootstrapPeers,
                BootstrapClientMessage::AskBootstrapPeers,
            ) => true,
            (
                BootstrapClientMessage::AskBootstrapPart {
                    last_slot: ls1,
                    last_state_step: lstate1,
                    last_versioning_step: lv1,
                    last_consensus_step: lcs1,
                    send_last_start_period: slp1,
                },
                BootstrapClientMessage::AskBootstrapPart {
                    last_slot: ls2,
                    last_state_step: lstate2,
                    last_versioning_step: lv2,
                    last_consensus_step: lcs2,
                    send_last_start_period: slp2,
                },
            ) => {
                (ls1 == ls2)
                    && (lstate1 == lstate2)
                    && (lv1 == lv2)
                    && (lcs1 == lcs2)
                    && (slp1 == slp2)
            }
            (
                BootstrapClientMessage::BootstrapError { error: e1 },
                BootstrapClientMessage::BootstrapError { error: e2 },
            ) => e1 == e2,
            (
                BootstrapClientMessage::BootstrapSuccess,
                BootstrapClientMessage::BootstrapSuccess,
            ) => true,
            _ => false,
        }
    }

    // Generates a message filled with random data of random size based on the limit given in
    // constants. Used for parametric testing
    pub fn generate<R: Rng>(rng: &mut R) -> Self {
        let variant = rng.gen_range(0..4);
        match variant {
            0 => BootstrapClientMessage::AskBootstrapPeers,
            1 => {
                let last_slot = if rng.gen_bool(0.95) {
                    Some(gen_random_slot(rng))
                } else {
                    None
                };
                let last_state_step = if last_slot.is_some() {
                    let data = gen_random_vector(10, rng);
                    gen_random_streaming_step(rng, data)
                } else {
                    StreamingStep::Started
                };

                let last_versioning_step = if last_slot.is_some() {
                    let data = gen_random_vector(10, rng);
                    gen_random_streaming_step(rng, data)
                } else {
                    StreamingStep::Started
                };

                let last_consensus_step = if last_slot.is_some() {
                    let nb = rng.gen_range(0..CONSENSUS_BOOTSTRAP_PART_SIZE);
                    let mut data = PreHashSet::with_capacity(nb as usize);
                    for _ in 0..nb {
                        data.insert(gen_random_block_id(rng));
                    }
                    gen_random_streaming_step(rng, data)
                } else {
                    StreamingStep::Started
                };

                let send_last_start_period = if last_slot.is_none() {
                    true
                } else {
                    rng.gen_bool(0.5)
                };
                BootstrapClientMessage::AskBootstrapPart {
                    last_slot,
                    last_state_step,
                    last_versioning_step,
                    last_consensus_step,
                    send_last_start_period,
                }
            }
            2 => BootstrapClientMessage::BootstrapError {
                error: gen_random_string(MAX_BOOTSTRAP_ERROR_LENGTH as usize, rng),
            },
            3 => BootstrapClientMessage::BootstrapSuccess,
            _ => unreachable!(),
        }
    }

    // Generate a message with errors added inside the message.
    // Has to be generated quickly, and have a previsible error case based on the
    // "faulty_part" input number.
    pub fn generate_faulty<R: Rng>(
        rng: &mut R,
        faulty_part: BootstrapClientMessageFaultyPart,
    ) -> Self {
        let mut last_slot = gen_random_slot(rng);
        if faulty_part == BootstrapClientMessageFaultyPart::LastSlotThreadOverflow {
            last_slot.thread = THREAD_COUNT;
        }
        let last_slot = Some(last_slot);

        let last_state_step_nb =
            if faulty_part == BootstrapClientMessageFaultyPart::LastStateStepMaxKeyOverflow {
                (MAX_DATASTORE_KEY_LENGTH as usize) + 1
            } else {
                5
            };
        let mut last_state_step = vec![0; last_state_step_nb];
        rng.fill_bytes(&mut last_state_step);
        let last_state_step = StreamingStep::Ongoing(last_state_step);

        let last_versioning_step_nb =
            if faulty_part == BootstrapClientMessageFaultyPart::LastVersioningStepMaxKeyOverflow {
                (MAX_DATASTORE_KEY_LENGTH as usize) + 1
            } else {
                5
            };
        let mut last_versioning_step = vec![0; last_versioning_step_nb];
        rng.fill_bytes(&mut last_versioning_step);
        let last_versioning_step = StreamingStep::Ongoing(last_versioning_step);

        let nb =
            if faulty_part == BootstrapClientMessageFaultyPart::LastConsensusStepMaxBlockOverflow {
                (MAX_CONSENSUS_BLOCKS_IDS as usize) + 1
            } else {
                5
            };
        let mut last_consensus_step = PreHashSet::with_capacity(nb);
        for _ in 0..nb {
            last_consensus_step.insert(gen_random_block_id(rng));
        }
        let last_consensus_step = StreamingStep::Ongoing(last_consensus_step);

        BootstrapClientMessage::AskBootstrapPart {
            last_slot,
            last_state_step,
            last_versioning_step,
            last_consensus_step,
            send_last_start_period: false,
        }
    }
}

// Perform a parametric test, meaning that it checks conditions that should be met whatever the
// data is.
// The idea is to generate random cases, tests them, and let the random check for every edge
// case a humain brain can't think of.
// This function will call the `test_fct` as long as we are under the `duration` time,
// passing some fixed data `data` to it.
// IMPORTANT
//    If a bug is encounteded with a given seed, add it to `regressions` so this case gets
//    always tested afterwards
// Also adds a new feature "heavy_testing" in order to extend the length of the testing
pub fn parametric_test<F, T>(
    duration: std::time::Duration,
    data: T,
    regressions: Vec<u64>,
    test_fct: F,
) where
    F: Fn(&T, &mut SmallRng),
{
    #[cfg(feature = "heavy_testing")]
    let duration = match std::env::var("NEXTEST") {
        Ok(s) if s == String::from("1") => duration,
        _ => duration * 120,
    };
    for reg in regressions {
        println!("[*] Regression {reg}");
        let mut rng = SmallRng::seed_from_u64(reg);
        test_fct(&data, &mut rng);
    }

    let tstart = std::time::Instant::now();
    let mut seeder = SmallRng::from_entropy();
    let mut n = 0;
    while tstart.elapsed() < duration {
        let new_seed: u64 = seeder.gen();
        println!("[{n}] Seed {new_seed}");
        let mut rng = SmallRng::seed_from_u64(new_seed);
        test_fct(&data, &mut rng);
        n += 1;
    }
}
