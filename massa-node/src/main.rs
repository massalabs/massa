// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![doc = include_str!("../../README.md")]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
extern crate massa_logging;

#[cfg(feature = "op_spammer")]
use crate::operation_injector::start_operation_injector;
use crate::settings::SETTINGS;
use crate::survey::MassaSurvey;

use cfg_if::cfg_if;
use clap::{crate_version, Parser};
use crossbeam_channel::TryRecvError;
use dialoguer::Password;
use massa_api::{ApiServer, ApiV2, Private, Public, RpcServer, StopHandle, API};
use massa_api_exports::config::APIConfig;
use massa_async_pool::AsyncPoolConfig;
use massa_bootstrap::BootstrapError;
use massa_bootstrap::{
    get_state, start_bootstrap_server, BootstrapConfig, BootstrapManager, BootstrapTcpListener,
    DefaultConnector,
};
use massa_channel::receiver::MassaReceiver;
use massa_channel::MassaChannel;
use massa_consensus_exports::events::ConsensusEvent;
use massa_consensus_exports::{
    ConsensusBroadcasts, ConsensusChannels, ConsensusConfig, ConsensusManager,
};
use massa_consensus_worker::start_consensus_worker;
use massa_db_exports::{MassaDBConfig, MassaDBController};
use massa_db_worker::MassaDB;
use massa_executed_ops::{ExecutedDenunciationsConfig, ExecutedOpsConfig};
use massa_execution_exports::{
    ExecutionChannels, ExecutionConfig, ExecutionManager, GasCosts, StorageCostsConstants,
};
use massa_execution_worker::start_execution_worker;
#[cfg(all(
    feature = "dump-block",
    feature = "file_storage_backend",
    not(feature = "db_storage_backend")
))]
use massa_execution_worker::storage_backend::FileStorageBackend;
#[cfg(all(feature = "dump-block", feature = "db_storage_backend"))]
use massa_execution_worker::storage_backend::RocksDBStorageBackend;

use massa_factory_exports::{FactoryChannels, FactoryConfig, FactoryManager};
use massa_factory_worker::start_factory;
use massa_final_state::{FinalState, FinalStateConfig, FinalStateController};
use massa_grpc::config::{GrpcConfig, ServiceName};
use massa_grpc::server::{MassaPrivateGrpc, MassaPublicGrpc};
use massa_ledger_exports::LedgerConfig;
use massa_ledger_worker::FinalLedger;
use massa_logging::massa_trace;
use massa_metrics::{MassaMetrics, MetricsStopper};
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::config::constants::{
    ASYNC_MSG_CST_GAS_COST, BLOCK_REWARD, BOOTSTRAP_RANDOMNESS_SIZE_BYTES, CHANNEL_SIZE,
    CONSENSUS_BOOTSTRAP_PART_SIZE, DEFERRED_CALL_MAX_FUTURE_SLOTS, DELTA_F0,
    DENUNCIATION_EXPIRE_PERIODS, ENDORSEMENT_COUNT, END_TIMESTAMP, GENESIS_KEY, GENESIS_TIMESTAMP,
    INITIAL_DRAW_SEED, LEDGER_COST_PER_BYTE, LEDGER_ENTRY_BASE_COST,
    LEDGER_ENTRY_DATASTORE_BASE_SIZE, MAX_ADVERTISE_LENGTH, MAX_ASYNC_GAS, MAX_ASYNC_POOL_LENGTH,
    MAX_BLOCK_SIZE, MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_ERROR_LENGTH, MAX_BYTECODE_LENGTH,
    MAX_CONSENSUS_BLOCKS_IDS, MAX_DATASTORE_ENTRY_COUNT, MAX_DATASTORE_KEY_LENGTH,
    MAX_DATASTORE_VALUE_LENGTH, MAX_DEFERRED_CREDITS_LENGTH, MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
    MAX_DENUNCIATION_CHANGES_LENGTH, MAX_ENDORSEMENTS_PER_MESSAGE, MAX_EXECUTED_OPS_CHANGES_LENGTH,
    MAX_EXECUTED_OPS_LENGTH, MAX_FUNCTION_NAME_LENGTH, MAX_GAS_PER_BLOCK, MAX_LEDGER_CHANGES_COUNT,
    MAX_LISTENERS_PER_PEER, MAX_OPERATIONS_PER_BLOCK, MAX_OPERATIONS_PER_MESSAGE,
    MAX_OPERATION_DATASTORE_ENTRY_COUNT, MAX_OPERATION_DATASTORE_KEY_LENGTH,
    MAX_OPERATION_DATASTORE_VALUE_LENGTH, MAX_OPERATION_STORAGE_TIME, MAX_PARAMETERS_SIZE,
    MAX_PEERS_IN_ANNOUNCEMENT_LIST, MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH,
    MAX_SIZE_CHANNEL_COMMANDS_CONNECTIVITY, MAX_SIZE_CHANNEL_COMMANDS_PEERS,
    MAX_SIZE_CHANNEL_COMMANDS_PEER_TESTERS, MAX_SIZE_CHANNEL_COMMANDS_PROPAGATION_BLOCKS,
    MAX_SIZE_CHANNEL_COMMANDS_PROPAGATION_ENDORSEMENTS,
    MAX_SIZE_CHANNEL_COMMANDS_PROPAGATION_OPERATIONS, MAX_SIZE_CHANNEL_COMMANDS_RETRIEVAL_BLOCKS,
    MAX_SIZE_CHANNEL_COMMANDS_RETRIEVAL_ENDORSEMENTS,
    MAX_SIZE_CHANNEL_COMMANDS_RETRIEVAL_OPERATIONS, MAX_SIZE_CHANNEL_NETWORK_TO_BLOCK_HANDLER,
    MAX_SIZE_CHANNEL_NETWORK_TO_ENDORSEMENT_HANDLER, MAX_SIZE_CHANNEL_NETWORK_TO_OPERATION_HANDLER,
    MAX_SIZE_CHANNEL_NETWORK_TO_PEER_HANDLER, MIP_STORE_STATS_BLOCK_CONSIDERED,
    OPERATION_VALIDITY_PERIODS, PERIODS_PER_CYCLE, POS_MISS_RATE_DEACTIVATION_THRESHOLD,
    POS_SAVED_CYCLES, PROTOCOL_CONTROLLER_CHANNEL_SIZE, PROTOCOL_EVENT_CHANNEL_SIZE,
    ROLL_COUNT_TO_SLASH_ON_DENUNCIATION, ROLL_PRICE, SELECTOR_DRAW_CACHE_SIZE, T0, THREAD_COUNT,
    VERSION,
};
use massa_models::config::{
    BASE_OPERATION_GAS_COST, CHAINID, KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
    MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE,
    MAX_EVENT_DATA_SIZE, MAX_MESSAGE_SIZE, POOL_CONTROLLER_DENUNCIATIONS_CHANNEL_SIZE,
    POOL_CONTROLLER_ENDORSEMENTS_CHANNEL_SIZE, POOL_CONTROLLER_OPERATIONS_CHANNEL_SIZE,
};
use massa_models::slot::Slot;
use massa_models::timeslots::get_block_slot_timestamp;
use massa_pool_exports::{PoolBroadcasts, PoolChannels, PoolConfig, PoolManager};
use massa_pool_worker::start_pool_controller;
use massa_pos_exports::{PoSConfig, SelectorConfig, SelectorManager};
use massa_pos_worker::start_selector_worker;
use massa_protocol_exports::{ProtocolConfig, ProtocolManager, TransportType};
use massa_protocol_worker::{create_protocol_controller, start_protocol_controller};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;
use massa_versioning::keypair_factory::KeyPairFactory;
use massa_versioning::mips::get_mip_list;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use massa_wallet::Wallet;
use num::rational::Ratio;
use parking_lot::RwLock;
use settings::GrpcSettings;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;
use std::{path::Path, process, sync::Arc};

use survey::MassaSurveyStopper;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use tracing_subscriber::filter::{filter_fn, LevelFilter};

#[cfg(feature = "op_spammer")]
mod operation_injector;
mod settings;
mod survey;

async fn launch(
    args: &Args,
    node_wallet: Arc<RwLock<Wallet>>,
    sig_int_toggled: Arc<(Mutex<bool>, Condvar)>,
) -> (
    MassaReceiver<ConsensusEvent>,
    Option<BootstrapManager>,
    Box<dyn ConsensusManager>,
    Box<dyn ExecutionManager>,
    Box<dyn SelectorManager>,
    Box<dyn PoolManager>,
    Box<dyn ProtocolManager>,
    Box<dyn FactoryManager>,
    StopHandle,
    StopHandle,
    StopHandle,
    Option<massa_grpc::server::StopHandle>,
    Option<massa_grpc::server::StopHandle>,
    MetricsStopper,
    MassaSurveyStopper,
) {
    let now = MassaTime::now();

    if let Some(end) = *END_TIMESTAMP {
        if now > end {
            panic!("This episode has come to an end, please get the latest testnet node version to continue");
        }
    }

    // Storage shared by multiple components.
    let shared_storage: Storage = Storage::create_root();

    // init final state
    let ledger_config = LedgerConfig {
        thread_count: THREAD_COUNT,
        initial_ledger_path: SETTINGS.ledger.initial_ledger_path.clone(),
        max_key_length: MAX_DATASTORE_KEY_LENGTH,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
    };
    let async_pool_config = AsyncPoolConfig {
        max_length: MAX_ASYNC_POOL_LENGTH,
        thread_count: THREAD_COUNT,
        max_function_length: MAX_FUNCTION_NAME_LENGTH,
        max_function_params_length: MAX_PARAMETERS_SIZE as u64,
        max_key_length: MAX_DATASTORE_KEY_LENGTH as u32,
    };
    let pos_config = PoSConfig {
        periods_per_cycle: PERIODS_PER_CYCLE,
        thread_count: THREAD_COUNT,
        cycle_history_length: POS_SAVED_CYCLES,
        max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
        max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
        max_credit_length: MAX_DEFERRED_CREDITS_LENGTH,
        initial_deferred_credits_path: SETTINGS.ledger.initial_deferred_credits_path.clone(),
    };
    let executed_ops_config = ExecutedOpsConfig {
        thread_count: THREAD_COUNT,
        keep_executed_history_extra_periods: KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
    };
    let executed_denunciations_config = ExecutedDenunciationsConfig {
        denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
        thread_count: THREAD_COUNT,
        endorsement_count: ENDORSEMENT_COUNT,
        keep_executed_history_extra_periods: KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
    };
    let final_state_config = FinalStateConfig {
        ledger_config: ledger_config.clone(),
        async_pool_config,
        pos_config,
        executed_ops_config,
        executed_denunciations_config,
        final_history_length: SETTINGS.ledger.final_history_length,
        thread_count: THREAD_COUNT,
        periods_per_cycle: PERIODS_PER_CYCLE,
        initial_seed_string: INITIAL_DRAW_SEED.into(),
        initial_rolls_path: SETTINGS.selector.initial_rolls_path.clone(),
        endorsement_count: ENDORSEMENT_COUNT,
        max_executed_denunciations_length: MAX_DENUNCIATION_CHANGES_LENGTH,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        ledger_backup_periods_interval: SETTINGS.ledger.ledger_backup_periods_interval,
        t0: T0,
        genesis_timestamp: *GENESIS_TIMESTAMP,
    };

    // Start massa metrics
    let (massa_metrics, metrics_stopper) = MassaMetrics::new(
        SETTINGS.metrics.enabled,
        SETTINGS.metrics.bind,
        THREAD_COUNT,
        SETTINGS.metrics.tick_delay.to_duration(),
    );

    // Remove current disk ledger if there is one and we don't want to restart from snapshot
    // NOTE: this is temporary, since we cannot currently handle bootstrap from remaining ledger
    if args.keep_ledger || args.restart_from_snapshot_at_period.is_some() {
        info!("Loading old ledger for next episode");
    } else {
        if SETTINGS.ledger.disk_ledger_path.exists() {
            std::fs::remove_dir_all(SETTINGS.ledger.disk_ledger_path.clone())
                .expect("disk ledger delete failed");
        }
        if SETTINGS.execution.hd_cache_path.exists() {
            std::fs::remove_dir_all(SETTINGS.execution.hd_cache_path.clone())
                .expect("disk hd cache delete failed");
        }
    }

    let db_config = MassaDBConfig {
        path: SETTINGS.ledger.disk_ledger_path.clone(),
        max_history_length: SETTINGS.ledger.final_history_length,
        max_final_state_elements_size: MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE.try_into().unwrap(),
        max_versioning_elements_size: MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE.try_into().unwrap(),
        thread_count: THREAD_COUNT,
        max_ledger_backups: SETTINGS.ledger.max_ledger_backups,
    };
    let db = Arc::new(RwLock::new(
        Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
    ));

    // Create final ledger
    let ledger = FinalLedger::new(ledger_config.clone(), db.clone());

    // launch selector worker
    let (selector_manager, selector_controller) = start_selector_worker(SelectorConfig {
        max_draw_cache: SELECTOR_DRAW_CACHE_SIZE,
        channel_size: CHANNEL_SIZE,
        thread_count: THREAD_COUNT,
        endorsement_count: ENDORSEMENT_COUNT,
        periods_per_cycle: PERIODS_PER_CYCLE,
        genesis_address: Address::from_public_key(&GENESIS_KEY.get_public_key()),
    })
    .expect("could not start selector worker");

    // Creates an empty default store
    let mip_stats_config = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        warn_announced_version_ratio: Ratio::new(
            u64::from(SETTINGS.versioning.mip_stats_warn_announced_version),
            100,
        ),
    };
    // Ratio::new_raw(*SETTINGS.versioning.warn_announced_version_ratio, 100),

    // Create final state, either from a snapshot, or from scratch
    let final_state: Arc<RwLock<dyn FinalStateController>> = Arc::new(parking_lot::RwLock::new(
        match args.restart_from_snapshot_at_period {
            Some(last_start_period) => {
                // The node is restarted from a snapshot:
                // MIP store by reading from the db as it must have been updated by the massa ledger editor
                // (to shift transitions that might have happened during the network shutdown)
                // Note that FinalState::new_derived_from_snapshot will check if MIP store is consistent
                // No Bootstrap are expected after this
                let mip_store: MipStore = MipStore::try_from_db(db.clone(), mip_stats_config)
                    .expect("MIP store creation failed");
                debug!("After read from db, Mip store: {:?}", mip_store);

                FinalState::new_derived_from_snapshot(
                    db.clone(),
                    final_state_config,
                    Box::new(ledger),
                    selector_controller.clone(),
                    mip_store,
                    last_start_period,
                )
                .expect("could not init final state")
            }
            None => {
                // The node is started in a normal way
                // Read the mip list supported by the current software
                // The resulting MIP store will likely be updated by the bootstrap process in order
                // to get the latest information for the MIP store (new states, votes...)

                let mip_list = get_mip_list();
                debug!("MIP list: {:?}", mip_list);
                let mip_store = MipStore::try_from((mip_list, mip_stats_config))
                    .expect("mip store creation failed");

                FinalState::new(
                    db.clone(),
                    final_state_config,
                    Box::new(ledger),
                    selector_controller.clone(),
                    mip_store,
                    true,
                )
                .expect("could not init final state")
            }
        },
    ));

    let mip_store = final_state.read().get_mip_store().clone();

    let bootstrap_config: BootstrapConfig = BootstrapConfig {
        bootstrap_list: SETTINGS.bootstrap.bootstrap_list.clone(),
        bootstrap_protocol: SETTINGS.bootstrap.bootstrap_protocol,
        bootstrap_whitelist_path: SETTINGS.bootstrap.bootstrap_whitelist_path.clone(),
        bootstrap_blacklist_path: SETTINGS.bootstrap.bootstrap_blacklist_path.clone(),
        listen_addr: SETTINGS.bootstrap.bind,
        connect_timeout: SETTINGS.bootstrap.connect_timeout,
        bootstrap_timeout: SETTINGS.bootstrap.bootstrap_timeout,
        read_timeout: SETTINGS.bootstrap.read_timeout,
        write_timeout: SETTINGS.bootstrap.write_timeout,
        read_error_timeout: SETTINGS.bootstrap.read_error_timeout,
        write_error_timeout: SETTINGS.bootstrap.write_error_timeout,
        retry_delay: SETTINGS.bootstrap.retry_delay,
        max_ping: SETTINGS.bootstrap.max_ping,
        max_clock_delta: SETTINGS.bootstrap.max_clock_delta,
        cache_duration: SETTINGS.bootstrap.cache_duration,
        keep_ledger: args.keep_ledger,
        max_listeners_per_peer: MAX_LISTENERS_PER_PEER as u32,
        max_simultaneous_bootstraps: SETTINGS.bootstrap.max_simultaneous_bootstraps,
        per_ip_min_interval: SETTINGS.bootstrap.per_ip_min_interval,
        ip_list_max_size: SETTINGS.bootstrap.ip_list_max_size,
        rate_limit: SETTINGS.bootstrap.rate_limit,
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
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_ledger_changes_count: MAX_LEDGER_CHANGES_COUNT,
        max_parameters_size: MAX_PARAMETERS_SIZE,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_changes_slot_count: SETTINGS.ledger.final_history_length as u64,
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
    };

    let bootstrap_state = match get_state(
        &bootstrap_config,
        final_state.clone(),
        DefaultConnector,
        *VERSION,
        *GENESIS_TIMESTAMP,
        *END_TIMESTAMP,
        args.restart_from_snapshot_at_period,
        sig_int_toggled.clone(),
        massa_metrics.clone(),
    ) {
        Ok(vals) => vals,
        Err(BootstrapError::Interrupted(msg)) => {
            info!("{}", msg);
            process::exit(0);
        }
        Err(err) => panic!("critical error detected in the bootstrap process: {}", err),
    };

    if !final_state.read().is_db_valid() {
        // TODO: Bootstrap again instead of panicking
        panic!("critical: db is not valid after bootstrap");
    }

    if args.restart_from_snapshot_at_period.is_none() {
        final_state.write().recompute_caches();

        // give the controller to final state in order for it to feed the cycles
        final_state
            .write()
            .compute_initial_draws()
            .expect("could not compute initial draws"); // TODO: this might just mean a bad bootstrap, no need to panic, just reboot
    }

    let last_slot_before_downtime_ = *final_state.read().get_last_slot_before_downtime();
    if let Some(last_slot_before_downtime) = last_slot_before_downtime_ {
        let last_shutdown_start = last_slot_before_downtime
            .get_next_slot(THREAD_COUNT)
            .unwrap();
        let last_shutdown_end = Slot::new(final_state.read().get_last_start_period(), 0)
            .get_prev_slot(THREAD_COUNT)
            .unwrap();

        final_state
            .read()
            .get_mip_store()
            .is_consistent_with_shutdown_period(
                last_shutdown_start,
                last_shutdown_end,
                THREAD_COUNT,
                T0,
                *GENESIS_TIMESTAMP,
            )
            .expect("Mip store is not consistent with shutdown period");

        // If we are before a network restart, print the hash to make it easier to debug bootstrapping issues
        let now = MassaTime::now();
        let last_start_slot = Slot::new(
            final_state.read().get_last_start_period(),
            THREAD_COUNT.saturating_sub(1),
        );
        let last_start_slot_timestamp =
            get_block_slot_timestamp(THREAD_COUNT, T0, *GENESIS_TIMESTAMP, last_start_slot)
                .expect("Can't get timestamp for last_start_slot");
        if now < last_start_slot_timestamp {
            let final_state_hash = final_state.read().get_fingerprint();
            info!(
                "final_state hash before network restarts at slot {}: {}",
                last_start_slot, final_state_hash
            );
        }
    }

    // Storage costs constants
    let storage_costs_constants = StorageCostsConstants {
        ledger_cost_per_byte: LEDGER_COST_PER_BYTE,
        ledger_entry_base_cost: LEDGER_ENTRY_BASE_COST,
        ledger_entry_datastore_base_cost: LEDGER_COST_PER_BYTE
            .checked_mul_u64(LEDGER_ENTRY_DATASTORE_BASE_SIZE as u64)
            .expect("Overflow when creating constant ledger_entry_datastore_base_size"),
    };

    // gas costs
    let gas_costs = GasCosts::new(
        SETTINGS.execution.abi_gas_costs_file.clone(),
        SETTINGS.execution.wasm_gas_costs_file.clone(),
    )
    .expect("Failed to load gas costs");

    let block_dump_folder_path = SETTINGS.block_dump.block_dump_folder_path.clone();
    if !block_dump_folder_path.exists() {
        info!("Current folder: {:?}", std::env::current_dir().unwrap());
        info!("Creating dump folder: {:?}", block_dump_folder_path);
        std::fs::create_dir_all(block_dump_folder_path.clone())
            .expect("Cannot create dump block folder");
    }

    // launch execution module
    let execution_config = ExecutionConfig {
        max_final_events: SETTINGS.execution.max_final_events,
        readonly_queue_length: SETTINGS.execution.readonly_queue_length,
        cursor_delay: SETTINGS.execution.cursor_delay,
        max_async_gas: MAX_ASYNC_GAS,
        async_msg_cst_gas_cost: ASYNC_MSG_CST_GAS_COST,
        max_gas_per_block: MAX_GAS_PER_BLOCK,
        roll_price: ROLL_PRICE,
        thread_count: THREAD_COUNT,
        t0: T0,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        block_reward: BLOCK_REWARD,
        endorsement_count: ENDORSEMENT_COUNT as u64,
        operation_validity_period: OPERATION_VALIDITY_PERIODS,
        periods_per_cycle: PERIODS_PER_CYCLE,
        stats_time_window_duration: SETTINGS.execution.stats_time_window_duration,
        max_miss_ratio: *POS_MISS_RATE_DEACTIVATION_THRESHOLD,
        max_datastore_key_length: MAX_DATASTORE_KEY_LENGTH,
        max_bytecode_size: MAX_BYTECODE_LENGTH,
        max_datastore_value_size: MAX_DATASTORE_VALUE_LENGTH,
        storage_costs_constants,
        max_read_only_gas: SETTINGS.execution.max_read_only_gas,
        gas_costs: gas_costs.clone(),
        base_operation_gas_cost: BASE_OPERATION_GAS_COST,
        last_start_period: final_state.read().get_last_start_period(),
        hd_cache_path: SETTINGS.execution.hd_cache_path.clone(),
        lru_cache_size: SETTINGS.execution.lru_cache_size,
        hd_cache_size: SETTINGS.execution.hd_cache_size,
        snip_amount: SETTINGS.execution.snip_amount,
        roll_count_to_slash_on_denunciation: ROLL_COUNT_TO_SLASH_ON_DENUNCIATION,
        denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
        broadcast_enabled: SETTINGS.api.enable_broadcast,
        broadcast_slot_execution_output_channel_capacity: SETTINGS
            .execution
            .broadcast_slot_execution_output_channel_capacity,
        max_event_size: MAX_EVENT_DATA_SIZE,
        max_function_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameter_length: MAX_PARAMETERS_SIZE,
        chain_id: *CHAINID,
        #[cfg(feature = "execution-trace")]
        broadcast_traces_enabled: true,
        #[cfg(not(feature = "execution-trace"))]
        broadcast_traces_enabled: false,
        broadcast_slot_execution_traces_channel_capacity: SETTINGS
            .execution
            .broadcast_slot_execution_traces_channel_capacity,
        max_execution_traces_slot_limit: SETTINGS.execution.execution_traces_limit,
        block_dump_folder_path,
        max_deferred_call_future_slots: DEFERRED_CALL_MAX_FUTURE_SLOTS,
    };

    let execution_channels = ExecutionChannels {
        slot_execution_output_sender: broadcast::channel(
            execution_config.broadcast_slot_execution_output_channel_capacity,
        )
        .0,
        #[cfg(feature = "execution-trace")]
        slot_execution_traces_sender: broadcast::channel(
            execution_config.broadcast_slot_execution_traces_channel_capacity,
        )
        .0,
    };

    cfg_if! {
        if #[cfg(all(feature = "dump-block", feature = "db_storage_backend"))] {
            let block_storage_backend = Arc::new(RwLock::new(
                RocksDBStorageBackend::new(
                execution_config.block_dump_folder_path.clone(), SETTINGS.block_dump.max_blocks),
            ));
        } else if #[cfg(all(feature = "dump-block", feature = "file_storage_backend"))] {
            let block_storage_backend = Arc::new(RwLock::new(
                FileStorageBackend::new(
                execution_config.block_dump_folder_path.clone(), SETTINGS.block_dump.max_blocks),
            ));
        } else if #[cfg(feature = "dump-block")] {
            compile_error!("feature dump-block requise either db_storage_backend or file_storage_backend");
        }
    }

    let (execution_manager, execution_controller) = start_execution_worker(
        execution_config,
        final_state.clone(),
        selector_controller.clone(),
        mip_store.clone(),
        execution_channels.clone(),
        node_wallet.clone(),
        massa_metrics.clone(),
        #[cfg(feature = "dump-block")]
        block_storage_backend.clone(),
    );

    // launch pool controller
    let pool_config = PoolConfig {
        thread_count: THREAD_COUNT,
        max_block_size: MAX_BLOCK_SIZE,
        max_block_gas: MAX_GAS_PER_BLOCK,
        base_operation_gas_cost: BASE_OPERATION_GAS_COST,
        sp_compilation_cost: gas_costs.sp_compilation_cost,
        roll_price: ROLL_PRICE,
        max_block_endorsement_count: ENDORSEMENT_COUNT,
        operation_validity_periods: OPERATION_VALIDITY_PERIODS,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        max_operation_pool_size: SETTINGS.pool.max_operation_pool_size,
        max_operation_pool_excess_items: SETTINGS.pool.max_operation_pool_excess_items,
        operation_pool_refresh_interval: SETTINGS.pool.operation_pool_refresh_interval,
        operation_max_future_start_delay: SETTINGS.pool.operation_max_future_start_delay,
        max_endorsements_pool_size_per_thread: SETTINGS.pool.max_endorsements_pool_size_per_thread,
        operations_channel_size: POOL_CONTROLLER_OPERATIONS_CHANNEL_SIZE,
        endorsements_channel_size: POOL_CONTROLLER_ENDORSEMENTS_CHANNEL_SIZE,
        denunciations_channel_size: POOL_CONTROLLER_DENUNCIATIONS_CHANNEL_SIZE,
        broadcast_enabled: SETTINGS.api.enable_broadcast,
        broadcast_endorsements_channel_capacity: SETTINGS
            .pool
            .broadcast_endorsements_channel_capacity,
        broadcast_operations_channel_capacity: SETTINGS.pool.broadcast_operations_channel_capacity,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        periods_per_cycle: PERIODS_PER_CYCLE,
        denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        minimal_fees: SETTINGS.pool.minimal_fees,
        last_start_period: final_state.read().get_last_start_period(),
    };

    let pool_channels = PoolChannels {
        broadcasts: PoolBroadcasts {
            endorsement_sender: broadcast::channel(
                pool_config.broadcast_endorsements_channel_capacity,
            )
            .0,
            operation_sender: broadcast::channel(pool_config.broadcast_operations_channel_capacity)
                .0,
        },
        selector: selector_controller.clone(),
        execution_controller: execution_controller.clone(),
    };

    let (pool_manager, pool_controller) = start_pool_controller(
        pool_config,
        &shared_storage,
        pool_channels.clone(),
        node_wallet.clone(),
    );

    // launch protocol controller
    let mut listeners = HashMap::default();
    listeners.insert(SETTINGS.protocol.bind, TransportType::Tcp);
    let protocol_config = ProtocolConfig {
        thread_count: THREAD_COUNT,
        ask_block_timeout: SETTINGS.protocol.ask_block_timeout,
        max_known_blocks_size: SETTINGS.protocol.max_known_blocks_size,
        max_node_known_blocks_size: SETTINGS.protocol.max_node_known_blocks_size,
        max_block_propagation_time: SETTINGS.protocol.max_block_propagation_time,
        max_node_wanted_blocks_size: SETTINGS.protocol.max_node_wanted_blocks_size,
        max_known_ops_size: SETTINGS.protocol.max_known_ops_size,
        max_node_known_ops_size: SETTINGS.protocol.max_node_known_ops_size,
        max_known_endorsements_size: SETTINGS.protocol.max_known_endorsements_size,
        max_node_known_endorsements_size: SETTINGS.protocol.max_node_known_endorsements_size,
        max_simultaneous_ask_blocks_per_node: SETTINGS
            .protocol
            .max_simultaneous_ask_blocks_per_node,
        max_send_wait: SETTINGS.protocol.max_send_wait,
        operation_batch_buffer_capacity: SETTINGS.protocol.operation_batch_buffer_capacity,
        operation_announcement_buffer_capacity: SETTINGS
            .protocol
            .operation_announcement_buffer_capacity,
        operation_batch_proc_period: SETTINGS.protocol.operation_batch_proc_period,
        operation_announcement_interval: SETTINGS.protocol.operation_announcement_interval,
        max_operations_per_message: SETTINGS.protocol.max_operations_per_message,
        max_serialized_operations_size_per_block: MAX_BLOCK_SIZE as usize,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        controller_channel_size: PROTOCOL_CONTROLLER_CHANNEL_SIZE,
        event_channel_size: PROTOCOL_EVENT_CHANNEL_SIZE,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        endorsement_count: ENDORSEMENT_COUNT,
        max_message_size: MAX_MESSAGE_SIZE as usize,
        max_ops_kept_for_propagation: SETTINGS.protocol.max_ops_kept_for_propagation,
        max_operations_propagation_time: SETTINGS.protocol.max_operations_propagation_time,
        max_endorsements_propagation_time: SETTINGS.protocol.max_endorsements_propagation_time,
        last_start_period: final_state.read().get_last_start_period(),
        max_endorsements_per_message: MAX_ENDORSEMENTS_PER_MESSAGE as u64,
        max_denunciations_in_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        initial_peers: SETTINGS.protocol.initial_peers_file.clone(),
        listeners,
        keypair_file: SETTINGS.protocol.keypair_file.clone(),
        max_blocks_kept_for_propagation: SETTINGS.protocol.max_blocks_kept_for_propagation,
        block_propagation_tick: SETTINGS.protocol.block_propagation_tick,
        asked_operations_buffer_capacity: SETTINGS.protocol.asked_operations_buffer_capacity,
        thread_tester_count: SETTINGS.protocol.thread_tester_count,
        max_operation_storage_time: MAX_OPERATION_STORAGE_TIME,
        max_size_channel_commands_propagation_blocks: MAX_SIZE_CHANNEL_COMMANDS_PROPAGATION_BLOCKS,
        max_size_channel_commands_propagation_operations:
            MAX_SIZE_CHANNEL_COMMANDS_PROPAGATION_OPERATIONS,
        max_size_channel_commands_propagation_endorsements:
            MAX_SIZE_CHANNEL_COMMANDS_PROPAGATION_ENDORSEMENTS,
        max_size_channel_commands_retrieval_blocks: MAX_SIZE_CHANNEL_COMMANDS_RETRIEVAL_BLOCKS,
        max_size_channel_commands_retrieval_operations:
            MAX_SIZE_CHANNEL_COMMANDS_RETRIEVAL_OPERATIONS,
        max_size_channel_commands_retrieval_endorsements:
            MAX_SIZE_CHANNEL_COMMANDS_RETRIEVAL_ENDORSEMENTS,
        max_size_channel_commands_connectivity: MAX_SIZE_CHANNEL_COMMANDS_CONNECTIVITY,
        max_size_channel_commands_peers: MAX_SIZE_CHANNEL_COMMANDS_PEERS,
        max_size_channel_commands_peer_testers: MAX_SIZE_CHANNEL_COMMANDS_PEER_TESTERS,
        max_size_channel_network_to_block_handler: MAX_SIZE_CHANNEL_NETWORK_TO_BLOCK_HANDLER,
        max_size_channel_network_to_operation_handler:
            MAX_SIZE_CHANNEL_NETWORK_TO_OPERATION_HANDLER,
        max_size_channel_network_to_endorsement_handler:
            MAX_SIZE_CHANNEL_NETWORK_TO_ENDORSEMENT_HANDLER,
        max_size_channel_network_to_peer_handler: MAX_SIZE_CHANNEL_NETWORK_TO_PEER_HANDLER,
        max_size_value_datastore: MAX_DATASTORE_VALUE_LENGTH,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_size_function_name: MAX_FUNCTION_NAME_LENGTH,
        max_size_call_sc_parameter: MAX_PARAMETERS_SIZE,
        max_size_listeners_per_peer: MAX_LISTENERS_PER_PEER,
        max_size_peers_announcement: MAX_PEERS_IN_ANNOUNCEMENT_LIST,
        read_write_limit_bytes_per_second: SETTINGS.protocol.read_write_limit_bytes_per_second
            as u128,
        try_connection_timer: SETTINGS.protocol.try_connection_timer,
        unban_everyone_timer: SETTINGS.protocol.unban_everyone_timer,
        max_in_connections: SETTINGS.protocol.max_in_connections,
        timeout_connection: SETTINGS.protocol.timeout_connection,
        message_timeout: SETTINGS.protocol.message_timeout,
        tester_timeout: SETTINGS.protocol.tester_timeout,
        routable_ip: SETTINGS
            .protocol
            .routable_ip
            .or(SETTINGS.network.routable_ip),
        debug: false,
        peers_categories: SETTINGS.protocol.peers_categories.clone(),
        default_category_info: SETTINGS.protocol.default_category_info,
        version: *VERSION,
        try_connection_timer_same_peer: SETTINGS.protocol.try_connection_timer_same_peer,
        test_oldest_peer_cooldown: SETTINGS.protocol.test_oldest_peer_cooldown,
        rate_limit: SETTINGS.protocol.rate_limit,
        chain_id: *CHAINID,
    };

    let (protocol_controller, protocol_channels) =
        create_protocol_controller(protocol_config.clone());

    let consensus_config = ConsensusConfig {
        genesis_timestamp: *GENESIS_TIMESTAMP,
        end_timestamp: *END_TIMESTAMP,
        thread_count: THREAD_COUNT,
        t0: T0,
        genesis_key: GENESIS_KEY.clone(),
        max_discarded_blocks: SETTINGS.consensus.max_discarded_blocks,
        max_future_processing_blocks: SETTINGS.consensus.max_future_processing_blocks,
        max_dependency_blocks: SETTINGS.consensus.max_dependency_blocks,
        delta_f0: DELTA_F0,
        operation_validity_periods: OPERATION_VALIDITY_PERIODS,
        periods_per_cycle: PERIODS_PER_CYCLE,
        stats_timespan: SETTINGS.consensus.stats_timespan,
        force_keep_final_periods: SETTINGS.consensus.force_keep_final_periods,
        endorsement_count: ENDORSEMENT_COUNT,
        block_db_prune_interval: SETTINGS.consensus.block_db_prune_interval,
        max_gas_per_block: MAX_GAS_PER_BLOCK,
        channel_size: CHANNEL_SIZE,
        bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
        broadcast_enabled: SETTINGS.api.enable_broadcast,
        broadcast_blocks_headers_channel_capacity: SETTINGS
            .consensus
            .broadcast_blocks_headers_channel_capacity,
        broadcast_blocks_channel_capacity: SETTINGS.consensus.broadcast_blocks_channel_capacity,
        broadcast_filled_blocks_channel_capacity: SETTINGS
            .consensus
            .broadcast_filled_blocks_channel_capacity,
        last_start_period: final_state.read().get_last_start_period(),
        force_keep_final_periods_without_ops: SETTINGS
            .consensus
            .force_keep_final_periods_without_ops,
        chain_id: *CHAINID,
    };

    let (consensus_event_sender, consensus_event_receiver) =
        MassaChannel::new("consensus_event".to_string(), Some(CHANNEL_SIZE));
    let consensus_channels = ConsensusChannels {
        execution_controller: execution_controller.clone(),
        selector_controller: selector_controller.clone(),
        pool_controller: pool_controller.clone(),
        controller_event_tx: consensus_event_sender,
        protocol_controller: protocol_controller.clone(),
        broadcasts: ConsensusBroadcasts {
            block_header_sender: broadcast::channel(
                consensus_config.broadcast_blocks_headers_channel_capacity,
            )
            .0,
            block_sender: broadcast::channel(consensus_config.broadcast_blocks_channel_capacity).0,
            filled_block_sender: broadcast::channel(
                consensus_config.broadcast_filled_blocks_channel_capacity,
            )
            .0,
        },
    };

    let (consensus_controller, consensus_manager) = start_consensus_worker(
        consensus_config,
        consensus_channels.clone(),
        bootstrap_state.graph,
        shared_storage.clone(),
        massa_metrics.clone(),
    );

    let (protocol_manager, keypair, node_id) = start_protocol_controller(
        protocol_config.clone(),
        selector_controller.clone(),
        consensus_controller.clone(),
        bootstrap_state.peers,
        pool_controller.clone(),
        shared_storage.clone(),
        protocol_channels,
        mip_store.clone(),
        massa_metrics.clone(),
    )
    .expect("could not start protocol controller");

    // launch factory
    let factory_config = FactoryConfig {
        thread_count: THREAD_COUNT,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        initial_delay: SETTINGS.factory.initial_delay,
        max_block_size: MAX_BLOCK_SIZE as u64,
        max_block_gas: MAX_GAS_PER_BLOCK,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        last_start_period: final_state.read().get_last_start_period(),
        periods_per_cycle: PERIODS_PER_CYCLE,
        denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
        stop_production_when_zero_connections: SETTINGS
            .factory
            .stop_production_when_zero_connections,
        chain_id: *CHAINID,
    };
    let factory_channels = FactoryChannels {
        selector: selector_controller.clone(),
        consensus: consensus_controller.clone(),
        pool: pool_controller.clone(),
        protocol: protocol_controller.clone(),
        storage: shared_storage.clone(),
    };
    let factory_manager = start_factory(
        factory_config,
        node_wallet.clone(),
        factory_channels,
        mip_store.clone(),
    );

    let bootstrap_manager = bootstrap_config.listen_addr.map(|addr| {
        let (listener_stopper, listener) =
            BootstrapTcpListener::create(&addr).unwrap_or_else(|_| {
                panic!(
                    "{}",
                    format!("Could not bind to address: {}", addr).as_str()
                )
            });

        start_bootstrap_server(
            listener,
            listener_stopper,
            consensus_controller.clone(),
            protocol_controller.clone(),
            final_state.clone(),
            bootstrap_config,
            keypair.clone(),
            *VERSION,
            massa_metrics.clone(),
        )
        .expect("Could not start bootstrap server")
    });

    let api_config: APIConfig = APIConfig {
        bind_private: SETTINGS.api.bind_private,
        bind_public: SETTINGS.api.bind_public,
        bind_api: SETTINGS.api.bind_api,
        draw_lookahead_period_count: SETTINGS.api.draw_lookahead_period_count,
        max_arguments: SETTINGS.api.max_arguments,
        openrpc_spec_path: SETTINGS.api.openrpc_spec_path.clone(),
        bootstrap_whitelist_path: SETTINGS.bootstrap.bootstrap_whitelist_path.clone(),
        bootstrap_blacklist_path: SETTINGS.bootstrap.bootstrap_blacklist_path.clone(),
        max_request_body_size: SETTINGS.api.max_request_body_size,
        max_response_body_size: SETTINGS.api.max_response_body_size,
        max_connections: SETTINGS.api.max_connections,
        max_subscriptions_per_connection: SETTINGS.api.max_subscriptions_per_connection,
        max_log_length: SETTINGS.api.max_log_length,
        allow_hosts: SETTINGS.api.allow_hosts.clone(),
        batch_request_limit: SETTINGS.api.batch_request_limit,
        ping_interval: SETTINGS.api.ping_interval,
        enable_http: SETTINGS.api.enable_http,
        enable_ws: SETTINGS.api.enable_ws,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_gas_per_block: MAX_GAS_PER_BLOCK,
        base_operation_gas_cost: BASE_OPERATION_GAS_COST,
        sp_compilation_cost: gas_costs.sp_compilation_cost,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameter_size: MAX_PARAMETERS_SIZE,
        thread_count: THREAD_COUNT,
        keypair: keypair.clone(),
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        periods_per_cycle: PERIODS_PER_CYCLE,
        last_start_period: final_state.read().get_last_start_period(),
        chain_id: *CHAINID,
        deferred_credits_delta: SETTINGS.api.deferred_credits_delta,
        minimal_fees: SETTINGS.pool.minimal_fees,
    };

    // spawn Massa API
    let api = API::<ApiV2>::new(
        consensus_controller.clone(),
        consensus_channels.broadcasts.clone(),
        execution_controller.clone(),
        pool_channels.broadcasts.clone(),
        api_config.clone(),
        *VERSION,
    );
    let api_handle = api
        .serve(&SETTINGS.api.bind_api, &api_config)
        .await
        .expect("failed to start MASSA API");

    info!(
        "API | EXPERIMENTAL JsonRPC | listening on: {}",
        &SETTINGS.api.bind_api
    );

    // Disable WebSockets for Private and Public API's
    let mut api_config = api_config.clone();
    api_config.enable_ws = false;

    // Whether to spawn gRPC PUBLIC API
    let grpc_public_handle = if SETTINGS.grpc.public.enabled {
        let grpc_public_config = configure_grpc(
            ServiceName::Public,
            &SETTINGS.grpc.public,
            keypair.clone(),
            &final_state,
            SETTINGS.pool.minimal_fees,
        );

        let grpc_public_api = MassaPublicGrpc {
            consensus_controller: consensus_controller.clone(),
            consensus_broadcasts: consensus_channels.broadcasts.clone(),
            execution_controller: execution_controller.clone(),
            execution_channels,
            pool_broadcasts: pool_channels.broadcasts.clone(),
            pool_controller: pool_controller.clone(),
            protocol_controller: protocol_controller.clone(),
            selector_controller: selector_controller.clone(),
            storage: shared_storage.clone(),
            grpc_config: grpc_public_config.clone(),
            protocol_config: protocol_config.clone(),
            node_id,
            version: *VERSION,
            keypair_factory: KeyPairFactory {
                mip_store: mip_store.clone(),
            },
        };

        // Spawn gRPC PUBLIC API
        let grpc_public_stop_handle = grpc_public_api
            .serve(&grpc_public_config)
            .await
            .expect("failed to start gRPC PUBLIC API");
        info!("gRPC | PUBLIC | listening on: {}", grpc_public_config.bind);

        Some(grpc_public_stop_handle)
    } else {
        None
    };

    // Whether to spawn gRPC PRIVATE API
    let grpc_private_handle = if SETTINGS.grpc.private.enabled {
        let grpc_private_config = configure_grpc(
            ServiceName::Private,
            &SETTINGS.grpc.private,
            keypair.clone(),
            &final_state,
            SETTINGS.pool.minimal_fees,
        );

        let bs_white_black_list = bootstrap_manager
            .as_ref()
            .map(|manager| manager.white_black_list.clone());

        let grpc_private_api = MassaPrivateGrpc {
            consensus_controller: consensus_controller.clone(),
            execution_controller: execution_controller.clone(),
            pool_controller: pool_controller.clone(),
            protocol_controller: protocol_controller.clone(),
            grpc_config: grpc_private_config.clone(),
            protocol_config: protocol_config.clone(),
            node_id,
            mip_store: mip_store.clone(),
            version: *VERSION,
            stop_cv: sig_int_toggled.clone(),
            node_wallet: node_wallet.clone(),
            bs_white_black_list,
        };

        // Spawn gRPC PRIVATE API
        let grpc_private_stop_handle = grpc_private_api
            .serve(&grpc_private_config)
            .await
            .expect("failed to start gRPC PRIVATE API");
        info!(
            "gRPC | PRIVATE | listening on: {}",
            grpc_private_config.bind
        );

        Some(grpc_private_stop_handle)
    } else {
        None
    };

    #[cfg(feature = "op_spammer")]
    start_operation_injector(
        *GENESIS_TIMESTAMP,
        shared_storage.clone_without_refs(),
        node_wallet.read().clone(),
        pool_controller.clone(),
        protocol_controller.clone(),
        args.nb_op,
    );

    // spawn private API
    let api_private = API::<Private>::new(
        protocol_controller.clone(),
        execution_controller.clone(),
        api_config.clone(),
        sig_int_toggled,
        node_wallet,
    );
    let api_private_handle = api_private
        .serve(&SETTINGS.api.bind_private, &api_config)
        .await
        .expect("failed to start PRIVATE API");
    info!(
        "API | PRIVATE JsonRPC | listening on: {}",
        api_config.bind_private
    );

    // spawn public API
    let api_public = API::<Public>::new(
        consensus_controller.clone(),
        execution_controller.clone(),
        api_config.clone(),
        selector_controller.clone(),
        pool_controller.clone(),
        protocol_controller.clone(),
        protocol_config.clone(),
        *VERSION,
        node_id,
        shared_storage.clone(),
        mip_store.clone(),
    );
    let api_public_handle = api_public
        .serve(&SETTINGS.api.bind_public, &api_config)
        .await
        .expect("failed to start PUBLIC API");
    info!(
        "API | PUBLIC JsonRPC | listening on: {}",
        api_config.bind_public
    );

    let massa_survey_stopper = MassaSurvey::run(
        SETTINGS.metrics.tick_delay.to_duration(),
        execution_controller,
        pool_controller,
        massa_metrics,
        (
            api_config.thread_count,
            api_config.t0,
            api_config.genesis_timestamp,
            api_config.periods_per_cycle,
            api_config.last_start_period,
        ),
    );

    #[cfg(feature = "deadlock_detection")]
    {
        // only for #[cfg]
        use parking_lot::deadlock;
        use std::thread;

        let interval = Duration::from_secs(args.dl_interval);
        warn!("deadlocks detector will run every {:?}", interval);

        // Create a background thread which checks for deadlocks at the defined interval
        let thread_builder = thread::Builder::new().name("deadlock-detection".into());
        thread_builder
            .spawn(move || loop {
                thread::sleep(interval);
                let deadlocks = deadlock::check_deadlock();
                if deadlocks.is_empty() {
                    continue;
                }
                warn!("{} deadlocks detected", deadlocks.len());
                for (i, threads) in deadlocks.iter().enumerate() {
                    warn!("Deadlock #{}", i);
                    for t in threads {
                        warn!("Thread Id {:#?}", t.thread_id());
                        warn!("{:#?}", t.backtrace());
                    }
                }
            })
            .expect("failed to spawn thread : deadlock-detection");
    }
    (
        consensus_event_receiver,
        bootstrap_manager,
        consensus_manager,
        execution_manager,
        selector_manager,
        pool_manager,
        protocol_manager,
        factory_manager,
        api_private_handle,
        api_public_handle,
        api_handle,
        grpc_private_handle,
        grpc_public_handle,
        metrics_stopper,
        massa_survey_stopper,
    )
}

// Get the configuration of the gRPC server
fn configure_grpc(
    name: ServiceName,
    settings: &GrpcSettings,
    keypair: KeyPair,
    final_state: &Arc<RwLock<dyn FinalStateController>>,
    minimal_fees: Amount,
) -> GrpcConfig {
    GrpcConfig {
        name,
        enabled: settings.enabled,
        accept_http1: settings.accept_http1,
        enable_cors: settings.enable_cors,
        enable_health: settings.enable_health,
        enable_reflection: settings.enable_reflection,
        enable_tls: settings.enable_tls,
        enable_mtls: settings.enable_mtls,
        generate_self_signed_certificates: settings.generate_self_signed_certificates,
        subject_alt_names: settings.subject_alt_names.clone(),
        bind: settings.bind,
        accept_compressed: settings.accept_compressed.clone(),
        send_compressed: settings.send_compressed.clone(),
        max_decoding_message_size: settings.max_decoding_message_size,
        max_encoding_message_size: settings.max_encoding_message_size,
        concurrency_limit_per_connection: settings.concurrency_limit_per_connection,
        timeout: settings.timeout.to_duration(),
        initial_stream_window_size: settings.initial_stream_window_size,
        initial_connection_window_size: settings.initial_connection_window_size,
        max_concurrent_streams: settings.max_concurrent_streams,
        max_arguments: settings.max_arguments,
        tcp_keepalive: settings.tcp_keepalive.map(|t| t.to_duration()),
        tcp_nodelay: settings.tcp_nodelay,
        http2_keepalive_interval: settings.http2_keepalive_interval.map(|t| t.to_duration()),
        http2_keepalive_timeout: settings.http2_keepalive_timeout.map(|t| t.to_duration()),
        http2_adaptive_window: settings.http2_adaptive_window,
        max_frame_size: settings.max_frame_size,
        thread_count: THREAD_COUNT,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        endorsement_count: ENDORSEMENT_COUNT,
        max_endorsements_per_message: MAX_ENDORSEMENTS_PER_MESSAGE,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_datastore_entries_per_request: settings.max_datastore_entries_per_request,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameter_size: MAX_PARAMETERS_SIZE,
        max_operations_per_message: MAX_OPERATIONS_PER_MESSAGE,
        max_gas_per_block: MAX_GAS_PER_BLOCK,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        periods_per_cycle: PERIODS_PER_CYCLE,
        keypair,
        max_channel_size: settings.max_channel_size,
        draw_lookahead_period_count: settings.draw_lookahead_period_count,
        last_start_period: final_state.read().get_last_start_period(),
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        max_addresses_per_request: settings.max_addresses_per_request,
        max_slot_ranges_per_request: settings.max_slot_ranges_per_request,
        max_block_ids_per_request: settings.max_block_ids_per_request,
        max_endorsement_ids_per_request: settings.max_endorsement_ids_per_request,
        max_operation_ids_per_request: settings.max_operation_ids_per_request,
        max_filters_per_request: settings.max_filters_per_request,
        max_query_items_per_request: settings.max_query_items_per_request,
        certificate_authority_root_path: settings.certificate_authority_root_path.clone(),
        server_certificate_path: settings.server_certificate_path.clone(),
        server_private_key_path: settings.server_private_key_path.clone(),
        client_certificate_authority_root_path: settings
            .client_certificate_authority_root_path
            .clone(),
        client_certificate_path: settings.client_certificate_path.clone(),
        client_private_key_path: settings.client_private_key_path.clone(),
        chain_id: *CHAINID,
        minimal_fees,
    }
}

struct Managers {
    bootstrap_manager: Option<BootstrapManager>,
    consensus_manager: Box<dyn ConsensusManager>,
    execution_manager: Box<dyn ExecutionManager>,
    selector_manager: Box<dyn SelectorManager>,
    pool_manager: Box<dyn PoolManager>,
    protocol_manager: Box<dyn ProtocolManager>,
    factory_manager: Box<dyn FactoryManager>,
}

#[allow(clippy::too_many_arguments)]
async fn stop(
    _consensus_event_receiver: MassaReceiver<ConsensusEvent>,
    Managers {
        bootstrap_manager,
        mut execution_manager,
        mut consensus_manager,
        mut selector_manager,
        mut pool_manager,
        mut protocol_manager,
        mut factory_manager,
    }: Managers,
    api_private_handle: StopHandle,
    api_public_handle: StopHandle,
    api_handle: StopHandle,
    grpc_private_handle: Option<massa_grpc::server::StopHandle>,
    grpc_public_handle: Option<massa_grpc::server::StopHandle>,
    mut metrics_stopper: MetricsStopper,
    mut massa_survey_stopper: MassaSurveyStopper,
) {
    // stop bootstrap
    if let Some(bootstrap_manager) = bootstrap_manager {
        bootstrap_manager
            .stop()
            .expect("bootstrap server shutdown failed")
    }

    info!("Start stopping API's: gRPC(PUBLIC, PRIVATE), EXPERIMENTAL, PUBLIC, PRIVATE");

    // stop Massa gRPC PUBLIC API
    if let Some(handle) = grpc_public_handle {
        handle.stop();
    }
    info!("API | PUBLIC gRPC | stopped");

    // stop Massa gRPC PRIVATE API
    if let Some(handle) = grpc_private_handle {
        handle.stop();
    }
    info!("API | PRIVATE gRPC | stopped");

    // stop Massa API
    api_handle.stop().await;
    info!("API | EXPERIMENTAL JsonRPC | stopped");

    // stop public API
    api_public_handle.stop().await;
    info!("API | PUBLIC JsonRPC | stopped");

    // stop private API
    api_private_handle.stop().await;
    info!("API | PRIVATE JsonRPC | stopped");

    // stop metrics
    metrics_stopper.stop();

    // stop massa survey thread
    massa_survey_stopper.stop();

    // stop factory
    factory_manager.stop();

    // stop protocol controller
    protocol_manager.stop();

    // stop consensus
    consensus_manager.stop();

    // stop pool
    pool_manager.stop();

    // stop execution controller
    execution_manager.stop();

    // stop selector controller
    selector_manager.stop();

    // stop pool controller
    // TODO
    //let protocol_pool_event_receiver = pool_manager.stop().await.expect("pool shutdown failed");

    // note that FinalLedger gets destroyed as soon as its Arc count goes to zero
}

#[derive(Parser)]
#[command(version = crate_version!())]
struct Args {
    #[arg(long = "keep-ledger")]
    keep_ledger: bool,
    /// Wallet password
    // #[arg(short = "p", long = "pwd")]
    #[arg(short = 'p', long = "pwd")]
    password: Option<String>,

    /// restart_from_snapshot_at_period
    #[arg(long = "restart-from-snapshot-at-period")]
    restart_from_snapshot_at_period: Option<u64>,

    #[cfg(feature = "op_spammer")]
    /// number of operations
    #[arg(
        name = "number of operations",
        long_help = "Define the number of operations the node can spam.",
        short = 'n',
        long = "number-operations"
    )]
    nb_op: u64,

    #[cfg(feature = "deadlock_detection")]
    /// Deadlocks detector
    #[structopt(
        name = "deadlocks interval",
        long_help = "Define the interval of launching a deadlocks checking.",
        short = 'i',
        long = "dli",
        default_value = "10"
    )]
    dl_interval: u64,
}

/// Load wallet, asking for passwords if necessary
fn load_wallet(
    password: Option<String>,
    path: &Path,
    chain_id: u64,
) -> anyhow::Result<Arc<RwLock<Wallet>>> {
    let password = if path.is_dir() {
        password.unwrap_or_else(|| {
            Password::new()
                .with_prompt("Enter staking keys file password")
                .interact()
                .expect("IO error: Password reading failed, staking keys file couldn't be unlocked")
        })
    } else {
        password.unwrap_or_else(|| {
            Password::new()
                .with_prompt("Enter new password for staking keys file")
                .with_confirmation("Confirm password", "Passwords mismatching")
                .interact()
                .expect("IO error: Password reading failed, staking keys file couldn't be created")
        })
    };
    Ok(Arc::new(RwLock::new(Wallet::new(
        PathBuf::from(path),
        password,
        chain_id,
    )?)))
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .thread_name_fn(|| {
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
            format!("tokio-node-{}", id)
        })
        .enable_all()
        .build()
        .unwrap();

    tokio_rt.block_on(run(args))
}

async fn run(args: Args) -> anyhow::Result<()> {
    let mut cur_args = args;
    use tracing_subscriber::prelude::*;
    // spawn the console server in the background, returning a `Layer`:
    let tracing_layer = tracing_subscriber::fmt::layer()
        .with_filter(match SETTINGS.logging.level {
            4 => LevelFilter::TRACE,
            3 => LevelFilter::DEBUG,
            2 => LevelFilter::INFO,
            1 => LevelFilter::WARN,
            _ => LevelFilter::ERROR,
        })
        .with_filter(filter_fn(|metadata| {
            metadata.target().starts_with("massa") // ignore non-massa logs
        }));
    // build a `Subscriber` by combining layers with a `tracing_subscriber::Registry`:
    tracing_subscriber::registry()
        // add the console layer to the subscriber or default layers...
        .with(tracing_layer)
        .init();

    // Setup panic handlers,
    // and when a panic occurs,
    // run default handler,
    // and then shutdown.
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    info!("Node version : {}", *VERSION);

    // load or create wallet, asking for password if necessary
    let node_wallet = load_wallet(
        cur_args.password.clone(),
        &SETTINGS.factory.staking_wallet_path,
        *CHAINID,
    )?;

    // interrupt signal listener
    let sig_int_toggled = Arc::new((Mutex::new(false), Condvar::new()));

    let sig_int_toggled_clone = Arc::clone(&sig_int_toggled);
    ctrlc::set_handler(move || {
        *sig_int_toggled_clone
            .0
            .lock()
            .expect("double-lock on interupt bool in ctrl-c handler") = true;
        sig_int_toggled_clone.1.notify_all();
    })
    .expect("Error setting Ctrl-C handler");

    #[cfg(feature = "resync_check")]
    let mut resync_check = Some(std::time::Instant::now() + std::time::Duration::from_secs(10));

    loop {
        let (
            consensus_event_receiver,
            bootstrap_manager,
            consensus_manager,
            execution_manager,
            selector_manager,
            pool_manager,
            protocol_manager,
            factory_manager,
            api_private_handle,
            api_public_handle,
            api_handle,
            grpc_private_handle,
            grpc_public_handle,
            metrics_stopper,
            massa_survey_stopper,
        ) = launch(&cur_args, node_wallet.clone(), Arc::clone(&sig_int_toggled)).await;

        // loop over messages
        let restart = loop {
            massa_trace!("massa-node.main.run.select", {});
            match consensus_event_receiver.try_recv() {
                Ok(evt) => match evt {
                    ConsensusEvent::NeedSync => {
                        warn!("in response to a desynchronization, the node is going to bootstrap again");
                        break true;
                    }
                    ConsensusEvent::Stop => {
                        break false;
                    }
                },
                Err(TryRecvError::Disconnected) => {
                    error!("consensus_event_receiver.wait_event disconnected");
                    break false;
                }
                _ => {}
            };

            // every 100ms/or when alerted, check if sigint toggled
            // if toggled, break loop
            let int_sig = sig_int_toggled
                .0
                .lock()
                .expect("double-lock() on interupted signal mutex");
            let wake = sig_int_toggled
                .1
                .wait_timeout(int_sig, Duration::from_millis(100))
                .expect("interupt signal mutex poisoned");
            if *wake.0 {
                info!("interrupt signal received");
                break false;
            }

            // Elements of the system that involve stopping and restarting should be checked by forcing a relaunch.
            // This check allows the system to start up as normal, wait 10s, then force a relaunch. If Things take too long
            // to shutdown, or does not allow for a clean relaunch, this feature flag can expose those issues.
            #[cfg(feature = "resync_check")]
            if let Some(resync_moment) = resync_check {
                if resync_moment < std::time::Instant::now() {
                    warn!("resync check triggered");
                    resync_check = None;
                    break true;
                }
            }
        };
        stop(
            consensus_event_receiver,
            Managers {
                bootstrap_manager,
                consensus_manager,
                execution_manager,
                selector_manager,
                pool_manager,
                protocol_manager,
                factory_manager,
            },
            api_private_handle,
            api_public_handle,
            api_handle,
            grpc_private_handle,
            grpc_public_handle,
            metrics_stopper,
            massa_survey_stopper,
        )
        .await;

        if !restart {
            break;
        }
        // If we restart because of a desync, then we do not want to restart from a snapshot
        cur_args.restart_from_snapshot_at_period = None;
    }
    Ok(())
}
