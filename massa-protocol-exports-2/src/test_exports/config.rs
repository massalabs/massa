use std::collections::HashMap;

use crate::ProtocolConfig;
use massa_time::MassaTime;
use peernet::types::KeyPair;
use tempfile::NamedTempFile;

impl Default for ProtocolConfig {
    fn default() -> Self {
        ProtocolConfig {
            max_in_connections: 10,
            max_out_connections: 10,
            keypair: KeyPair::generate(),
            ask_block_timeout: 500.into(),
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
            debug: true,
        }
    }
}
