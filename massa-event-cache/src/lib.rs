// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # General description
//!
//! A database (build on top of rocksdb) to store final events emitted by:
//! * The massa node during execution
//! * A Smart contract during execution
//!
//! The database can also be queried with: get_filtered_sc_output_events
//!
//! # Architecture
//!
//! * controller.rs / worker.rs: db wrapper controller & worker
//! * rocksdb_operator: a rocksdb operator to handle counter (as u64 counter)
//! * ser_deser.rs: event serializer && deserializer (using nom)
//! * event_cache: db over rocksdb code
//!
//! # DBKeyBuilder
//!
//! The db use the DbKeyBuilder structure to create the rocksdb keys required for its operations.
//! There are 3 types of keys:
//! * regular key: and the associated value is the event data serialized
//! * prefix key: Key used to iterate in rocksdb
//! * counter key: key used to query count info (example: number of event for operation id X)
//!
//! See `key_from_event` method for more information about the keys.
//!
//! # Db queries
//!
//! WHen inserting an event in the DB, counters are being updated. They are used later when we need
//! to query the DB in order to query by ordering with counter values (to optimize the DB query plan).
//!
//! Let say you want to query events for a specific slot range && an emitter_address, the query method
//! will first query the counts for each filter item then select the one with fewer rows then select the
//! other.
//!
//! # Snip
//!
//! The method snip on the DB must be used to remove items and keep a reasonable DB size. It will remove
//! the oldest (by slot) items (number is configurable) in the DB.
//!
//! # Tests
//!
//! * Check unit tests for info on how to use the DB directly.

pub mod config;

pub mod controller;
mod event_cache;
mod rocksdb_operator;
mod ser_deser;
pub mod worker;

#[cfg(feature = "test-exports")]
pub use controller::{MockEventCacheController, MockEventCacheControllerWrapper};
