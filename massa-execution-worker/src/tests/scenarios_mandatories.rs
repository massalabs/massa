// Copyright (c) 2022 MASSA LABS <info@massa.net>
use crate::start_execution_worker;
use massa_async_pool::AsyncPoolConfig;
use massa_execution_exports::{ExecutionConfig, ExecutionError, ReadOnlyExecutionRequest};
use massa_final_state::{FinalState, FinalStateConfig};
use massa_hash::hash::Hash;
use massa_ledger::{LedgerConfig, LedgerError};
use massa_models::{
    constants::{AMOUNT_DECIMAL_FACTOR, FINAL_HISTORY_LENGTH, THREAD_COUNT},
    Block, BlockHeader, BlockId, Operation, OperationType, SerializeCompact, SignedHeader,
    SignedOperation,
};
use massa_models::{Address, Amount, Slot};
use massa_signature::{derive_public_key, generate_random_private_key, PrivateKey, PublicKey};
use massa_storage::Storage;
use parking_lot::RwLock;
use serial_test::serial;
use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tempfile::NamedTempFile;

/// Same as `get_random_address()` and return priv_key and pub_key associated
/// to the address.
pub fn get_random_address_full() -> (Address, PrivateKey, PublicKey) {
    let priv_key = generate_random_private_key();
    let pub_key = derive_public_key(&priv_key);
    (Address::from_public_key(&pub_key), priv_key, pub_key)
}

/// Get a randomized address
pub fn get_random_address() -> Address {
    get_random_address_full().0
}

fn get_sample_ledger() -> Result<(Arc<RwLock<FinalState>>, NamedTempFile), LedgerError> {
    let mut initial: BTreeMap<Address, Amount> = Default::default();
    initial.insert(get_random_address(), Amount::from_str("129").unwrap());
    initial.insert(get_random_address(), Amount::from_str("878").unwrap());
    let (ledger_config, tempfile) = LedgerConfig::sample(&initial);
    let async_pool_config = AsyncPoolConfig { max_length: 100 };
    let cfg = FinalStateConfig {
        ledger_config,
        async_pool_config,
        final_history_length: FINAL_HISTORY_LENGTH,
        thread_count: THREAD_COUNT,
    };
    Ok((
        Arc::new(RwLock::new(FinalState::new(cfg).unwrap())),
        tempfile,
    ))
}

#[test]
#[serial]
fn test_execution_basic() {
    let (sample_ledger, _keep) = get_sample_ledger().unwrap();
    let (_, _) = start_execution_worker(
        ExecutionConfig::default(),
        sample_ledger,
        Default::default(),
    );
}

#[test]
#[serial]
fn test_execution_shutdown() {
    let (sample_ledger, _keep) = get_sample_ledger().unwrap();
    let (mut manager, _) = start_execution_worker(
        ExecutionConfig::default(),
        sample_ledger,
        Default::default(),
    );
    manager.stop()
}

#[test]
#[serial]
fn test_sending_command() {
    let (sample_ledger, _keep) = get_sample_ledger().unwrap();
    let (mut manager, controller) = start_execution_worker(
        ExecutionConfig::default(),
        sample_ledger,
        Default::default(),
    );
    controller.update_blockclique_status(Default::default(), Default::default());
    manager.stop()
}

#[test]
#[serial]
fn test_sending_read_only_execution_command() {
    let (sample_ledger, _keep) = get_sample_ledger().unwrap();
    let (mut manager, controller) = start_execution_worker(
        ExecutionConfig::default(),
        sample_ledger,
        Default::default(),
    );
    controller
        .execute_readonly_request(ReadOnlyExecutionRequest {
            max_gas: 1_000_000,
            simulated_gas_price: Amount::from_raw(1_000_000 * AMOUNT_DECIMAL_FACTOR),
            bytecode: include_bytes!("./event_test.wasm").to_vec(),
            call_stack: vec![],
        })
        .unwrap();
    manager.stop()
}

//#[test]
//#[serial]
//fn test_execution_with_bootstrap() {
//    let bootstrap_state = crate::BootstrapExecutionState {
//        final_slot: Slot::new(12, 5),
//        final_ledger: get_sample_ledger(),
//    };
//    let (_config_file_keepalive, settings) = get_sample_settings();
//    let (command_sender, _event_receiver, manager) =
//        start_controller(settings, Some(bootstrap_state))
//            .await
//            .expect("Failed to start execution.");
//    command_sender
//        .update_blockclique(Default::default(), Default::default())
//        .await
//        .expect("Failed to send command");
//    manager.stop().await.expect("Failed to stop execution.");
//}

#[test]
#[serial]
fn generate_events() {
    massa_models::init_serialization_context(massa_models::SerializationContext::default());
    // Compile the `./wasm_tests` and generate a block with `event_test.wasm`
    // as data. Then we check if we get an event as expected.
    let exec_cfg = ExecutionConfig {
        t0: 10.into(),
        ..ExecutionConfig::default()
    };
    let storage: Storage = Default::default();
    let (sample_ledger, _keep) = get_sample_ledger().unwrap();
    let (mut manager, controller) =
        start_execution_worker(exec_cfg, sample_ledger, storage.clone());

    let (sender_address, sender_private_key, sender_public_key) = get_random_address_full();
    let event_test_data = include_bytes!("./event_test.wasm");
    let (block_id, block) = create_block(vec![create_execute_sc_operation(
        sender_private_key,
        sender_public_key,
        event_test_data,
    )
    .unwrap()])
    .unwrap();
    let slot = block.header.content.slot;

    storage.store_block(block_id, block, Default::default());

    let finalized_blocks: HashMap<Slot, BlockId> = Default::default();
    let mut blockclique: HashMap<Slot, BlockId> = Default::default();

    blockclique.insert(slot, block_id);

    controller.update_blockclique_status(finalized_blocks, blockclique);

    std::thread::sleep(Duration::from_millis(1000));
    manager.stop();
    let events = controller.get_filtered_sc_output_event(
        Some(slot),
        Some(slot),
        Some(sender_address),
        None,
        None,
    );
    assert!(!events.is_empty(), "At least one event was expected")
}

/// Create an operation for the given sender with `data` as bytecode.
/// Return a result that should be unwraped in the root `#[test]` routine.
fn create_execute_sc_operation(
    sender_private_key: PrivateKey,
    sender_public_key: PublicKey,
    data: &[u8],
) -> Result<SignedOperation, ExecutionError> {
    let op = OperationType::ExecuteSC {
        data: data.to_vec(),
        max_gas: u64::MAX,
        coins: Amount::from_raw(u64::MAX),
        gas_price: Amount::from_raw(AMOUNT_DECIMAL_FACTOR),
    };
    let (_, op) = SignedOperation::new_signed(
        Operation {
            sender_public_key,
            fee: Amount::zero(),
            expire_period: 10,
            op,
        },
        &sender_private_key,
    )?;
    Ok(op)
}

/// Create an almost empty block with a vector `operations` and a random
/// creator.
///
/// Return a result that should be unwraped in the root `#[test]` routine.
fn create_block(operations: Vec<SignedOperation>) -> Result<(BlockId, Block), ExecutionError> {
    let creator = generate_random_private_key();
    let public_key = derive_public_key(&creator);

    let operation_merkle_root = Hash::compute_from(
        &operations.iter().fold(Vec::new(), |acc, v| {
            [acc, v.to_bytes_compact().unwrap()].concat()
        })[..],
    );

    let (id, header) = SignedHeader::new_signed(
        BlockHeader {
            creator: public_key,
            slot: Slot {
                period: 1,
                thread: 0,
            },
            parents: vec![],
            operation_merkle_root,
            endorsements: vec![],
        },
        &creator,
    )?;
    Ok((id, Block { header, operations }))
}
