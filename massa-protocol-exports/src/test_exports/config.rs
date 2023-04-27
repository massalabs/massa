use std::collections::HashMap;

use crate::ProtocolConfig;
use massa_models::config::ENDORSEMENT_COUNT;
use massa_time::MassaTime;
use tempfile::NamedTempFile;

impl Default for ProtocolConfig {
    fn default() -> Self {
        ProtocolConfig {
            max_in_connections: 10,
            max_out_connections: 10,
            keypair_file: NamedTempFile::new()
                .expect("cannot create temp file")
                .path()
                .to_path_buf(),
            ask_block_timeout: 500.into(),
            max_known_blocks_saved_size: 300,
            max_known_blocks_size: 100,
            max_node_known_blocks_size: 100,
            max_node_wanted_blocks_size: 100,
            max_simultaneous_ask_blocks_per_node: 10,
            max_send_wait: MassaTime::from_millis(100),
            max_known_ops_size: 1000,
            max_node_known_ops_size: 1000,
            max_known_endorsements_size: 1000,
            max_node_known_endorsements_size: 1000,
            operation_batch_buffer_capacity: 1000,
            operation_announcement_buffer_capacity: 1000,
            max_operation_storage_time: MassaTime::from_millis(60000),
            operation_batch_proc_period: 200.into(),
            asked_operations_buffer_capacity: 10000,
            asked_operations_pruning_period: 500.into(),
            operation_announcement_interval: 150.into(),
            max_operations_per_message: 1024,
            max_operations_per_block: 5000,
            thread_count: 32,
            max_serialized_operations_size_per_block: 1024,
            controller_channel_size: 1024,
            event_channel_size: 1024,
            genesis_timestamp: MassaTime::now().unwrap(),
            t0: MassaTime::from_millis(16000),
            max_operations_propagation_time: MassaTime::from_millis(30000),
            max_endorsements_propagation_time: MassaTime::from_millis(60000),
            initial_peers: NamedTempFile::new()
                .expect("cannot create temp file")
                .path()
                .to_path_buf(),
            listeners: HashMap::default(),
            thread_tester_count: 2,
            max_size_channel_commands_connectivity: 1000,
            max_size_channel_commands_retrieval_operations: 10000,
            max_size_channel_commands_propagation_operations: 10000,
            max_size_channel_commands_retrieval_blocks: 1000,
            max_size_channel_commands_propagation_blocks: 1000,
            max_size_channel_commands_propagation_endorsements: 5000,
            max_size_channel_commands_retrieval_endorsements: 5000,
            max_size_channel_network_to_block_handler: 1000,
            max_size_channel_network_to_endorsement_handler: 1000,
            max_size_channel_network_to_operation_handler: 10000,
            max_size_channel_network_to_peer_handler: 1000,
            max_size_channel_commands_peer_testers: 10000,
            max_size_channel_commands_peers: 300,
            endorsement_count: ENDORSEMENT_COUNT,
            max_size_block_infos: 200,
            max_size_value_datastore: 1_000_000,
            max_size_function_name: u16::MAX,
            max_size_call_sc_parameter: 10_000_000,
            max_denunciations_in_block_header: 100,
            max_op_datastore_entry_count: 100000,
            max_op_datastore_key_length: u8::MAX,
            max_op_datastore_value_length: 1000000,
            max_endorsements_per_message: 1000,
            max_size_listeners_per_peer: 100,
            max_size_peers_announcement: 100,
            last_start_period: 0,
            read_write_limit_bytes_per_second: 1024 * 1000,
            debug: true,
        }
    }
}
