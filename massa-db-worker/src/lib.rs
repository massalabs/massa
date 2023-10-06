// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # General description
//!
//! MassaDB is a wrapper around:
//! * A RocksDB database (on Disk)
//! * Some caches (on RAM).
//! * a config
//!
//! # RocksDB
//!
//! RocksDB stores keys and values, which are arbitrarily-sized byte streams (aka vec<u8> or &[u8]).
//! It supports both point lookups and range scans.
//!
//! For MassaDB, we use 3 rocksdb column:
//! * state: all data used to compute the db hash (or final state hash)
//! * versioning: partial MIP store data see Versioning doc section: "MipStore and Final state hash"
//! * metadata: final state hash + slot
//!
//! Note that data is stored with a prefix (see constants.rs in massa-db-exports).
//! For instance, a ledger update, will be stored (in column: 'state') as:
//! * key: LEDGER_PREFIX+Address+[...] (serialized as bytes)
//! * value: Amount (serialized as bytes)
//!
//! # DB hash
//!
//! Whenever something is stored on the column 'state', the db hash is updating using Xor
//! (For more detail: HashXof).
//! This hash is often referred as 'final state hash'.
//!
//! # Caches
//!
//! A cache of db changes is kept in memory allowing to easily stream it
//! (by streaming, we means: sending it to another massa node (aka bootstrap))
//! There is 2 separate caches: one for 'state' and one for 'versioning'
//!
//! These caches is stored as a key, value: slot -> insertion_data|deletion_data.
//!
//! # Streaming steps
//!
//! 1- Streaming always starts by reading data from disk (from rocksdb). Data is send by chunk.
//! 2- As this process can be slow (e.g. db size is high, network is slow), updates can happen simultaneously
//!    (as new slots are processed by the node, data can be updated)
//!    thus the (DB) caches are updated so as the streaming process is ongoing, we can easily only send
//!    the updates (by querying only the cache)
//! 3- Even after this process is finished (and as other things like consensus data are streamed),
//!    we can send the updates

mod massa_db;

pub use crate::massa_db::*;
