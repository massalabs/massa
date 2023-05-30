//! Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{
    /*test_exports::{assert_eq_final_state, assert_eq_final_state_hash},*/
    FinalState, FinalStateConfig, StateChanges,
};
use massa_async_pool::{AsyncMessage, AsyncPoolChanges, AsyncPoolConfig};
use massa_db::{DBBatch, MassaDB, MassaDBConfig};
use massa_executed_ops::{ExecutedDenunciationsConfig, ExecutedOpsConfig};
use massa_ledger_exports::{
    LedgerChanges, LedgerConfig, LedgerEntryUpdate, SetOrKeep, SetUpdateOrDelete,
};
use massa_ledger_worker::FinalLedger;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::bytecode::Bytecode;
use massa_models::config::{
    DENUNCIATION_EXPIRE_PERIODS, ENDORSEMENT_COUNT, GENESIS_TIMESTAMP, MAX_DEFERRED_CREDITS_LENGTH,
    MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH, T0,
};
use massa_models::config::{
    MAX_ASYNC_MESSAGE_DATA, MAX_ASYNC_POOL_LENGTH, MAX_DATASTORE_KEY_LENGTH, POS_SAVED_CYCLES,
};
use massa_models::{config::MAX_DATASTORE_VALUE_LENGTH, slot::Slot};
use massa_pos_exports::{PoSConfig, SelectorConfig};
use massa_pos_worker::start_selector_worker;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::path::Path;
use std::{path::PathBuf, str::FromStr, sync::Arc};
use tempfile::TempDir;

fn create_final_state(temp_dir: &TempDir, reset_final_state: bool) -> Arc<RwLock<FinalState>> {
    let thread_count = 2;

    let db_config = MassaDBConfig {
        path: temp_dir.path().to_path_buf(),
        max_history_length: 10,
        max_new_elements: 100,
        thread_count,
    };
    let db = Arc::new(RwLock::new(MassaDB::new(db_config)));

    let rolls_path = PathBuf::from_str("../massa-node/base_config/initial_rolls.json").unwrap();

    let periods_per_cycle = 2;

    let final_state_local_config = FinalStateConfig {
        ledger_config: LedgerConfig {
            thread_count,
            initial_ledger_path: "".into(),
            disk_ledger_path: temp_dir.path().to_path_buf(),
            max_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        },
        async_pool_config: AsyncPoolConfig {
            thread_count,
            max_length: MAX_ASYNC_POOL_LENGTH,
            max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
            max_key_length: MAX_DATASTORE_KEY_LENGTH as u32,
        },
        pos_config: PoSConfig {
            periods_per_cycle,
            thread_count,
            cycle_history_length: POS_SAVED_CYCLES,
            max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
            max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
            max_credit_length: MAX_DEFERRED_CREDITS_LENGTH,
        },
        executed_ops_config: ExecutedOpsConfig { thread_count },
        executed_denunciations_config: ExecutedDenunciationsConfig {
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            thread_count,
            endorsement_count: ENDORSEMENT_COUNT,
        },
        final_history_length: 100,
        initial_seed_string: "".into(),
        initial_rolls_path: rolls_path,
        endorsement_count: ENDORSEMENT_COUNT,
        max_executed_denunciations_length: 1000,
        thread_count,
        periods_per_cycle,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        t0: T0,
        genesis_timestamp: *GENESIS_TIMESTAMP,
    };

    // setup selector local config
    let selector_local_config = SelectorConfig {
        thread_count,
        periods_per_cycle,
        ..Default::default()
    };

    // start proof-of-stake selectors
    let (mut _selector_manager, selector_controller) =
        start_selector_worker(selector_local_config.clone())
            .expect("could not start server selector controller");

    // MIP store
    let mip_store = MipStore::try_from((
        [],
        MipStatsConfig {
            block_count_considered: 10,
            counters_max: 10,
        },
    ))
    .unwrap();

    // setup final states

    let ledger = FinalLedger::new(final_state_local_config.ledger_config.clone(), db.clone());

    let final_state = Arc::new(RwLock::new(
        FinalState::new(
            db.clone(),
            final_state_local_config.clone(),
            Box::new(ledger),
            selector_controller,
            mip_store,
            reset_final_state,
        )
        .unwrap(),
    ));

    final_state
}

use massa_versioning::versioning::{MipStatsConfig, MipStore};
use std::{fs, io};

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[test]
fn test_final_state() {
    let temp_dir = TempDir::new().unwrap();
    let temp_dir2 = TempDir::new().unwrap();

    let hash;
    {
        let fs = create_final_state(&temp_dir, true);

        let mut batch = DBBatch::new();
        let versioning_batch = DBBatch::new();

        fs.write().pos_state.create_initial_cycle(&mut batch);

        let slot = fs.read().db.read().get_change_id().unwrap();

        fs.write()
            .db
            .write()
            .write_batch(batch, versioning_batch, Some(slot));

        let slot = Slot::new(1, 0);
        let mut state_changes = StateChanges::default();

        let message = AsyncMessage::new_with_hash(
            Slot::new(1, 0),
            0,
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            String::from("test"),
            10000000,
            Amount::from_str("1").unwrap(),
            Amount::from_str("1").unwrap(),
            Slot::new(2, 0),
            Slot::new(3, 0),
            vec![1, 2, 3, 4],
            None,
            None,
        );
        let mut async_pool_changes = AsyncPoolChanges::default();
        async_pool_changes.0.insert(
            message.compute_id(),
            SetUpdateOrDelete::Set(message.clone()),
        );
        state_changes.async_pool_changes = async_pool_changes;

        let amount = Amount::from_str("1").unwrap();
        let bytecode = Bytecode(vec![1, 2, 3]);
        let ledger_entry = LedgerEntryUpdate {
            balance: SetOrKeep::Set(amount),
            bytecode: SetOrKeep::Set(bytecode),
            datastore: BTreeMap::default(),
        };
        let mut ledger_changes = LedgerChanges::default();
        ledger_changes.0.insert(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            SetUpdateOrDelete::Update(ledger_entry),
        );
        state_changes.ledger_changes = ledger_changes;

        fs.write().finalize(slot, state_changes);

        hash = fs.read().db.read().get_db_hash();

        fs.write().db.write().db.flush().unwrap();
    }

    copy_dir_all(temp_dir.path(), &temp_dir2.path()).unwrap();

    let fs2 = create_final_state(&temp_dir2, false);
    let hash2 = fs2.read().db.read().get_db_hash();

    assert_eq!(hash, hash2);
}
