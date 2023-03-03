// Copyright (c) 2022 MASSA LABS <info@massa.net>

use serde::Deserialize;
use std::net::SocketAddr;

/// gRPC configuration.
/// the gRPC configuration
#[derive(Debug, Deserialize, Clone)]
pub struct GrpcConfig {
    /// whether to enable gRPC
    pub enabled: bool,
    /// whether to accept HTTP/1.1 requests.
    pub accept_http1: bool,
    /// whether to enable gRPC reflection
    pub enable_reflection: bool,
    /// bind for the Massa gRPC API
    pub bind: SocketAddr,
    /// which compression encodings does the server accept for requests
    pub accept_compressed: Option<String>,
    /// which compression encodings might the server use for responses
    pub send_compressed: Option<String>,
    /// limits the maximum size of a decoded message. Defaults to 4MB
    pub max_decoding_message_size: usize,
    /// limits the maximum size of an encoded message. Defaults to 4MB
    pub max_encoding_message_size: usize,
    /// thread count
    pub thread_count: u8,
    /// max operations per block
    pub max_operations_per_block: u32,
    /// endorsement count
    pub endorsement_count: u32,
    /// max endorsements per message
    pub max_endorsements_per_message: u32,
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
    /// max operations per message in the network to avoid sending to big data packet
    pub max_operations_per_message: u32,
    /// limits the maximum size of streaming channel
    pub max_channel_size: usize,
}
