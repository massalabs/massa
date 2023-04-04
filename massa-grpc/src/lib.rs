// Copyright (c) 2023 MASSA LABS <info@massa.net>
//
//! ## **Overview**
//!
//! This Rust module is a gRPC API for providing services for the Massa blockchain.
//! It implements gRPC services defined in the [massa_proto] module.
//!
//! ## **Structure**
//!
//! * `api.rs`: implements gRPC service methods without streams.
//! * `handler.rs`: defines the logic for handling incoming gRPC requests.
//! * `server`: initializes the gRPC service and serve It.
//! * `stream/`: contains the gRPC streaming methods implementations files.

#![feature(async_closure)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

use tonic_health as _;
use tonic_reflection as _;
use tonic_web as _;

/// business code for non stream methods
pub mod api;
/// gRPC configuration
pub mod config;
/// models error
pub mod error;
/// gRPC API implementation
pub mod handler;
/// gRPC service initialization and serve
pub mod server;
/// business code for stream methods
pub mod stream;

#[cfg(test)]
mod tests;
