// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_signature::PublicKey;
use massa_time::MassaTime;
use serde::Deserialize;
use std::net::SocketAddr;

/// Bootstrap configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct BootstrapConfig {
    /// Ip address of our bootstrap nodes and their public key.
    pub bootstrap_list: Vec<(SocketAddr, PublicKey)>,
    /// Path to the bootstrap whitelist file. This whitelist define IPs that can bootstrap on your node, even if there is no slots.
    pub bootstrap_whitelist_file: std::path::PathBuf,
    /// Port to listen if we choose to allow other nodes to use us as bootstrap node.
    pub bind: Option<SocketAddr>,
    /// connection timeout
    pub connect_timeout: MassaTime,
    /// readout timeout
    pub read_timeout: MassaTime,
    /// write timeout
    pub write_timeout: MassaTime,
    /// readout error timeout
    pub read_error_timeout: MassaTime,
    /// write error timeout
    pub write_error_timeout: MassaTime,
    /// Time we wait before retrying a bootstrap
    pub retry_delay: MassaTime,
    /// Max ping delay.
    pub max_ping: MassaTime,
    /// Enable clock synchronization
    pub enable_clock_synchronization: bool,
    /// Cache duration
    pub cache_duration: MassaTime,
    /// Max simultaneous bootstraps
    pub max_simultaneous_bootstraps: u32,
    /// Minimum interval between two bootstrap attempts from a given IP
    pub per_ip_min_interval: MassaTime,
    /// Max size of the IP list
    pub ip_list_max_size: usize,
    /// Read-Write limitation for a connection in bytes per seconds
    pub max_bytes_read_write: f64,
    /// max bootstrap message size in bytes
    pub max_bootstrap_message_size: u32,
    /// thread count
    pub thread_count: u8,
    /// period per cycle
    pub periods_per_cycle: u64,
    /// max datastore key length
    pub max_datastore_key_length: u8,
    /// randomness size bytes
    pub randomness_size_bytes: usize,
    /// endorsement count
    pub endorsement_count: u32,
    /// max advertise length
    pub max_advertise_length: u32,
    /// max bootstrap blocks length
    pub max_bootstrap_blocks_length: u32,
    /// max operations per block
    pub max_operations_per_block: u32,
    /// max bootstrap error length
    pub max_bootstrap_error_length: u64,
    /// max bootstrap final state parts size
    pub max_bootstrap_final_state_parts_size: u64,
    /// max datastore entry count
    pub max_datastore_entry_count: u64,
    /// max datastore value length
    pub max_datastore_value_length: u64,
    /// max op datastore entry count
    pub max_op_datastore_entry_count: u64,
    /// max op datastore key length
    pub max_op_datastore_key_length: u8,
    /// max op datastore value length
    pub max_op_datastore_value_length: u64,
    /// max async pool changes
    pub max_async_pool_changes: u64,
    /// max async pool length
    pub max_async_pool_length: u64,
    /// max data async message
    pub max_async_message_data: u64,
    /// max function name length
    pub max_function_name_length: u16,
    /// max parameters size
    pub max_parameters_size: u32,
    /// max ledger changes
    pub max_ledger_changes_count: u64,
    /// max slot count in state changes
    pub max_changes_slot_count: u64,
    /// max rolls in proof-of-stake and state changes
    pub max_rolls_length: u64,
    /// max production stats in proof-of-stake and state changes
    pub max_production_stats_length: u64,
    /// max credits in proof-of-stake and state changes
    pub max_credits_length: u64,
}
