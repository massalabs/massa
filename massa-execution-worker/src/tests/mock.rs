use massa_db::{DBBatch, MassaDB, MassaDBConfig};
use massa_execution_exports::ExecutionError;
use massa_final_state::{FinalState, FinalStateConfig};
use massa_hash::Hash;
use massa_ledger_exports::{LedgerConfig, LedgerController, LedgerEntry, LedgerError};
use massa_ledger_worker::FinalLedger;
use massa_models::config::ENDORSEMENT_COUNT;
use massa_models::denunciation::Denunciation;
use massa_models::execution::TempFileVestingRange;
use massa_models::prehash::PreHashMap;
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

    let raw_keypairs = [
        "S18r2i8oJJyhF7Kprx98zwxAc3W4szf7RKuVMX6JydZz8zSxHeC", // thread 0
        "S1FpYC4ugG9ivZZbLVrTwWtF9diSRiAwwrVX5Gx1ANSRLfouUjq", // thread 1
        "S1LgXhWLEgAgCX3nm6y8PVPzpybmsYpi6yg6ZySwu5Z4ERnD7Bu", // thread 2
    ];

    for s in raw_keypairs {
        let keypair = KeyPair::from_str(s).unwrap();
        let addr = Address::from_public_key(&keypair.get_public_key());
        rolls.insert(addr, 100);
        ledger.insert(
            addr,
            LedgerEntry {
                balance: Amount::from_str("300_000").unwrap(),
                ..Default::default()
            },
        );
    }

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
    let keypair = KeyPair::generate(0).unwrap();
    (Address::from_public_key(&keypair.get_public_key()), keypair)
}

pub fn get_sample_state(
    last_start_period: u64,
) -> Result<(Arc<RwLock<FinalState>>, NamedTempFile, TempDir), LedgerError> {
    let (rolls_file, ledger) = get_initials();
    let (ledger_config, tempfile, tempdir) = LedgerConfig::sample(&ledger);
    let db_config = MassaDBConfig {
        path: tempdir.path().to_path_buf(),
        max_history_length: 10,
        max_new_elements: 100,
        thread_count: THREAD_COUNT,
    };
    let db = Arc::new(RwLock::new(MassaDB::new(db_config)));

    let mut ledger = FinalLedger::new(ledger_config.clone(), db.clone());
    ledger.load_initial_ledger().unwrap();
    let default_config = FinalStateConfig::default();
    let cfg = FinalStateConfig {
        ledger_config,
        async_pool_config: default_config.async_pool_config,
        pos_config: default_config.pos_config,
        executed_ops_config: default_config.executed_ops_config,
        executed_denunciations_config: default_config.executed_denunciations_config,
        final_history_length: 128,
        thread_count: THREAD_COUNT,
        initial_rolls_path: rolls_file.path().to_path_buf(),
        endorsement_count: ENDORSEMENT_COUNT,
        max_executed_denunciations_length: 1000,
        initial_seed_string: "".to_string(),
        periods_per_cycle: 10,
        max_denunciations_per_block_header: 0,
    };
    let (_, selector_controller) = start_selector_worker(SelectorConfig::default())
        .expect("could not start selector controller");
    let mut final_state = if last_start_period > 0 {
        FinalState::new_derived_from_snapshot(
            db.clone(),
            cfg,
            Box::new(ledger),
            selector_controller,
            last_start_period,
        )
        .unwrap()
    } else {
        FinalState::new(db.clone(), cfg, Box::new(ledger), selector_controller, true).unwrap()
    };
    let mut batch: BTreeMap<Vec<u8>, Option<Vec<u8>>> = DBBatch::new();
    final_state.pos_state.create_initial_cycle(&mut batch);
    final_state.db.write().write_batch(batch, None);
    final_state.compute_initial_draws().unwrap();
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
    denunciations: Vec<Denunciation>,
    slot: Slot,
) -> Result<SecureShareBlock, ExecutionError> {
    let operation_merkle_root = Hash::compute_from(
        &operations.iter().fold(Vec::new(), |acc, v| {
            [acc, v.serialized_data.clone()].concat()
        })[..],
    );

    let header = BlockHeader::new_verifiable(
        BlockHeader {
            current_version: 0,
            announced_version: 0,
            slot,
            parents: vec![],
            operation_merkle_root,
            endorsements: vec![],
            denunciations,
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
    let mut map: PreHashMap<Address, Vec<TempFileVestingRange>> = PreHashMap::default();

    if with_value {
        const PAST_TIMESTAMP: u64 = 1675356692000; // 02/02/2023 17h51
        const SEC_TIMESTAMP: u64 = 1677775892000; // 02/03/2023 17h51;
        const FUTURE_TIMESTAMP: u64 = 1731257385000; // 10/11/2024 17h49;

        let vec = vec![
            TempFileVestingRange {
                timestamp: MassaTime::from_millis(SEC_TIMESTAMP),
                min_balance: Some(Amount::from_str("100000").unwrap()),
                max_rolls: Some(50),
            },
            TempFileVestingRange {
                timestamp: MassaTime::from_millis(FUTURE_TIMESTAMP),
                min_balance: Some(Amount::from_str("80000").unwrap()),
                max_rolls: None,
            },
            TempFileVestingRange {
                timestamp: MassaTime::from_millis(PAST_TIMESTAMP),
                min_balance: Some(Amount::from_str("150000").unwrap()),
                max_rolls: Some(30),
            },
        ];

        let keypair_0 =
            KeyPair::from_str("S18r2i8oJJyhF7Kprx98zwxAc3W4szf7RKuVMX6JydZz8zSxHeC").unwrap();
        let addr_0 = Address::from_public_key(&keypair_0.get_public_key());

        map.insert(addr_0, vec);
    }

    // write file
    serde_json::to_writer_pretty::<&File, PreHashMap<Address, Vec<TempFileVestingRange>>>(
        file.as_file(),
        &map,
    )
    .expect("unable to write initial vesting file");
    file.as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");

    file
}
