// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![doc = include_str!("../../README.md")]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(ip)]
extern crate massa_logging;

use crate::settings::SETTINGS;

use chrono::{TimeZone, Utc};
use crossbeam_channel::{Receiver, TryRecvError};
use dialoguer::Password;
use massa_api::{ApiServer, ApiV2, Private, Public, RpcServer, StopHandle, API};
use massa_api_exports::config::APIConfig;
use massa_async_pool::AsyncPoolConfig;
use massa_bootstrap::BootstrapError;
use massa_bootstrap::{
    get_state, start_bootstrap_server, BootstrapConfig, BootstrapManager, BootstrapTcpListener,
    DefaultConnector,
};
use massa_consensus_exports::events::ConsensusEvent;
use massa_consensus_exports::{ConsensusChannels, ConsensusConfig, ConsensusManager};
use massa_consensus_worker::start_consensus_worker;
use massa_executed_ops::{ExecutedDenunciationsConfig, ExecutedOpsConfig};
use massa_execution_exports::{
    ExecutionChannels, ExecutionConfig, ExecutionManager, GasCosts, StorageCostsConstants,
};
use massa_execution_worker::start_execution_worker;
use massa_factory_exports::{FactoryChannels, FactoryConfig, FactoryManager};
use massa_factory_worker::start_factory;
use massa_final_state::{FinalState, FinalStateConfig};
use massa_grpc::config::GrpcConfig;
use massa_grpc::server::MassaGrpc;
use massa_ledger_exports::LedgerConfig;
use massa_ledger_worker::FinalLedger;
use massa_logging::massa_trace;
use massa_models::address::Address;
use massa_models::config::constants::{
    ASYNC_POOL_BOOTSTRAP_PART_SIZE, BLOCK_REWARD, BOOTSTRAP_RANDOMNESS_SIZE_BYTES, CHANNEL_SIZE,
    CONSENSUS_BOOTSTRAP_PART_SIZE, DEFERRED_CREDITS_BOOTSTRAP_PART_SIZE, DELTA_F0,
    DENUNCIATION_EXPIRE_PERIODS, ENDORSEMENT_COUNT, END_TIMESTAMP,
    EXECUTED_OPS_BOOTSTRAP_PART_SIZE, GENESIS_KEY, GENESIS_TIMESTAMP, INITIAL_DRAW_SEED,
    LEDGER_COST_PER_BYTE, LEDGER_ENTRY_BASE_SIZE, LEDGER_ENTRY_DATASTORE_BASE_SIZE,
    LEDGER_PART_SIZE_MESSAGE_BYTES, MAX_ADVERTISE_LENGTH, MAX_ASK_BLOCKS_PER_MESSAGE,
    MAX_ASYNC_GAS, MAX_ASYNC_MESSAGE_DATA, MAX_ASYNC_POOL_LENGTH, MAX_BLOCK_SIZE,
    MAX_BOOTSTRAP_ASYNC_POOL_CHANGES, MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_ERROR_LENGTH,
    MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_BYTECODE_LENGTH, MAX_CONSENSUS_BLOCKS_IDS,
    MAX_DATASTORE_ENTRY_COUNT, MAX_DATASTORE_KEY_LENGTH, MAX_DATASTORE_VALUE_LENGTH,
    MAX_DEFERRED_CREDITS_LENGTH, MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
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
    MIP_STORE_STATS_COUNTERS_MAX, OPERATION_VALIDITY_PERIODS, PERIODS_PER_CYCLE,
    POOL_CONTROLLER_CHANNEL_SIZE, POS_MISS_RATE_DEACTIVATION_THRESHOLD, POS_SAVED_CYCLES,
    PROTOCOL_CONTROLLER_CHANNEL_SIZE, PROTOCOL_EVENT_CHANNEL_SIZE,
    ROLL_COUNT_TO_SLASH_ON_DENUNCIATION, ROLL_PRICE, SELECTOR_DRAW_CACHE_SIZE, T0, THREAD_COUNT,
    VERSION,
};
use massa_pool_exports::{PoolChannels, PoolConfig, PoolManager};
use massa_pool_worker::start_pool_controller;
use massa_pos_exports::{PoSConfig, SelectorConfig, SelectorManager};
use massa_pos_worker::start_selector_worker;
use massa_protocol_exports::{ProtocolConfig, ProtocolManager};
use massa_protocol_worker::{create_protocol_controller, start_protocol_controller};
use massa_storage::Storage;
use massa_time::MassaTime;
use massa_versioning_worker::versioning::{ComponentStateTypeId, MipStatsConfig, MipStore};
use massa_wallet::Wallet;
use parking_lot::RwLock;
use peernet::transports::TransportType;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread::sleep;
use std::time::Duration;
use std::{path::Path, process, sync::Arc};
use structopt::StructOpt;
use tokio::signal;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};
use tracing_subscriber::filter::{filter_fn, LevelFilter};

mod settings;

async fn launch(
    args: &Args,
    node_wallet: Arc<RwLock<Wallet>>,
    sig_int_toggled: Arc<(Mutex<bool>, Condvar)>,
) -> (
    Receiver<ConsensusEvent>,
    Option<BootstrapManager>,
    Box<dyn ConsensusManager>,
    Box<dyn ExecutionManager>,
    Box<dyn SelectorManager>,
    Box<dyn PoolManager>,
    Box<dyn ProtocolManager>,
    Box<dyn FactoryManager>,
    mpsc::Receiver<()>,
    StopHandle,
    StopHandle,
    StopHandle,
    Option<massa_grpc::server::StopHandle>,
) {
    info!("Node version : {}", *VERSION);
    let now = MassaTime::now().expect("could not get now time");
    // Do not start if genesis is in the future. This is meant to prevent nodes
    // from desync if the bootstrap nodes keep a previous ledger
    #[cfg(all(not(feature = "sandbox"), not(feature = "bootstrap_server")))]
    {
        if *GENESIS_TIMESTAMP > now {
            let (days, hours, mins, secs) = GENESIS_TIMESTAMP
                .saturating_sub(now)
                .days_hours_mins_secs()
                .unwrap();
            panic!(
                "This episode has not started yet, please wait {} days, {} hours, {} minutes, {} seconds for genesis",
                days, hours, mins, secs,
            )
        }
    }

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
        disk_ledger_path: SETTINGS.ledger.disk_ledger_path.clone(),
        max_key_length: MAX_DATASTORE_KEY_LENGTH,
        max_ledger_part_size: LEDGER_PART_SIZE_MESSAGE_BYTES,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
    };
    let async_pool_config = AsyncPoolConfig {
        max_length: MAX_ASYNC_POOL_LENGTH,
        thread_count: THREAD_COUNT,
        bootstrap_part_size: ASYNC_POOL_BOOTSTRAP_PART_SIZE,
        max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
    };
    let pos_config = PoSConfig {
        periods_per_cycle: PERIODS_PER_CYCLE,
        thread_count: THREAD_COUNT,
        cycle_history_length: POS_SAVED_CYCLES,
        credits_bootstrap_part_size: DEFERRED_CREDITS_BOOTSTRAP_PART_SIZE,
    };
    let executed_ops_config = ExecutedOpsConfig {
        thread_count: THREAD_COUNT,
        bootstrap_part_size: EXECUTED_OPS_BOOTSTRAP_PART_SIZE,
    };
    let executed_denunciations_config = ExecutedDenunciationsConfig {
        denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
        bootstrap_part_size: EXECUTED_OPS_BOOTSTRAP_PART_SIZE,
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
    };

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

    // Create final ledger
    let ledger = FinalLedger::new(
        ledger_config.clone(),
        args.restart_from_snapshot_at_period.is_some() || cfg!(feature = "create_snapshot"),
    );

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

    // Create final state, either from a snapshot, or from scratch
    let final_state = Arc::new(parking_lot::RwLock::new(
        match args.restart_from_snapshot_at_period {
            Some(last_start_period) => FinalState::new_derived_from_snapshot(
                final_state_config,
                Box::new(ledger),
                selector_controller.clone(),
                last_start_period,
            )
            .expect("could not init final state"),
            None => FinalState::new(
                final_state_config,
                Box::new(ledger),
                selector_controller.clone(),
            )
            .expect("could not init final state"),
        },
    ));

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
        max_bytes_read_write: SETTINGS.bootstrap.max_bytes_read_write,
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
        mip_store_stats_counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        max_denunciation_changes_length: MAX_DENUNCIATION_CHANGES_LENGTH,
    };

    let bootstrap_state = match get_state(
        &bootstrap_config,
        final_state.clone(),
        DefaultConnector,
        *VERSION,
        *GENESIS_TIMESTAMP,
        *END_TIMESTAMP,
        args.restart_from_snapshot_at_period,
        sig_int_toggled,
    ) {
        Ok(vals) => vals,
        Err(BootstrapError::Interupted(msg)) => {
            info!("{}", msg);
            process::exit(0);
        }
        Err(err) => panic!("critical error detected in the bootstrap process: {}", err),
    };

    if args.restart_from_snapshot_at_period.is_none() {
        let last_start_period = final_state.read().last_start_period;
        final_state.write().init_ledger_hash(last_start_period);

        // give the controller to final state in order for it to feed the cycles
        final_state
            .write()
            .compute_initial_draws()
            .expect("could not compute initial draws"); // TODO: this might just mean a bad bootstrap, no need to panic, just reboot
    }

    // Storage costs constants
    let storage_costs_constants = StorageCostsConstants {
        ledger_cost_per_byte: LEDGER_COST_PER_BYTE,
        ledger_entry_base_cost: LEDGER_COST_PER_BYTE
            .checked_mul_u64(LEDGER_ENTRY_BASE_SIZE as u64)
            .expect("Overflow when creating constant ledger_entry_base_cost"),
        ledger_entry_datastore_base_cost: LEDGER_COST_PER_BYTE
            .checked_mul_u64(LEDGER_ENTRY_DATASTORE_BASE_SIZE as u64)
            .expect("Overflow when creating constant ledger_entry_datastore_base_size"),
    };

    // Creates an empty default store
    let mip_stats_config = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        counters_max: MIP_STORE_STATS_COUNTERS_MAX,
    };
    let mut mip_store =
        MipStore::try_from(([], mip_stats_config)).expect("Cannot create an empty MIP store");
    if let Some(bootstrap_mip_store) = bootstrap_state.mip_store {
        // TODO: in some cases, should bootstrap again
        let (updated, added) = mip_store
            .update_with(&bootstrap_mip_store)
            .expect("Cannot update MIP store with bootstrap mip store");

        if !added.is_empty() {
            for (mip_info, mip_state) in added.iter() {
                let now = MassaTime::now().expect("Cannot get current time");
                match mip_state.state_at(now, mip_info.start, mip_info.timeout) {
                    Ok(st_id) => {
                        if st_id == ComponentStateTypeId::LockedIn {
                            // A new MipInfo @ state locked_in - we need to urge the user to update
                            warn!(
                                "A new MIP has been received: {}, version: {}",
                                mip_info.name, mip_info.version
                            );
                            // Safe to unwrap here (only panic if not LockedIn)
                            let activation_at = mip_state.activation_at(mip_info).unwrap();
                            let dt = Utc
                                .timestamp_opt(activation_at.to_duration().as_secs() as i64, 0)
                                .unwrap();
                            warn!("Please update your Massa node before: {}", dt.to_rfc2822());
                        } else if st_id == ComponentStateTypeId::Active {
                            // A new MipInfo @ state active - we are not compatible anymore
                            warn!(
                                "A new MIP has been received {:?}, version: {:?}",
                                mip_info.name, mip_info.version
                            );
                            panic!("Please update your Massa node to support it");
                        } else if st_id == ComponentStateTypeId::Defined {
                            // a new MipInfo @ state defined or started (or failed / error)
                            // warn the user to update its node
                            warn!(
                                "A new MIP has been received: {}, version: {}",
                                mip_info.name, mip_info.version
                            );
                            debug!("MIP state: {:?}", mip_state);
                            let dt_start = Utc
                                .timestamp_opt(mip_info.start.to_duration().as_secs() as i64, 0)
                                .unwrap();
                            let dt_timeout = Utc
                                .timestamp_opt(mip_info.timeout.to_duration().as_secs() as i64, 0)
                                .unwrap();
                            warn!("Please update your node between: {} and {} if you want to support this update",
                                dt_start.to_rfc2822(),
                                dt_timeout.to_rfc2822()
                            );
                        } else {
                            // a new MipInfo @ state defined or started (or failed / error)
                            // warn the user to update its node
                            warn!(
                                "A new MIP has been received: {}, version: {}",
                                mip_info.name, mip_info.version
                            );
                            debug!("MIP state: {:?}", mip_state);
                            warn!("Please update your Massa node to support it");
                        }
                    }
                    Err(e) => {
                        // Should never happen
                        panic!(
                            "Unable to get state at {} of mip info: {:?}, error: {}",
                            now, mip_info, e
                        )
                    }
                }
            }
        }

        debug!("MIP store got {} MIP updated from bootstrap", updated.len());
    }

    // launch execution module
    let execution_config = ExecutionConfig {
        max_final_events: SETTINGS.execution.max_final_events,
        readonly_queue_length: SETTINGS.execution.readonly_queue_length,
        cursor_delay: SETTINGS.execution.cursor_delay,
        max_async_gas: MAX_ASYNC_GAS,
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
        initial_vesting_path: SETTINGS.execution.initial_vesting_path.clone(),
        gas_costs: GasCosts::new(
            SETTINGS.execution.abi_gas_costs_file.clone(),
            SETTINGS.execution.wasm_gas_costs_file.clone(),
        )
        .expect("Failed to load gas costs"),
        last_start_period: final_state.read().last_start_period,
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
    };

    let execution_channels = ExecutionChannels {
        slot_execution_output_sender: broadcast::channel(
            execution_config.broadcast_slot_execution_output_channel_capacity,
        )
        .0,
    };

    let (execution_manager, execution_controller) = start_execution_worker(
        execution_config,
        final_state.clone(),
        selector_controller.clone(),
        mip_store.clone(),
        execution_channels.clone(),
    );

    // launch pool controller
    let pool_config = PoolConfig {
        thread_count: THREAD_COUNT,
        max_block_size: MAX_BLOCK_SIZE,
        max_block_gas: MAX_GAS_PER_BLOCK,
        roll_price: ROLL_PRICE,
        max_block_endorsement_count: ENDORSEMENT_COUNT,
        operation_validity_periods: OPERATION_VALIDITY_PERIODS,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        max_operation_pool_size_per_thread: SETTINGS.pool.max_pool_size_per_thread,
        max_endorsements_pool_size_per_thread: SETTINGS.pool.max_pool_size_per_thread,
        channels_size: POOL_CONTROLLER_CHANNEL_SIZE,
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
        last_start_period: final_state.read().last_start_period,
    };

    let pool_channels = PoolChannels {
        endorsement_sender: broadcast::channel(pool_config.broadcast_endorsements_channel_capacity)
            .0,
        operation_sender: broadcast::channel(pool_config.broadcast_operations_channel_capacity).0,
        selector: selector_controller.clone(),
    };

    let (pool_manager, pool_controller) = start_pool_controller(
        pool_config,
        &shared_storage,
        execution_controller.clone(),
        pool_channels.clone(),
    );

    // launch protocol controller
    let mut listeners = HashMap::default();
    listeners.insert(SETTINGS.protocol.bind, TransportType::Tcp);
    let protocol_config = ProtocolConfig {
        thread_count: THREAD_COUNT,
        ask_block_timeout: SETTINGS.protocol.ask_block_timeout,
        max_known_blocks_size: SETTINGS.protocol.max_known_blocks_size,
        max_node_known_blocks_size: SETTINGS.protocol.max_node_known_blocks_size,
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
        asked_operations_pruning_period: SETTINGS.protocol.asked_operations_pruning_period,
        operation_announcement_interval: SETTINGS.protocol.operation_announcement_interval,
        max_operations_per_message: SETTINGS.protocol.max_operations_per_message,
        max_serialized_operations_size_per_block: MAX_BLOCK_SIZE as usize,
        max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
        controller_channel_size: PROTOCOL_CONTROLLER_CHANNEL_SIZE,
        event_channel_size: PROTOCOL_EVENT_CHANNEL_SIZE,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        endorsement_count: ENDORSEMENT_COUNT,
        max_operations_propagation_time: SETTINGS.protocol.max_operations_propagation_time,
        max_endorsements_propagation_time: SETTINGS.protocol.max_endorsements_propagation_time,
        last_start_period: final_state.read().last_start_period,
        max_endorsements_per_message: MAX_ENDORSEMENTS_PER_MESSAGE as u64,
        max_denunciations_in_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        initial_peers: SETTINGS.protocol.initial_peers_file.clone(),
        listeners,
        keypair_file: SETTINGS.protocol.keypair_file.clone(),
        max_known_blocks_saved_size: SETTINGS.protocol.max_known_blocks_size,
        asked_operations_buffer_capacity: SETTINGS.protocol.max_known_ops_size,
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
        max_size_block_infos: MAX_ASK_BLOCKS_PER_MESSAGE as u64,
        max_size_listeners_per_peer: MAX_LISTENERS_PER_PEER,
        max_size_peers_announcement: MAX_PEERS_IN_ANNOUNCEMENT_LIST,
        read_write_limit_bytes_per_second: SETTINGS.protocol.read_write_limit_bytes_per_second
            as u128,
        try_connection_timer: SETTINGS.protocol.try_connection_timer,
        max_in_connections: SETTINGS.protocol.max_in_connections,
        timeout_connection: SETTINGS.protocol.timeout_connection,
        routable_ip: SETTINGS
            .protocol
            .routable_ip
            .or(SETTINGS.network.routable_ip),
        debug: false,
        peers_categories: SETTINGS.protocol.peers_categories.clone(),
        default_category_info: SETTINGS.protocol.default_category_info,
        version: *VERSION,
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
        future_block_processing_max_periods: SETTINGS.consensus.future_block_processing_max_periods,
        max_future_processing_blocks: SETTINGS.consensus.max_future_processing_blocks,
        max_dependency_blocks: SETTINGS.consensus.max_dependency_blocks,
        delta_f0: DELTA_F0,
        operation_validity_periods: OPERATION_VALIDITY_PERIODS,
        periods_per_cycle: PERIODS_PER_CYCLE,
        stats_timespan: SETTINGS.consensus.stats_timespan,
        max_send_wait: SETTINGS.consensus.max_send_wait,
        force_keep_final_periods: SETTINGS.consensus.force_keep_final_periods,
        endorsement_count: ENDORSEMENT_COUNT,
        block_db_prune_interval: SETTINGS.consensus.block_db_prune_interval,
        max_item_return_count: SETTINGS.consensus.max_item_return_count,
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
        last_start_period: final_state.read().last_start_period,
    };

    let (consensus_event_sender, consensus_event_receiver) =
        crossbeam_channel::bounded(CHANNEL_SIZE);
    let consensus_channels = ConsensusChannels {
        execution_controller: execution_controller.clone(),
        selector_controller: selector_controller.clone(),
        pool_controller: pool_controller.clone(),
        controller_event_tx: consensus_event_sender,
        protocol_controller: protocol_controller.clone(),
        block_header_sender: broadcast::channel(
            consensus_config.broadcast_blocks_headers_channel_capacity,
        )
        .0,
        block_sender: broadcast::channel(consensus_config.broadcast_blocks_channel_capacity).0,
        filled_block_sender: broadcast::channel(
            consensus_config.broadcast_filled_blocks_channel_capacity,
        )
        .0,
    };

    let (consensus_controller, consensus_manager) = start_consensus_worker(
        consensus_config,
        consensus_channels.clone(),
        bootstrap_state.graph,
        shared_storage.clone(),
    );

    let (protocol_manager, keypair, node_id) = start_protocol_controller(
        protocol_config.clone(),
        consensus_controller.clone(),
        bootstrap_state.peers,
        pool_controller.clone(),
        shared_storage.clone(),
        protocol_channels,
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
        last_start_period: final_state.read().last_start_period,
        periods_per_cycle: PERIODS_PER_CYCLE,
        denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
    };
    let factory_channels = FactoryChannels {
        selector: selector_controller.clone(),
        consensus: consensus_controller.clone(),
        pool: pool_controller.clone(),
        protocol: protocol_controller.clone(),
        storage: shared_storage.clone(),
    };
    let factory_manager = start_factory(factory_config, node_wallet.clone(), factory_channels);

    let bootstrap_manager = bootstrap_config.listen_addr.map(|addr| {
        let (waker, listener) = BootstrapTcpListener::new(&addr).unwrap_or_else(|_| {
            panic!(
                "{}",
                format!("Could not bind to address: {}", addr).as_str()
            )
        });
        let mut manager = start_bootstrap_server(
            listener,
            consensus_controller.clone(),
            protocol_controller.clone(),
            final_state.clone(),
            bootstrap_config,
            keypair.clone(),
            *VERSION,
            mip_store.clone(),
        )
        .expect("Could not start bootstrap server");
        manager.set_listener_stopper(waker);
        manager
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
        batch_requests_supported: SETTINGS.api.batch_requests_supported,
        ping_interval: SETTINGS.api.ping_interval,
        enable_http: SETTINGS.api.enable_http,
        enable_ws: SETTINGS.api.enable_ws,
        max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
        max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
        max_parameter_size: MAX_PARAMETERS_SIZE,
        thread_count: THREAD_COUNT,
        keypair,
        genesis_timestamp: *GENESIS_TIMESTAMP,
        t0: T0,
        periods_per_cycle: PERIODS_PER_CYCLE,
    };

    // spawn Massa API
    let api = API::<ApiV2>::new(
        consensus_controller.clone(),
        consensus_channels.clone(),
        execution_controller.clone(),
        pool_channels.clone(),
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

    // Whether to spawn gRPC API
    let grpc_handle = if SETTINGS.grpc.enabled {
        let grpc_config = GrpcConfig {
            enabled: SETTINGS.grpc.enabled,
            accept_http1: SETTINGS.grpc.accept_http1,
            enable_cors: SETTINGS.grpc.enable_cors,
            enable_health: SETTINGS.grpc.enable_health,
            enable_reflection: SETTINGS.grpc.enable_reflection,
            enable_mtls: SETTINGS.grpc.enable_mtls,
            bind: SETTINGS.grpc.bind,
            accept_compressed: SETTINGS.grpc.accept_compressed.clone(),
            send_compressed: SETTINGS.grpc.send_compressed.clone(),
            max_decoding_message_size: SETTINGS.grpc.max_decoding_message_size,
            max_encoding_message_size: SETTINGS.grpc.max_encoding_message_size,
            concurrency_limit_per_connection: SETTINGS.grpc.concurrency_limit_per_connection,
            timeout: SETTINGS.grpc.timeout.to_duration(),
            initial_stream_window_size: SETTINGS.grpc.initial_stream_window_size,
            initial_connection_window_size: SETTINGS.grpc.initial_connection_window_size,
            max_concurrent_streams: SETTINGS.grpc.max_concurrent_streams,
            tcp_keepalive: SETTINGS.grpc.tcp_keepalive.map(|t| t.to_duration()),
            tcp_nodelay: SETTINGS.grpc.tcp_nodelay,
            http2_keepalive_interval: SETTINGS
                .grpc
                .http2_keepalive_interval
                .map(|t| t.to_duration()),
            http2_keepalive_timeout: SETTINGS
                .grpc
                .http2_keepalive_timeout
                .map(|t| t.to_duration()),
            http2_adaptive_window: SETTINGS.grpc.http2_adaptive_window,
            max_frame_size: SETTINGS.grpc.max_frame_size,
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_endorsements_per_message: MAX_ENDORSEMENTS_PER_MESSAGE,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
            max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
            max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
            max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
            max_parameter_size: MAX_PARAMETERS_SIZE,
            max_operations_per_message: MAX_OPERATIONS_PER_MESSAGE,
            genesis_timestamp: *GENESIS_TIMESTAMP,
            t0: T0,
            periods_per_cycle: PERIODS_PER_CYCLE,
            max_channel_size: SETTINGS.grpc.max_channel_size,
            draw_lookahead_period_count: SETTINGS.grpc.draw_lookahead_period_count,
            last_start_period: final_state.read().last_start_period,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            max_block_ids_per_request: SETTINGS.grpc.max_block_ids_per_request,
            max_operation_ids_per_request: SETTINGS.grpc.max_operation_ids_per_request,
            server_certificate_path: SETTINGS.grpc.server_certificate_path.clone(),
            server_private_key_path: SETTINGS.grpc.server_private_key_path.clone(),
            client_certificate_authority_root_path: SETTINGS
                .grpc
                .client_certificate_authority_root_path
                .clone(),
        };

        let grpc_api = MassaGrpc {
            consensus_controller: consensus_controller.clone(),
            consensus_channels: consensus_channels.clone(),
            execution_controller: execution_controller.clone(),
            execution_channels,
            pool_channels,
            pool_command_sender: pool_controller.clone(),
            protocol_command_sender: protocol_controller.clone(),
            selector_controller: selector_controller.clone(),
            storage: shared_storage.clone(),
            grpc_config: grpc_config.clone(),
            version: *VERSION,
            mip_store,
        };

        // HACK maybe should remove timeout later
        if let Ok(result) =
            tokio::time::timeout(Duration::from_secs(3), grpc_api.serve(&grpc_config)).await
        {
            match result {
                Ok(stop) => {
                    info!("API | gRPC | listening on: {}", grpc_config.bind);
                    Some(stop)
                }
                Err(e) => {
                    error!("{}", e);
                    None
                }
            }
        } else {
            error!("Timeout on start grpc API");
            None
        }
    } else {
        None
    };

    // spawn private API
    let (api_private, api_private_stop_rx) = API::<Private>::new(
        protocol_controller.clone(),
        execution_controller.clone(),
        api_config.clone(),
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
    );
    let api_public_handle = api_public
        .serve(&SETTINGS.api.bind_public, &api_config)
        .await
        .expect("failed to start PUBLIC API");
    info!(
        "API | PUBLIC JsonRPC | listening on: {}",
        api_config.bind_public
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
        api_private_stop_rx,
        api_private_handle,
        api_public_handle,
        api_handle,
        grpc_handle,
    )
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

async fn stop(
    _consensus_event_receiver: Receiver<ConsensusEvent>,
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
    grpc_handle: Option<massa_grpc::server::StopHandle>,
) {
    // stop bootstrap
    if let Some(bootstrap_manager) = bootstrap_manager {
        bootstrap_manager
            .stop()
            .expect("bootstrap server shutdown failed")
    }

    info!("Start stopping API's: gRPC, EXPERIMENTAL, PUBLIC, PRIVATE");

    // stop Massa gRPC API
    if let Some(handle) = grpc_handle {
        handle.stop();
    }

    // stop Massa API
    api_handle.stop().await;
    info!("API | EXPERIMENTAL JsonRPC | stopped");

    // stop public API
    api_public_handle.stop().await;
    info!("API | PUBLIC JsonRPC | stopped");

    // stop private API
    api_private_handle.stop().await;
    info!("API | PRIVATE JsonRPC | stopped");

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

#[derive(StructOpt)]
struct Args {
    #[structopt(long = "keep-ledger")]
    keep_ledger: bool,
    /// Wallet password
    #[structopt(short = "p", long = "pwd")]
    password: Option<String>,

    /// restart_from_snapshot_at_period
    #[structopt(long = "restart-from-snapshot-at-period")]
    restart_from_snapshot_at_period: Option<u64>,

    #[cfg(feature = "deadlock_detection")]
    /// Deadlocks detector
    #[structopt(
        name = "deadlocks interval",
        about = "Define the interval of launching a deadlocks checking.",
        short = "i",
        long = "dli",
        default_value = "10"
    )]
    dl_interval: u64,
}

/// Load wallet, asking for passwords if necessary
fn load_wallet(password: Option<String>, path: &Path) -> anyhow::Result<Arc<RwLock<Wallet>>> {
    let password = if path.is_file() {
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
    )?)))
}

#[paw::main]
fn main(args: Args) -> anyhow::Result<()> {
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

    // load or create wallet, asking for password if necessary
    let node_wallet = load_wallet(
        cur_args.password.clone(),
        &SETTINGS.factory.staking_wallet_path,
    )?;

    // interrupt signal listener
    let sig_int_toggled = Arc::new((Mutex::new(false), Condvar::new()));
    let sig_int_toggled_clone = Arc::clone(&sig_int_toggled);

    // currently used by the bootstrap client to break out of the to preempt the retry wait
    ctrlc::set_handler(move || {
        *sig_int_toggled_clone
            .0
            .lock()
            .expect("double-lock on interupt bool in ctrl-c handler") = true;
        sig_int_toggled_clone.1.notify_all();
    })
    .expect("Error setting Ctrl-C handler");

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
            mut api_private_stop_rx,
            api_private_handle,
            api_public_handle,
            api_handle,
            grpc_handle,
        ) = launch(&cur_args, node_wallet.clone(), Arc::clone(&sig_int_toggled)).await;

        // interrupt signal listener
        let (tx, rx) = crossbeam_channel::bounded(1);
        let interrupt_signal_listener = tokio::spawn(async move {
            signal::ctrl_c().await.unwrap();
            tx.send(()).unwrap();
        });

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

            match api_private_stop_rx.try_recv() {
                Ok(_) => {
                    info!("stop command received from private API");
                    break false;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    error!("api_private_stop_rx disconnected");
                    break false;
                }
                _ => {}
            }
            match rx.try_recv() {
                Ok(_) => {
                    info!("interrupt signal received");
                    break false;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    error!("interrupt_signal_listener disconnected");
                    break false;
                }
                _ => {}
            }
            sleep(Duration::from_millis(100));
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
            grpc_handle,
        )
        .await;

        if !restart {
            break;
        }
        // If we restart because of a desync, then we do not want to restart from a snapshot
        cur_args.restart_from_snapshot_at_period = None;
        interrupt_signal_listener.abort();
    }
    Ok(())
}
