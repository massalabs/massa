// Copyright (c) 2022 MASSA LABS <info@massa.net>
#![allow(clippy::ptr_arg)] // this allow &Vec<..> as function argument type

use crate::start_consensus_controller;
use massa_cipher::decrypt;
use massa_consensus_exports::error::ConsensusResult;
use massa_consensus_exports::{
    settings::ConsensusChannels, ConsensusCommandSender, ConsensusConfig, ConsensusEventReceiver,
};
use massa_execution_exports::test_exports::MockExecutionController;
use massa_graph::{export_active_block::ExportActiveBlock, BlockGraphExport, BootstrapableGraph};
use massa_hash::Hash;
use massa_models::prehash::PreHashMap;
use massa_models::{
    address::Address,
    amount::Amount,
    block::{
        Block, BlockHeader, BlockHeaderSerializer, BlockId, BlockSerializer, WrappedBlock,
        WrappedHeader,
    },
    operation::{Operation, OperationSerializer, OperationType, WrappedOperation},
    prehash::PreHashSet,
    slot::Slot,
    wrapped::{Id, WrappedContent},
};
use massa_pool_exports::test_exports::MockPoolController;
use massa_pool_exports::PoolController;
use massa_pos_exports::{SelectorConfig, SelectorController};
use massa_pos_worker::start_selector_worker;
use massa_protocol_exports::test_exports::MockProtocolController;
use massa_protocol_exports::ProtocolCommand;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;
use parking_lot::Mutex;
use std::{collections::HashSet, future::Future, path::Path};
use std::{str::FromStr, sync::Arc, time::Duration};

use tracing::info;

/* TODO https://github.com/massalabs/massa/issues/3099
/// Handle the expected selector messages, always approving the address.
pub fn approve_producer_and_selector_for_staker(
    staking_key: &KeyPair,
    selector_controller: &Receiver<MockSelectorControllerMessage>,
) {
    let addr = Address::from_public_key(&staking_key.get_public_key());
    // Drain all messages, assuming there can be a slight delay between sending some.
    loop {
        let timeout = Duration::from_millis(100);
        match selector_controller.recv_timeout(timeout) {
            Ok(MockSelectorControllerMessage::GetSelection {
                slot: _,
                response_tx,
            }) => {
                let selection = Selection {
                    producer: addr.clone(),
                    endorsements: vec![addr.clone(); ENDORSEMENT_COUNT as usize],
                };
                response_tx.send(Ok(selection)).unwrap();
            }
            Ok(MockSelectorControllerMessage::GetProducer {
                slot: _,
                response_tx,
            }) => {
                response_tx.send(Ok(addr.clone())).unwrap();
            }
            Ok(msg) => panic!("Unexpected selector message {:?}", msg),
            Err(RecvTimeoutError::Timeout) => break,
            _ => panic!("Unexpected error from selector receiver"),
        }
    }
}
*/

pub fn get_dummy_block_id(s: &str) -> BlockId {
    BlockId(Hash::compute_from(s.as_bytes()))
}

pub struct AddressTest {
    pub address: Address,
    pub keypair: KeyPair,
}

impl From<AddressTest> for (Address, KeyPair) {
    fn from(addr: AddressTest) -> Self {
        (addr.address, addr.keypair)
    }
}

/// Same as `random_address()` but force a specific thread
pub fn random_address_on_thread(thread: u8, thread_count: u8) -> AddressTest {
    loop {
        let keypair = KeyPair::generate();
        let address = Address::from_public_key(&keypair.get_public_key());
        if thread == address.get_thread(thread_count) {
            return AddressTest { address, keypair };
        }
    }
}

/// Generate a random address
pub fn _random_address() -> AddressTest {
    let keypair = KeyPair::generate();
    AddressTest {
        address: Address::from_public_key(&keypair.get_public_key()),
        keypair,
    }
}

/// return true if another block has been seen
pub async fn validate_notpropagate_block(
    protocol_controller: &mut MockProtocolController,
    not_propagated: BlockId,
    timeout_ms: u64,
) -> bool {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock {
                block_id,
                storage: _,
            } => Some(block_id),
            _ => None,
        })
        .await;
    match param {
        Some(block_id) => not_propagated != block_id,
        None => false,
    }
}

/// return true if another block has been seen
pub async fn validate_notpropagate_block_in_list(
    protocol_controller: &mut MockProtocolController,
    not_propagated: &Vec<BlockId>,
    timeout_ms: u64,
) -> bool {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock {
                block_id,
                storage: _,
            } => Some(block_id),
            _ => None,
        })
        .await;
    match param {
        Some(block_id) => !not_propagated.contains(&block_id),
        None => false,
    }
}

pub async fn validate_propagate_block_in_list(
    protocol_controller: &mut MockProtocolController,
    valid: &Vec<BlockId>,
    timeout_ms: u64,
) -> BlockId {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock {
                block_id,
                storage: _,
            } => Some(block_id),
            _ => None,
        })
        .await;
    match param {
        Some(block_id) => {
            assert!(
                valid.contains(&block_id),
                "not the valid hash propagated, it can be a genesis_timestamp problem"
            );
            block_id
        }
        None => panic!("Hash not propagated."),
    }
}

pub async fn validate_ask_for_block(
    protocol_controller: &mut MockProtocolController,
    valid: BlockId,
    timeout_ms: u64,
) -> BlockId {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::WishlistDelta { new, .. } => Some(new),
            _ => None,
        })
        .await;
    match param {
        Some(new) => {
            assert!(new.contains_key(&valid), "not the valid hash asked for");
            assert_eq!(new.len(), 1);
            valid
        }
        None => panic!("Block not asked for before timeout."),
    }
}

pub async fn validate_wishlist(
    protocol_controller: &mut MockProtocolController,
    new: PreHashSet<BlockId>,
    remove: PreHashSet<BlockId>,
    timeout_ms: u64,
) {
    let new: PreHashMap<BlockId, Option<WrappedHeader>> =
        new.into_iter().map(|id| (id, None)).collect();
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::WishlistDelta { new, remove } => Some((new, remove)),
            _ => None,
        })
        .await;
    match param {
        Some((got_new, got_remove)) => {
            for key in got_new.keys() {
                assert!(new.contains_key(key));
            }
            assert_eq!(remove, got_remove);
        }
        None => panic!("Wishlist delta not sent for before timeout."),
    }
}

pub async fn validate_does_not_ask_for_block(
    protocol_controller: &mut MockProtocolController,
    hash: &BlockId,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::WishlistDelta { new, .. } => Some(new),
            _ => None,
        })
        .await;
    if let Some(new) = param {
        if new.contains_key(hash) {
            panic!("unexpected ask for block {}", hash);
        }
    }
}

pub async fn validate_propagate_block(
    protocol_controller: &mut MockProtocolController,
    valid_hash: BlockId,
    timeout_ms: u64,
) {
    protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::IntegratedBlock {
                block_id,
                storage: _,
            } => {
                if block_id == valid_hash {
                    return Some(());
                }
                None
            }
            _ => None,
        })
        .await
        .expect("Block not propagated before timeout.")
}

pub async fn validate_notify_block_attack_attempt(
    protocol_controller: &mut MockProtocolController,
    valid_hash: BlockId,
    timeout_ms: u64,
) {
    let param = protocol_controller
        .wait_command(timeout_ms.into(), |cmd| match cmd {
            ProtocolCommand::AttackBlockDetected(hash) => Some(hash),
            _ => None,
        })
        .await;
    match param {
        Some(hash) => assert_eq!(valid_hash, hash, "Attack attempt notified for wrong hash."),
        None => panic!("Attack attempt not notified before timeout."),
    }
}

pub async fn validate_block_found(
    _protocol_controller: &mut MockProtocolController,
    _valid_hash: &BlockId,
    _timeout_ms: u64,
) {
}

pub async fn validate_block_not_found(
    _protocol_controller: &mut MockProtocolController,
    _valid_hash: &BlockId,
    _timeout_ms: u64,
) {
}

pub async fn create_and_test_block(
    protocol_controller: &mut MockProtocolController,
    cfg: &ConsensusConfig,
    slot: Slot,
    best_parents: Vec<BlockId>,
    valid: bool,
    trace: bool,
    creator: &KeyPair,
) -> BlockId {
    let block = create_block(cfg, slot, best_parents, creator);
    let block_id = block.id;
    let slot = block.content.header.content.slot;
    let mut storage = Storage::create_root();
    if trace {
        info!("create block:{}", block.id);
    }

    storage.store_block(block);
    protocol_controller
        .receive_block(block_id, slot, storage.clone())
        .await;
    if valid {
        // Assert that the block is propagated.
        validate_propagate_block(protocol_controller, block_id, 2000).await;
    } else {
        // Assert that the the block is not propagated.
        validate_notpropagate_block(protocol_controller, block_id, 500).await;
    }
    block_id
}

pub async fn propagate_block(
    protocol_controller: &mut MockProtocolController,
    block_id: BlockId,
    slot: Slot,
    storage: Storage,
    valid: bool,
    timeout_ms: u64,
) -> BlockId {
    let block_hash = block_id;
    protocol_controller
        .receive_block(block_id, slot, storage)
        .await;
    if valid {
        // see if the block is propagated.
        validate_propagate_block(protocol_controller, block_hash, timeout_ms).await;
    } else {
        // see if the block is propagated.
        validate_notpropagate_block(protocol_controller, block_hash, timeout_ms).await;
    }
    block_hash
}

pub fn _create_roll_transaction(
    keypair: &KeyPair,
    roll_count: u64,
    buy: bool,
    expire_period: u64,
    fee: u64,
) -> WrappedOperation {
    let op = if buy {
        OperationType::RollBuy { roll_count }
    } else {
        OperationType::RollSell { roll_count }
    };

    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        expire_period,
        op,
    };
    Operation::new_wrapped(content, OperationSerializer::new(), keypair).unwrap()
}

pub async fn _wait_pool_slot(
    _pool_controller: &mut MockPoolController,
    _t0: MassaTime,
    period: u64,
    thread: u8,
) -> Slot {
    // TODO: Replace ??
    // pool_controller
    //     .wait_command(t0.checked_mul(2).unwrap(), |cmd| match cmd {
    //         PoolCommand::UpdateCurrentSlot(s) => {
    //             if s >= Slot::new(period, thread) {
    //                 Some(s)
    //             } else {
    //                 None
    //             }
    //         }
    //         _ => None,
    //     })
    //     .await
    //     .expect("timeout while waiting for slot")
    Slot::new(period, thread)
}

pub fn _create_transaction(
    keypair: &KeyPair,
    recipient_address: Address,
    amount: u64,
    expire_period: u64,
    fee: u64,
) -> WrappedOperation {
    let op = OperationType::Transaction {
        recipient_address,
        amount: Amount::from_str(&amount.to_string()).unwrap(),
    };

    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        expire_period,
        op,
    };
    Operation::new_wrapped(content, OperationSerializer::new(), keypair).unwrap()
}

pub fn _create_roll_buy(
    keypair: &KeyPair,
    roll_count: u64,
    expire_period: u64,
    fee: u64,
) -> WrappedOperation {
    let op = OperationType::RollBuy { roll_count };
    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        expire_period,
        op,
    };
    Operation::new_wrapped(content, OperationSerializer::new(), keypair).unwrap()
}

/* TODO https://github.com/massalabs/massa/issues/3099
pub fn create_roll_sell(
    keypair: &KeyPair,
    roll_count: u64,
    expire_period: u64,
    fee: u64,
) -> WrappedOperation {
    let op = OperationType::RollSell { roll_count };
    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        expire_period,
        op,
    };
    Operation::new_wrapped(content, OperationSerializer::new(), keypair).unwrap()
}
*/

// returns hash and resulting discarded blocks
pub fn create_block(
    cfg: &ConsensusConfig,
    slot: Slot,
    best_parents: Vec<BlockId>,
    creator: &KeyPair,
) -> WrappedBlock {
    create_block_with_merkle_root(
        cfg,
        Hash::compute_from("default_val".as_bytes()),
        slot,
        best_parents,
        creator,
    )
}

// returns hash and resulting discarded blocks
pub fn create_block_with_merkle_root(
    _cfg: &ConsensusConfig,
    operation_merkle_root: Hash,
    slot: Slot,
    best_parents: Vec<BlockId>,
    creator: &KeyPair,
) -> WrappedBlock {
    let header = BlockHeader::new_wrapped(
        BlockHeader {
            slot,
            parents: best_parents,
            operation_merkle_root,
            endorsements: Vec::new(),
        },
        BlockHeaderSerializer::new(),
        creator,
    )
    .unwrap();

    Block::new_wrapped(
        Block {
            header,
            operations: Default::default(),
        },
        BlockSerializer::new(),
        creator,
    )
    .unwrap()
}

/* TODO https://github.com/massalabs/massa/issues/3099
/// Creates an endorsement for use in consensus tests.
pub fn create_endorsement(
    sender_keypair: &KeyPair,
    slot: Slot,
    endorsed_block: BlockId,
    index: u32,
) -> WrappedEndorsement {
    let content = Endorsement {
        slot,
        index,
        endorsed_block,
    };
    Endorsement::new_wrapped(content, EndorsementSerializer::new(), sender_keypair).unwrap()
}
*/

pub fn _get_export_active_test_block(
    parents: Vec<(BlockId, u64)>,
    operations: Vec<WrappedOperation>,
    slot: Slot,
    is_final: bool,
) -> ExportActiveBlock {
    let keypair = KeyPair::generate();
    let block = Block::new_wrapped(
        Block {
            header: BlockHeader::new_wrapped(
                BlockHeader {
                    operation_merkle_root: Hash::compute_from(
                        &operations
                            .iter()
                            .flat_map(|op| op.id.into_bytes())
                            .collect::<Vec<_>>()[..],
                    ),
                    parents: parents.iter().map(|(id, _)| *id).collect(),
                    slot,
                    endorsements: Vec::new(),
                },
                BlockHeaderSerializer::new(),
                &keypair,
            )
            .unwrap(),
            operations: operations.iter().cloned().map(|op| op.id).collect(),
        },
        BlockSerializer::new(),
        &keypair,
    )
    .unwrap();

    ExportActiveBlock {
        parents,
        block,
        operations,
        is_final,
    }
}

pub fn create_block_with_operations(
    _cfg: &ConsensusConfig,
    slot: Slot,
    best_parents: &Vec<BlockId>,
    creator: &KeyPair,
    operations: Vec<WrappedOperation>,
) -> WrappedBlock {
    let operation_merkle_root = Hash::compute_from(
        &operations.iter().fold(Vec::new(), |acc, v| {
            [acc, v.id.get_hash().to_bytes().to_vec()].concat()
        })[..],
    );

    let header = BlockHeader::new_wrapped(
        BlockHeader {
            slot,
            parents: best_parents.clone(),
            operation_merkle_root,
            endorsements: Vec::new(),
        },
        BlockHeaderSerializer::new(),
        creator,
    )
    .unwrap();

    Block::new_wrapped(
        Block {
            header,
            operations: operations.into_iter().map(|op| op.id).collect(),
        },
        BlockSerializer::new(),
        creator,
    )
    .unwrap()
}

/* TODO https://github.com/massalabs/massa/issues/3099
pub fn create_block_with_operations_and_endorsements(
    _cfg: &ConsensusConfig,
    slot: Slot,
    best_parents: &Vec<BlockId>,
    creator: &KeyPair,
    operations: Vec<WrappedOperation>,
    endorsements: Vec<WrappedEndorsement>,
) -> WrappedBlock {
    let operation_merkle_root = Hash::compute_from(
        &operations.iter().fold(Vec::new(), |acc, v| {
            [acc, v.id.get_hash().to_bytes().to_vec()].concat()
        })[..],
    );

    let header = BlockHeader::new_wrapped(
        BlockHeader {
            slot,
            parents: best_parents.clone(),
            operation_merkle_root,
            endorsements,
        },
        BlockHeaderSerializer::new(),
        creator,
    )
    .unwrap();

    Block::new_wrapped(
        Block {
            header,
            operations: operations.into_iter().map(|op| op.id).collect(),
        },
        BlockSerializer::new(),
        creator,
    )
    .unwrap()
}
*/

pub fn get_creator_for_draw(draw: &Address, nodes: &Vec<KeyPair>) -> KeyPair {
    for key in nodes.iter() {
        let address = Address::from_public_key(&key.get_public_key());
        if address == *draw {
            return key.clone();
        }
    }
    panic!("Matching key for draw not found.");
}

/// Load staking keys from file and derive public keys and addresses
pub async fn _load_initial_staking_keys(
    path: &Path,
    password: &str,
) -> ConsensusResult<PreHashMap<Address, KeyPair>> {
    if !std::path::Path::is_file(path) {
        return Ok(PreHashMap::default());
    }
    let (_version, data) = decrypt(password, &tokio::fs::read(path).await?)?;
    serde_json::from_slice::<Vec<KeyPair>>(&data)
        .unwrap()
        .into_iter()
        .map(|key| Ok((Address::from_public_key(&key.get_public_key()), key)))
        .collect()
}

/// Runs a consensus test, passing a mock pool controller to it.
pub async fn _consensus_pool_test<F, V>(
    cfg: ConsensusConfig,
    boot_graph: Option<BootstrapableGraph>,
    test: F,
) where
    F: FnOnce(
        Box<dyn PoolController>,
        MockProtocolController,
        ConsensusCommandSender,
        ConsensusEventReceiver,
    ) -> V,
    V: Future<
        Output = (
            Box<dyn PoolController>,
            MockProtocolController,
            ConsensusCommandSender,
            ConsensusEventReceiver,
        ),
    >,
{
    let mut storage: Storage = Storage::create_root();
    if let Some(ref graph) = boot_graph {
        for export_block in &graph.final_blocks {
            storage.store_block(export_block.block.clone());
        }
    }
    // mock protocol & pool
    let (protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, _pool_event_receiver) = MockPoolController::new_with_receiver();
    // for now, execution_rx is ignored: cique updates to Execution pile up and are discarded
    let (execution_controller, execution_rx) = MockExecutionController::new_with_receiver();
    let stop_sinks = Arc::new(Mutex::new(false));
    let stop_sinks_clone = stop_sinks.clone();
    let execution_sink = std::thread::spawn(move || {
        while !*stop_sinks_clone.lock() {
            let _ = execution_rx.recv_timeout(Duration::from_millis(500));
        }
    });
    let staking_key =
        KeyPair::from_str("S1UxdCJv5ckDK8z87E5Jq5fEfSVLi2cTHgtpfZy7iURs3KpPns8").unwrap();
    let genesis_address = Address::from_public_key(&staking_key.get_public_key());
    let selector_config = SelectorConfig {
        max_draw_cache: 12,
        channel_size: 256,
        thread_count: 2,
        endorsement_count: 8,
        periods_per_cycle: 2,
        genesis_address,
    };
    // launch consensus controller
    let (_selector_manager, selector_controller) = start_selector_worker(selector_config).unwrap();
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            ConsensusChannels {
                execution_controller,
                protocol_command_sender: protocol_command_sender.clone(),
                protocol_event_receiver,
                pool_command_sender: pool_controller.clone(),
                selector_controller,
            },
            boot_graph,
            storage.clone(),
            0,
        )
        .await
        .expect("could not start consensus controller");

    // Call test func.
    let (
        _pool_controller,
        mut protocol_controller,
        _consensus_command_sender,
        consensus_event_receiver,
    ) = test(
        pool_controller,
        protocol_controller,
        consensus_command_sender,
        consensus_event_receiver,
    )
    .await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();

    // stop sinks
    *stop_sinks.lock() = true;
    execution_sink.join().unwrap();
}

/* TODO https://github.com/massalabs/massa/issues/3099
/// Runs a consensus test, passing a mock pool controller to it.
pub async fn consensus_pool_test_with_storage<F, V>(
    cfg: ConsensusConfig,
    boot_graph: Option<BootstrapableGraph>,
    test: F,
) where
    F: FnOnce(
        Box<dyn PoolController>,
        MockProtocolController,
        ConsensusCommandSender,
        ConsensusEventReceiver,
        Storage,
        Receiver<MockSelectorControllerMessage>,
    ) -> V,
    V: Future<
        Output = (
            Box<dyn PoolController>,
            MockProtocolController,
            ConsensusCommandSender,
            ConsensusEventReceiver,
            Receiver<MockSelectorControllerMessage>,
        ),
    >,
{
    let mut storage: Storage = Storage::create_root();
    if let Some(ref graph) = boot_graph {
        for export_block in &graph.final_blocks {
            storage.store_block(export_block.block.clone());
        }
    }
    // mock protocol & pool
    let (protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, _pool_event_receiver) = MockPoolController::new_with_receiver();
    // for now, execution_rx is ignored: cique updates to Execution pile up and are discarded
    let (execution_controller, execution_rx) = MockExecutionController::new_with_receiver();
    let stop_sinks = Arc::new(Mutex::new(false));
    let stop_sinks_clone = stop_sinks.clone();
    let execution_sink = std::thread::spawn(move || {
        while !*stop_sinks_clone.lock() {
            let _ = execution_rx.recv_timeout(Duration::from_millis(500));
        }
    });
    let (selector_controller, selector_receiver) = MockSelectorController::new_with_receiver();
    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            ConsensusChannels {
                execution_controller,
                protocol_command_sender: protocol_command_sender.clone(),
                protocol_event_receiver,
                pool_command_sender: pool_controller.clone(),
                selector_controller: selector_controller,
            },
            boot_graph,
            storage.clone(),
            0,
        )
        .await
        .expect("could not start consensus controller");

    // Call test func.
    let (
        _pool_controller,
        mut protocol_controller,
        _consensus_command_sender,
        consensus_event_receiver,
        _selector_controller,
    ) = test(
        pool_controller,
        protocol_controller,
        consensus_command_sender,
        consensus_event_receiver,
        storage,
        selector_receiver,
    )
    .await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();

    // stop sinks
    *stop_sinks.lock() = true;
    execution_sink.join().unwrap();
}
*/

/// Runs a consensus test, without passing a mock pool controller to it.
pub async fn consensus_without_pool_test<F, V>(cfg: ConsensusConfig, test: F)
where
    F: FnOnce(
        MockProtocolController,
        ConsensusCommandSender,
        ConsensusEventReceiver,
        Box<dyn SelectorController>,
    ) -> V,
    V: Future<
        Output = (
            MockProtocolController,
            ConsensusCommandSender,
            ConsensusEventReceiver,
            Box<dyn SelectorController>,
        ),
    >,
{
    let storage: Storage = Storage::create_root();
    // mock protocol & pool
    let (protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, _pool_event_receiver) = MockPoolController::new_with_receiver();
    let staking_key =
        KeyPair::from_str("S1UxdCJv5ckDK8z87E5Jq5fEfSVLi2cTHgtpfZy7iURs3KpPns8").unwrap();
    let genesis_address = Address::from_public_key(&staking_key.get_public_key());
    let selector_config = SelectorConfig {
        max_draw_cache: 12,
        channel_size: 256,
        thread_count: 2,
        endorsement_count: 8,
        periods_per_cycle: 2,
        genesis_address,
    };
    let (mut selector_manager, selector_controller) =
        start_selector_worker(selector_config).unwrap();
    // for now, execution_rx is ignored: clique updates to Execution pile up and are discarded
    let (execution_controller, execution_rx) = MockExecutionController::new_with_receiver();
    let stop_sinks = Arc::new(Mutex::new(false));
    let stop_sinks_clone = stop_sinks.clone();
    let execution_sink = std::thread::spawn(move || {
        while !*stop_sinks_clone.lock() {
            let _ = execution_rx.recv_timeout(Duration::from_millis(500));
        }
    });
    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            ConsensusChannels {
                execution_controller,
                protocol_command_sender: protocol_command_sender.clone(),
                protocol_event_receiver,
                pool_command_sender: pool_controller,
                selector_controller: selector_controller.clone(),
            },
            None,
            storage.clone(),
            0,
        )
        .await
        .expect("could not start consensus controller");

    // Call test func.
    let (
        mut protocol_controller,
        _consensus_command_sender,
        consensus_event_receiver,
        _selector_controller,
    ) = test(
        protocol_controller,
        consensus_command_sender,
        consensus_event_receiver,
        selector_controller,
    )
    .await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    selector_manager.stop();
    // stop sinks
    *stop_sinks.lock() = true;
    execution_sink.join().unwrap();
}

/// Runs a consensus test, without passing a mock pool controller to it,
/// and passing a reference to storage.
pub async fn consensus_without_pool_with_storage_test<F, V>(cfg: ConsensusConfig, test: F)
where
    F: FnOnce(
        Storage,
        MockProtocolController,
        ConsensusCommandSender,
        ConsensusEventReceiver,
        Box<dyn SelectorController>,
    ) -> V,
    V: Future<
        Output = (
            MockProtocolController,
            ConsensusCommandSender,
            ConsensusEventReceiver,
            Box<dyn SelectorController>,
        ),
    >,
{
    let storage: Storage = Storage::create_root();
    // mock protocol & pool
    let (protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, _pool_event_receiver) = MockPoolController::new_with_receiver();
    // for now, execution_rx is ignored: clique updates to Execution pile up and are discarded
    let (execution_controller, execution_rx) = MockExecutionController::new_with_receiver();
    let stop_sinks = Arc::new(Mutex::new(false));
    let stop_sinks_clone = stop_sinks.clone();
    let execution_sink = std::thread::spawn(move || {
        while !*stop_sinks_clone.lock() {
            let _ = execution_rx.recv_timeout(Duration::from_millis(500));
        }
    });
    let staking_key =
        KeyPair::from_str("S1UxdCJv5ckDK8z87E5Jq5fEfSVLi2cTHgtpfZy7iURs3KpPns8").unwrap();
    let genesis_address = Address::from_public_key(&staking_key.get_public_key());
    let selector_config = SelectorConfig {
        max_draw_cache: 12,
        channel_size: 256,
        thread_count: 2,
        endorsement_count: 8,
        periods_per_cycle: 2,
        genesis_address,
    };
    let (mut selector_manager, selector_controller) =
        start_selector_worker(selector_config).unwrap();
    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            ConsensusChannels {
                execution_controller,
                protocol_command_sender: protocol_command_sender.clone(),
                protocol_event_receiver,
                pool_command_sender: pool_controller,
                selector_controller: selector_controller.clone(),
            },
            None,
            storage.clone(),
            0,
        )
        .await
        .expect("could not start consensus controller");

    // Call test func.
    let (
        mut protocol_controller,
        _consensus_command_sender,
        consensus_event_receiver,
        _selector_controller,
    ) = test(
        storage,
        protocol_controller,
        consensus_command_sender,
        consensus_event_receiver,
        selector_controller,
    )
    .await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    selector_manager.stop();
    // stop sinks
    *stop_sinks.lock() = true;
    execution_sink.join().unwrap();
}

pub fn get_cliques(graph: &BlockGraphExport, hash: BlockId) -> HashSet<usize> {
    let mut res = HashSet::new();
    for (i, clique) in graph.max_cliques.iter().enumerate() {
        if clique.block_ids.contains(&hash) {
            res.insert(i);
        }
    }
    res
}
