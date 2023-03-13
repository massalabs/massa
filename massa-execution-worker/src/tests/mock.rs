use massa_execution_exports::ExecutionError;
use massa_final_state::{FinalState, FinalStateConfig};
use massa_hash::Hash;
use massa_ledger_exports::{LedgerConfig, LedgerController, LedgerEntry, LedgerError};
use massa_ledger_worker::FinalLedger;
use massa_models::prehash::PreHashMap;
use massa_models::vesting_range::VestingRange;
use massa_models::{
    address::Address,
    amount::Amount,
    block::{Block, BlockSerializer, SecureShareBlock},
    block_header::{BlockHeader, BlockHeaderSerializer},
    config::THREAD_COUNT,
    operation::SecureShareOperation,
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pos_exports::SelectorConfig;
use massa_pos_worker::start_selector_worker;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use parking_lot::RwLock;
use std::str::FromStr;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Seek,
    sync::Arc,
};
use tempfile::{NamedTempFile, TempDir};

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

    // thread 2 / 31
    let keypair_2 =
        KeyPair::from_str("S12APSAzMPsJjVGWzUJ61ZwwGFTNapA4YtArMKDyW4edLu6jHvCr").unwrap();
    let addr_2 = Address::from_public_key(&keypair_2.get_public_key());
    rolls.insert(addr_2, 100);
    ledger.insert(
        addr_2,
        LedgerEntry {
            balance: Amount::from_str("300_000").unwrap(),
            ..Default::default()
        },
    );

    // thread 3 / 31
    let keypair_3 =
        KeyPair::from_str("S12onbtxzgHcDSrVMp9bzP1cUjno8V5hZd4yYiqaMmC3nq4z7fSv").unwrap();
    let addr_3 = Address::from_public_key(&keypair_3.get_public_key());
    rolls.insert(addr_3, 100);
    ledger.insert(
        addr_3,
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
#[allow(dead_code)] // to avoid warnings on gas_calibration feature
pub fn get_random_address_full() -> (Address, KeyPair) {
    let keypair = KeyPair::generate();
    (Address::from_public_key(&keypair.get_public_key()), keypair)
}

pub fn get_sample_state() -> Result<(Arc<RwLock<FinalState>>, NamedTempFile, TempDir), LedgerError>
{
    let (rolls_file, ledger) = get_initials();
    let (ledger_config, tempfile, tempdir) = LedgerConfig::sample(&ledger);
    let mut ledger = FinalLedger::new(ledger_config.clone());
    ledger.load_initial_ledger().unwrap();
    let default_config = FinalStateConfig::default();
    let cfg = FinalStateConfig {
        ledger_config,
        async_pool_config: default_config.async_pool_config,
        pos_config: default_config.pos_config,
        executed_ops_config: default_config.executed_ops_config,
        final_history_length: 128,
        thread_count: THREAD_COUNT,
        initial_rolls_path: rolls_file.path().to_path_buf(),
        initial_seed_string: "".to_string(),
        periods_per_cycle: 10,
    };
    let (_, selector_controller) = start_selector_worker(SelectorConfig::default())
        .expect("could not start selector controller");
    let mut final_state = FinalState::new(cfg, Box::new(ledger), selector_controller).unwrap();
    final_state.compute_initial_draws().unwrap();
    final_state.pos_state.create_initial_cycle();
    Ok((Arc::new(RwLock::new(final_state)), tempfile, tempdir))
}

/// Create an almost empty block with a vector `operations` and a random
/// creator.
///
/// Return a result that should be unwrapped in the root `#[test]` routine.
#[allow(dead_code)] // to avoid warnings on gas_calibration feature
pub fn create_block(
    creator_keypair: KeyPair,
    operations: Vec<SecureShareOperation>,
    slot: Slot,
) -> Result<SecureShareBlock, ExecutionError> {
    let operation_merkle_root = Hash::compute_from(
        &operations.iter().fold(Vec::new(), |acc, v| {
            [acc, v.serialized_data.clone()].concat()
        })[..],
    );

    let header = BlockHeader::new_verifiable(
        BlockHeader {
            slot,
            parents: vec![],
            operation_merkle_root,
            endorsements: vec![],
        },
        BlockHeaderSerializer::new(),
        &creator_keypair,
    )?;

    Ok(Block::new_verifiable(
        Block {
            header,
            operations: operations.into_iter().map(|op| op.id).collect(),
        },
        BlockSerializer::new(),
        &creator_keypair,
    )?)
}

/// get the mocked file for initial vesting
#[allow(dead_code)]
pub fn get_initials_vesting(with_value: bool) -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    let mut map = PreHashMap::default();

    if with_value {
        const PAST_TIMESTAMP: u64 = 1675356692000; // 02/02/2023 17h51
        const SEC_TIMESTAMP: u64 = 1677775892000; // 02/03/2023 17h51;
        const FUTURE_TIMESTAMP: u64 = 1731257385000; // 10/11/2024 17h49;

        // this range has past timestamp so she doesn't appear in vesting HashMap
        let vesting1 = VestingRange {
            start_slot: Slot::min(),
            end_slot: Slot::min(),
            timestamp: MassaTime::from(PAST_TIMESTAMP),
            min_balance: Amount::from_str("150000").unwrap(),
            max_rolls: 170,
        };

        let vesting2 = VestingRange {
            start_slot: Slot::min(),
            end_slot: Slot::min(),
            timestamp: MassaTime::from_millis(SEC_TIMESTAMP),
            min_balance: Amount::from_str("100000").unwrap(),
            max_rolls: 150,
        };

        let vesting3 = VestingRange {
            start_slot: Slot::min(),
            end_slot: Slot::min(),
            timestamp: MassaTime::from_millis(FUTURE_TIMESTAMP),
            min_balance: Amount::from_str("80000").unwrap(),
            max_rolls: 80,
        };

        let vec1 = vec![vesting1, vesting2, vesting3];

        let keypair_0 =
            KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
        let addr_0 = Address::from_public_key(&keypair_0.get_public_key());

        map.insert(addr_0, vec1);
    }

    // write file
    serde_json::to_writer_pretty::<&File, PreHashMap<Address, Vec<VestingRange>>>(
        file.as_file(),
        &map,
    )
    .expect("unable to write initial vesting file");
    file.as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");

    file
}
