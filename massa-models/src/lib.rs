//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! All the structures that are used everywhere
//!
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

extern crate lazy_static;

/// active blocks related structures
pub mod active_block;
/// address related structures
pub mod address;
/// amount related structures
pub mod amount;
/// block structure
pub mod block;
/// block-related structure: block_header
pub mod block_header;
/// block-related structure: block_id
pub mod block_id;
/// bytecode structures
pub mod bytecode;
/// clique
pub mod clique;
/// various structures
pub mod composite;
/// node configuration
pub mod config;
/// datastore serialization / deserialization
pub mod datastore;
/// deferred call id
pub mod deferred_call_id;
/// denunciation
pub mod denunciation;
/// endorsements
pub mod endorsement;
/// models error
pub mod error;
/// execution related structures
pub mod execution;
/// ledger related structures
pub mod ledger;
/// mapping grpc
pub mod mapping_grpc;
/// node related structure
pub mod node;
/// operations
pub mod operation;
/// smart contract output events
pub mod output_event;
/// pre-hashed trait, for hash less hashmap/set
pub mod prehash;
/// rolls
pub mod rolls;
/// trait for [massa_signature::Signature] secured data-structs
pub mod secure_share;
/// serialization
pub mod serialization;
/// slots
pub mod slot;
/// various statistics
pub mod stats;
/// bootstrap streaming cursor
pub mod streaming_step;
/// management of the relation between time and slots
pub mod timeslots;
/// versions
pub mod version;

/// Test utils
#[cfg(feature = "test-exports")]
pub mod test_exports;
