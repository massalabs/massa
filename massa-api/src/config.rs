// Copyright (c) 2022 MASSA LABS <info@massa.net>

use jsonrpc_core::serde::Deserialize;
use std::net::SocketAddr;

/// API settings.
/// the API settings
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct APIConfig {
    /// when looking for next draw we want to look at max `draw_lookahead_period_count`
    pub draw_lookahead_period_count: u64,
    /// bind for the private API
    pub bind_private: SocketAddr,
    /// bind for the public API
    pub bind_public: SocketAddr,
    /// max argument count
    pub max_arguments: u64,
    /// max datastore value length
    pub max_datastore_value_length: u64,
    /// max op datastore entry
    pub max_op_datastore_entry_count: u64,
    /// max datastore key length
    pub max_op_datastore_key_length: u8,
    /// max datastore value length
    pub max_op_datastore_value_length: u64,
    /// max function name length
    pub max_function_name_length: u16,
    /// max parameter size
    pub max_parameter_size: u32,
}
