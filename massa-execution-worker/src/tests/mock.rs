use massa_async_pool::AsyncPoolConfig;
use massa_execution_exports::ExecutionError;
use massa_final_state::{FinalState, FinalStateConfig};
use massa_hash::Hash;
use massa_ledger_exports::LedgerEntry;
use massa_ledger_exports::{LedgerConfig, LedgerController, LedgerError};
use massa_ledger_worker::FinalLedger;
use massa_models::config::{
    ASYNC_POOL_BOOTSTRAP_PART_SIZE, MAX_ASYNC_MESSAGE_DATA, MAX_ASYNC_POOL_LENGTH,
};
use massa_models::{
    address::Address,
    amount::Amount,
    block::{Block, BlockHeader, BlockHeaderSerializer, BlockSerializer, WrappedBlock},
    config::THREAD_COUNT,
    operation::WrappedOperation,
    slot::Slot,
    wrapped::WrappedContent,
};
use massa_pos_exports::SelectorConfig;
use massa_pos_worker::start_selector_worker;
use massa_signature::KeyPair;
use parking_lot::RwLock;
use std::str::FromStr;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Seek,
    sync::Arc,
};
use tempfile::NamedTempFile;
use tempfile::TempDir;

fn get_initials() -> (NamedTempFile, HashMap<Address, LedgerEntry>) {
    let file = NamedTempFile::new().unwrap();
    let mut rolls: BTreeMap<Address, u64> = BTreeMap::new();
    let mut ledger: HashMap<Address, LedgerEntry> = HashMap::new();

    // thread 0 / 31
    let keypair_0 =
        KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    let addr_0 = Address::from_public_key(&keypair_0.get_public_key());
    rolls.insert(addr_0, 100);
    ledger.insert(
        addr_0,
        LedgerEntry {
            balance: Amount::from_str("300_000").unwrap(),
            ..Default::default()
        },
    );

    // thread 1 / 31
    let keypair_1 =
        KeyPair::from_str("S1kEBGgxHFBdsNC4HtRHhsZsB5irAtYHEmuAKATkfiomYmj58tm").unwrap();
    let addr_1 = Address::from_public_key(&keypair_1.get_public_key());
    rolls.insert(addr_1, 100);
    ledger.insert(
        addr_1,
        LedgerEntry {
            balance: Amount::from_str("300_000").unwrap(),
            ..Default::default()
        },
    );

    // write file
    serde_json::to_writer_pretty::<&File, BTreeMap<Address, u64>>(file.as_file(), &rolls)
        .expect("unable to write ledger file");
    file.as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");

    (file, ledger)
}

/// Same as `get_random_address()` and return `keypair` associated
/// to the address.
pub fn get_random_address_full() -> (Address, KeyPair) {
    let keypair = KeyPair::generate();
    (Address::from_public_key(&keypair.get_public_key()), keypair)
}

pub fn get_sample_state() -> Result<(Arc<RwLock<FinalState>>, NamedTempFile, TempDir), LedgerError>
{
    let (rolls_file, ledger) = get_initials();
    let (ledger_config, tempfile, tempdir) = LedgerConfig::sample(&ledger);
    let mut ledger = FinalLedger::new(ledger_config.clone()).expect("could not init final ledger");
    ledger.load_initial_ledger().unwrap();
    let async_pool_config = AsyncPoolConfig {
        max_length: MAX_ASYNC_POOL_LENGTH,
        bootstrap_part_size: ASYNC_POOL_BOOTSTRAP_PART_SIZE,
        max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
        thread_count: THREAD_COUNT,
    };
    let cfg = FinalStateConfig {
        ledger_config,
        async_pool_config,
        final_history_length: 128,
        thread_count: THREAD_COUNT,
        initial_rolls_path: rolls_file.path().to_path_buf(),
        initial_seed_string: "".to_string(),
        periods_per_cycle: 10,
    };
    let (_, selector_controller) = start_selector_worker(SelectorConfig::default())
        .expect("could not start selector controller");
    let mut final_state =
        FinalState::new(cfg, Box::new(ledger), selector_controller.clone()).unwrap();
    final_state.compute_initial_draws().unwrap();
    final_state.pos_state.create_initial_cycle();
    Ok((Arc::new(RwLock::new(final_state)), tempfile, tempdir))
}

/// Create an almost empty block with a vector `operations` and a random
/// creator.
///
/// Return a result that should be unwrapped in the root `#[test]` routine.
pub fn create_block(
    creator_keypair: KeyPair,
    operations: Vec<WrappedOperation>,
    slot: Slot,
) -> Result<WrappedBlock, ExecutionError> {
    let operation_merkle_root = Hash::compute_from(
        &operations.iter().fold(Vec::new(), |acc, v| {
            [acc, v.serialized_data.clone()].concat()
        })[..],
    );

    let header = BlockHeader::new_wrapped(
        BlockHeader {
            slot,
            parents: vec![],
            operation_merkle_root,
            endorsements: vec![],
        },
        BlockHeaderSerializer::new(),
        &creator_keypair,
    )?;

    Ok(Block::new_wrapped(
        Block {
            header,
            operations: operations.into_iter().map(|op| op.id).collect(),
        },
        BlockSerializer::new(),
        &creator_keypair,
    )?)
}
