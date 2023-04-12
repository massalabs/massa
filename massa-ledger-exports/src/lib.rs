//! # General description
//!
//! TODO

#![feature(let_chains)]

mod config;
mod controller;
mod error;
mod key;
mod ledger_batch;
mod ledger_changes;
mod ledger_entry;
mod types;

pub use config::LedgerConfig;
pub use controller::LedgerController;
pub use error::LedgerError;
pub use key::{
    datastore_prefix_from_address, Key, KeyDeserializer, KeySerializer, KeyType, BALANCE_IDENT,
    BYTECODE_IDENT, DATASTORE_IDENT,
};
pub use ledger_batch::LedgerBatch;
pub use ledger_changes::{
    DatastoreUpdateDeserializer, DatastoreUpdateSerializer, LedgerChanges,
    LedgerChangesDeserializer, LedgerChangesSerializer, LedgerEntryUpdate,
    LedgerEntryUpdateDeserializer, LedgerEntryUpdateSerializer,
};
pub use ledger_entry::{LedgerEntry, LedgerEntryDeserializer, LedgerEntrySerializer};
pub use types::{Applicable, SetOrDelete, SetOrKeep, SetUpdateOrDelete};

#[cfg(feature = "testing")]
pub mod test_exports;
