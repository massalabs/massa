use std::collections::HashMap;

use crate::{settings::PeerCategoryInfo, ProtocolConfig};
use massa_models::config::{ENDORSEMENT_COUNT, MAX_MESSAGE_SIZE};
use massa_time::MassaTime;
use tempfile::NamedTempFile;

impl Default for ProtocolConfig {
    fn default() -> Self {
        ProtocolConfig {
            keypair_file: NamedTempFile::new()
                .expect("cannot create temp file")
                .path()
                .to_path_buf(),
            ask_block_timeout: MassaTime::from_millis(500),
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
            operation_batch_proc_period: MassaTime::from_millis(200),
            asked_operations_buffer_capacity: 10000,
            operation_announcement_interval: MassaTime::from_millis(150),
            max_operations_per_message: 1024,
            max_operations_per_block: 5000,
            thread_count: 32,
            max_serialized_operations_size_per_block: 1024,
            controller_channel_size: 1024,
            event_channel_size: 1024,
            genesis_timestamp: MassaTime::now().unwrap(),
            t0: MassaTime::from_millis(16000),
            max_ops_kept_for_propagation: 10000,
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
            max_message_size: MAX_MESSAGE_SIZE as usize,
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
            message_timeout: MassaTime::from_millis(10000),
            tester_timeout: MassaTime::from_millis(500),
            last_start_period: 0,
            read_write_limit_bytes_per_second: 1024 * 1000,
            timeout_connection: MassaTime::from_millis(1000),
            try_connection_timer: MassaTime::from_millis(5000),
            routable_ip: None,
            max_in_connections: 10,
            debug: true,
            peers_categories: HashMap::default(),
            default_category_info: PeerCategoryInfo {
                allow_local_peers: true,
                max_in_connections: 10,
                target_out_connections: 10,
                max_in_connections_per_ip: 0,
            },
            version: "TEST.23.2".parse().unwrap(),
            try_connection_timer_same_peer: MassaTime::from_millis(1000),
        }
    }
}
