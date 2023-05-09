//! # General description
//!
//! TODO

#![feature(let_chains)]

mod config;
mod controller;
mod error;
mod key;
mod ledger_changes;
mod ledger_entry;
mod types;

pub use config::LedgerConfig;
pub use controller::LedgerController;
pub use error::LedgerError;
pub use key::datastore_prefix_from_address;
pub use key::Key;
pub use key::KeyDeserializer;
pub use key::KeySerializer;
pub use key::KeyType;
pub(crate) use key::{BALANCE_IDENT, BYTECODE_IDENT, DATASTORE_IDENT};
pub use ledger_changes::LedgerChanges;

pub(crate) use ledger_changes::DatastoreUpdateDeserializer;
pub(crate) use ledger_changes::DatastoreUpdateSerializer;
pub use ledger_changes::LedgerChangesDeserializer;
pub use ledger_changes::LedgerChangesSerializer;
pub use ledger_changes::LedgerEntryUpdate;
pub(crate) use ledger_changes::LedgerEntryUpdateDeserializer;
pub(crate) use ledger_changes::LedgerEntryUpdateSerializer;

pub use ledger_entry::LedgerEntry;
pub(crate) use ledger_entry::{LedgerEntryDeserializer, LedgerEntrySerializer};
pub use types::Applicable;
pub use types::SetOrDelete;
pub use types::SetOrKeep;
pub use types::SetUpdateOrDelete;

#[cfg(feature = "testing")]
pub(crate) mod test_exports;
