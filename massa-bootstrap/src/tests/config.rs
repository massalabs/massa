use std::{net::SocketAddr, path::PathBuf};

use massa_models::config::CHAINID;
use massa_models::{
    config::{
        BOOTSTRAP_RANDOMNESS_SIZE_BYTES, CONSENSUS_BOOTSTRAP_PART_SIZE, ENDORSEMENT_COUNT,
        MAX_ADVERTISE_LENGTH, MAX_BOOTSTRAP_BLOCKS, MAX_BOOTSTRAP_ERROR_LENGTH,
        MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE,
        MAX_CONSENSUS_BLOCKS_IDS, MAX_DATASTORE_ENTRY_COUNT, MAX_DATASTORE_KEY_LENGTH,
        MAX_DATASTORE_VALUE_LENGTH, MAX_DEFERRED_CREDITS_LENGTH,
        MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_DENUNCIATION_CHANGES_LENGTH,
        MAX_EXECUTED_OPS_CHANGES_LENGTH, MAX_EXECUTED_OPS_LENGTH, MAX_FUNCTION_NAME_LENGTH,
        MAX_LEDGER_CHANGES_COUNT, MAX_OPERATIONS_PER_BLOCK, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        MAX_PARAMETERS_SIZE, MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH,
        MIP_STORE_STATS_BLOCK_CONSIDERED, PERIODS_PER_CYCLE, THREAD_COUNT,
    },
    node::NodeId,
};
use massa_signature::KeyPair;
use massa_time::MassaTime;

use crate::{BootstrapConfig, IpType};

use super::tools::BASE_BOOTSTRAP_IP;

impl Default for BootstrapConfig {
    fn default() -> Self {
        let node_id = NodeId::new(KeyPair::generate(0).unwrap().get_public_key());
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
            bootstrap_list: vec![(SocketAddr::new(BASE_BOOTSTRAP_IP, 8069), node_id)],
            keep_ledger: false,
            bootstrap_whitelist_path: PathBuf::from("bootstrap_whitelist.json"),
            bootstrap_blacklist_path: PathBuf::from("bootstrap_blacklist.json"),
            max_clock_delta: MassaTime::from_millis(1000),
            cache_duration: MassaTime::from_millis(10000),
            max_simultaneous_bootstraps: 2,
            ip_list_max_size: 10,
            per_ip_min_interval: MassaTime::from_millis(10000),
            rate_limit: u64::MAX,
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
}
