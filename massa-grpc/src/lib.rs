// Copyright (c) 2023 MASSA LABS <info@massa.net>
//
//! ## **Overview**
//!
//! This Rust module is a gRPC API for providing services for the Massa blockchain.
//! It implements gRPC services defined in the [massa_proto_rs] crate.
//!
//! ## **Structure**
//!
//! * `api.rs`: implements gRPC service methods without streams.
//! * `handler.rs`: defines the logic for handling incoming gRPC requests.
//! * `server`: initializes the gRPC service and serve It.
//! * `stream/`: contains the gRPC streaming methods implementations files.

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

use massa_models::slot::Slot;
use massa_proto_rs::massa::model::v1 as grpc_model;
use std::hash::Hash;
use tonic_health as _;
use tonic_reflection as _;
use tonic_web as _;

/// gRPC configuration
pub mod config;
/// models error
pub mod error;
/// gRPC API implementation
pub mod handler;
/// business code for node management methods
pub mod private;
/// business code for non stream methods
pub mod public;
/// gRPC service initialization and serve
pub mod server;
/// business code for stream methods
pub mod stream;

#[cfg(test)]
mod tests;

/// Slot range type
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct SlotRange {
    // Start lot
    start_slot: Option<Slot>,
    // End slot
    end_slot: Option<Slot>,
}

// Slot draw
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
struct SlotDraw {
    /// Slot
    slot: Option<Slot>,
    /// Block producer address (Optional)
    block_producer: Option<String>,
    /// Endorsement draws
    endorsement_draws: Vec<EndorsementDraw>,
}

// Endorsement draw
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
struct EndorsementDraw {
    /// Endorsement index
    index: u64,
    /// Producer address
    producer: String,
}

impl From<SlotDraw> for grpc_model::SlotDraw {
    fn from(value: SlotDraw) -> Self {
        grpc_model::SlotDraw {
            slot: value.slot.map(Into::into),
            block_producer: value.block_producer,
            endorsement_draws: value
                .endorsement_draws
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

impl From<EndorsementDraw> for grpc_model::EndorsementDraw {
    fn from(value: EndorsementDraw) -> Self {
        grpc_model::EndorsementDraw {
            index: value.index,
            producer: value.producer,
        }
    }
}
