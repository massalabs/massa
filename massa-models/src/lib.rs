//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! All the structures that are used everywhere
//!
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(bound_map)]
#![feature(int_roundings)]
#![feature(iter_intersperse)]

extern crate lazy_static;

/// active blocks related structures
pub mod active_block;
/// address related structures
pub mod address;
/// amount related structures
pub mod amount;
/// structure use by the API
pub mod api;
/// block-related structures
pub mod block;
/// clique
pub mod clique;
/// various structures
pub mod composite;
/// node configuration
pub mod config;
/// datastore serialization / deserialization
pub mod datastore;
/// endorsements
pub mod endorsement;
/// models error
pub mod error;
/// execution related structures
pub mod execution;
/// ledger related structures
pub mod ledger_models;
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
/// trait for signed structure
pub mod wrapped;

/// Test utils
#[cfg(feature = "testing")]
pub mod test_exports;
