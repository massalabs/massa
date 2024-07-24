// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines testing tools related to the configuration

use crate::{ExecutionConfig, StorageCostsConstants};
use massa_models::config::*;
use massa_sc_runtime::GasCosts;
use massa_time::MassaTime;
use tempfile::TempDir;

impl Default for ExecutionConfig {
    /// default configuration used for testing
    fn default() -> Self {
        let storage_costs_constants = StorageCostsConstants {
            ledger_cost_per_byte: LEDGER_COST_PER_BYTE,
            ledger_entry_base_cost: LEDGER_ENTRY_BASE_COST,
            ledger_entry_datastore_base_cost: LEDGER_COST_PER_BYTE
                .checked_mul_u64(LEDGER_ENTRY_DATASTORE_BASE_SIZE as u64)
                .expect("Overflow when creating constant ledger_entry_datastore_base_size"),
        };

        // Create a tmp dir then only storing the path will drop the original tmp dir object
        // thus deleting the folders
        // So we need to create it manually (not really safe but ok for unit testing)
        let hd_cache_path = TempDir::new().unwrap().path().to_path_buf();
        std::fs::create_dir_all(hd_cache_path.clone()).unwrap();
        let block_dump_folder_path = TempDir::new().unwrap().path().to_path_buf();
        std::fs::create_dir_all(block_dump_folder_path.clone()).unwrap();

        Self {
            readonly_queue_length: 100,
            max_final_events: 1000,
            max_async_gas: MAX_ASYNC_GAS,
            async_msg_cst_gas_cost: ASYNC_MSG_CST_GAS_COST,
            thread_count: THREAD_COUNT,
            roll_price: ROLL_PRICE,
            cursor_delay: MassaTime::from_millis(0),
            block_reward: BLOCK_REWARD,
            endorsement_count: ENDORSEMENT_COUNT as u64,
            max_gas_per_block: MAX_GAS_PER_BLOCK,
            operation_validity_period: OPERATION_VALIDITY_PERIODS,
            periods_per_cycle: PERIODS_PER_CYCLE,
            // reset genesis timestamp because we are in test mode that can take a while to process
            genesis_timestamp: MassaTime::now(),
            t0: MassaTime::from_millis(64),
            stats_time_window_duration: MassaTime::from_millis(30000),
            max_miss_ratio: *POS_MISS_RATE_DEACTIVATION_THRESHOLD,
            max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_bytecode_size: MAX_BYTECODE_LENGTH,
            max_datastore_value_size: MAX_DATASTORE_VALUE_LENGTH,
            storage_costs_constants,
            max_read_only_gas: 1_000_000_000,
            gas_costs: GasCosts::new(
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../massa-node/base_config/gas_costs/abi_gas_costs.json"
                )
                .into(),
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../massa-node/base_config/gas_costs/wasm_gas_costs.json"
                )
                .into(),
            )
            .unwrap(),
            base_operation_gas_cost: BASE_OPERATION_GAS_COST,
            last_start_period: 0,
            hd_cache_path,
            lru_cache_size: 1000,
            hd_cache_size: 10_000,
            snip_amount: 10,
            roll_count_to_slash_on_denunciation: 1,
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            broadcast_enabled: true,
            broadcast_slot_execution_output_channel_capacity: 5000,
            max_event_size: 50_000,
            max_function_length: 1000,
            max_parameter_length: 1000,
            chain_id: *CHAINID,
            broadcast_traces_enabled: true,
            broadcast_slot_execution_traces_channel_capacity: 5000,
            max_execution_traces_slot_limit: 320,
            block_dump_folder_path,
            max_deferred_call_future_slots: DEFERRED_CALL_MAX_FUTURE_SLOTS,
        }
    }
}
