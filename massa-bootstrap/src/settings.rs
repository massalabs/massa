// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::block::BlockDeserializerArgs;
use massa_models::node::NodeId;
use massa_time::MassaTime;
use serde::Deserialize;
use serde_json::to_value;
use std::{net::SocketAddr, path::PathBuf};

/// Bootstrap IP protocol version setting.
#[derive(Debug, Deserialize, Clone, Copy)]
pub enum IpType {
    /// Bootstrap with both IPv4 and IPv6 protocols (default).
    Both,
    /// Bootstrap only with IPv4.
    IPv4,
    /// Bootstrap only with IPv6.
    IPv6,
}

/// Bootstrap configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct BootstrapConfig {
    /// Ip address of our bootstrap nodes and their public key.
    pub bootstrap_list: Vec<(SocketAddr, NodeId)>,
    /// IP version filter for bootstrap list, targeting IpType::IPv4, IpType::IPv6 or IpType::Both. Defaults to IpType::Both.
    pub bootstrap_protocol: IpType,
    /// Path to the bootstrap whitelist file. This whitelist define IPs that can bootstrap on your node.
    pub bootstrap_whitelist_path: PathBuf,
    /// Path to the bootstrap blacklist file. This whitelist define IPs that will not be able to bootstrap on your node. This list is optional.
    pub bootstrap_blacklist_path: PathBuf,
    /// Port to listen if we choose to allow other nodes to use us as bootstrap node.
    pub bind: Option<SocketAddr>,
    /// connection timeout
    pub connect_timeout: MassaTime,
    /// Time allocated to managing the bootstrapping process,
    /// i.e. providing the ledger and consensus
    pub bootstrap_timeout: MassaTime,
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
    /// Maximum allowed time between server and client clocks
    pub max_clock_delta: MassaTime,
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
    /// max executed ops
    pub max_executed_ops_length: u64,
    /// max executed ops changes
    pub max_ops_changes_length: u64,
    /// consensus bootstrap part size
    pub consensus_bootstrap_part_size: u64,
}

///
#[derive(Debug, Deserialize, Clone)]
pub struct BootstrapClientConfig {
    pub max_bytes_read_write: f64,
    pub max_bootstrap_message_size: u32,
    pub endorsement_count: u32,
    pub max_advertise_length: u32,
    pub max_bootstrap_blocks_length: u32,
    pub max_operations_per_block: u32,
    pub thread_count: u8,
    pub randomness_size_bytes: usize,
    pub max_bootstrap_error_length: u64,
    pub max_bootstrap_final_state_parts_size: u64,
    pub max_datastore_entry_count: u64,
    pub max_datastore_key_length: u8,
    pub max_datastore_value_length: u64,
    pub max_async_pool_changes: u64,
    pub max_async_pool_length: u64,
    pub max_async_message_data: u64,
    pub max_ledger_changes_count: u64,
    pub max_changes_slot_count: u64,
    pub max_rolls_length: u64,
    pub max_production_stats_length: u64,
    pub max_credits_length: u64,
    pub max_executed_ops_length: u64,
    pub max_ops_changes_length: u64,
}

impl From<&BootstrapConfig> for BootstrapClientConfig {
    fn from(value: &BootstrapConfig) -> Self {
        Self {
            max_bytes_read_write: value.max_bytes_read_write,
            max_bootstrap_message_size: value.max_bootstrap_message_size,
            endorsement_count: value.endorsement_count,
            max_advertise_length: value.max_advertise_length,
            max_bootstrap_blocks_length: value.max_bootstrap_blocks_length,
            max_operations_per_block: value.max_operations_per_block,
            thread_count: value.thread_count,
            randomness_size_bytes: value.randomness_size_bytes,
            max_bootstrap_error_length: value.max_bootstrap_error_length,
            max_bootstrap_final_state_parts_size: value.max_bootstrap_final_state_parts_size,
            max_datastore_entry_count: value.max_datastore_entry_count,
            max_datastore_key_length: value.max_datastore_key_length,
            max_datastore_value_length: value.max_datastore_value_length,
            max_async_pool_changes: value.max_async_pool_changes,
            max_async_pool_length: value.max_async_pool_length,
            max_async_message_data: value.max_async_message_data,
            max_ledger_changes_count: value.max_ledger_changes_count,
            max_changes_slot_count: value.max_changes_slot_count,
            max_rolls_length: value.max_rolls_length,
            max_production_stats_length: value.max_production_stats_length,
            max_credits_length: value.max_credits_length,
            max_executed_ops_length: value.max_executed_ops_length,
            max_ops_changes_length: value.max_ops_changes_length,
        }
    }
}

///
pub struct BootstrapServerMessageDeserializerArgs {
    pub thread_count: u8,
    pub endorsement_count: u32,
    pub max_advertise_length: u32,
    pub max_bootstrap_blocks: u32,
    pub max_operations_per_block: u32,
    pub max_bootstrap_final_state_parts_size: u64,
    pub max_async_pool_changes: u64,
    pub max_async_pool_length: u64,
    pub max_async_message_data: u64,
    pub max_ledger_changes_count: u64,
    pub max_datastore_key_length: u8,
    pub max_datastore_value_length: u64,
    pub max_datastore_entry_count: u64,
    pub max_bootstrap_error_length: u64,
    pub max_changes_slot_count: u64,
    pub max_rolls_length: u64,
    pub max_production_stats_length: u64,
    pub max_credits_length: u64,
    pub max_executed_ops_length: u64,
    pub max_ops_changes_length: u64,
}

impl From<&BootstrapClientConfig> for BootstrapServerMessageDeserializerArgs {
    fn from(value: &BootstrapClientConfig) -> Self {
        Self {
            thread_count: value.thread_count,
            endorsement_count: value.endorsement_count,
            max_advertise_length: value.max_advertise_length,
            max_bootstrap_blocks: value.max_bootstrap_blocks_length, // FIXME: not the same name
            max_operations_per_block: value.max_operations_per_block,
            max_bootstrap_final_state_parts_size: value.max_bootstrap_final_state_parts_size,
            max_async_pool_changes: value.max_async_pool_changes,
            max_async_pool_length: value.max_async_pool_length,
            max_async_message_data: value.max_async_message_data,
            max_ledger_changes_count: value.max_ledger_changes_count,
            max_datastore_key_length: value.max_datastore_key_length,
            max_datastore_value_length: value.max_datastore_value_length,
            max_datastore_entry_count: value.max_datastore_entry_count,
            max_bootstrap_error_length: value.max_bootstrap_error_length,
            max_changes_slot_count: value.max_changes_slot_count,
            max_rolls_length: value.max_rolls_length,
            max_production_stats_length: value.max_production_stats_length,
            max_credits_length: value.max_credits_length,
            max_executed_ops_length: value.max_executed_ops_length,
            max_ops_changes_length: value.max_ops_changes_length,
        }
    }
}

impl From<&BootstrapServerMessageDeserializerArgs> for BlockDeserializerArgs {
    fn from(value: &BootstrapServerMessageDeserializerArgs) -> Self {
        Self {
            thread_count: value.thread_count,
            max_operations_per_block: value.max_operations_per_block,
            endorsement_count: value.endorsement_count,
        }
    }
}
