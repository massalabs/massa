// Copyright (c) 2022 MASSA LABS <info@massa.net>

use serde::Deserialize;
use std::net::SocketAddr;

/// gRPC configuration.
/// the gRPC configuration
#[derive(Debug, Deserialize, Clone)]
pub struct GrpcConfig {
    /// whether to enable gRPC.
    pub enabled: bool,
    /// whether to accept HTTP/1.1 requests.
    pub accept_http1: bool,
    /// whether to enable gRPC reflection(introspection).
    pub enable_reflection: bool,
    /// bind for the Massa gRPC API
    pub bind: SocketAddr,
    /// which compression encodings does the server accept for requests.
    pub accept_compressed: Option<String>,
    /// which compression encodings might the server use for responses.
    pub send_compressed: Option<String>,
    /// limits the maximum size of a decoded message. Defaults to 4MB.
    pub max_decoding_message_size: usize,
    /// limits the maximum size of an encoded message. Defaults to 4MB.
    pub max_encoding_message_size: usize,
}
