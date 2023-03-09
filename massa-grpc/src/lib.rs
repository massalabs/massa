//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! gRPC RPC API for a massa-node
#![feature(async_closure)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

use tonic_health as _;
use tonic_reflection as _;
use tonic_web as _;

/// gRPC API implementation
pub mod api;
/// business code
pub mod business;
/// gRPC configuration
pub mod config;
/// models error
pub mod error;
/// models
pub mod models;
