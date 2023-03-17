//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! gRPC RPC API for a massa-node
#![feature(async_closure)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

use tonic_health as _;
use tonic_reflection as _;
use tonic_web as _;

/// business code
pub mod api;
/// gRPC configuration
pub mod config;
/// models error
pub mod error;
/// gRPC API implementation
pub mod handler;
/// service
pub mod service;
/// stream
pub mod stream;

#[cfg(test)]
mod tests;
