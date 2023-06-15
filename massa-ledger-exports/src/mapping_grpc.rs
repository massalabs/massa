// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{LedgerEntry, LedgerEntryUpdate, SetOrDelete, SetOrKeep};
use massa_proto_rs::massa::model::v1 as grpc_model;

impl From<LedgerEntry> for grpc_model::LedgerEntry {
    fn from(value: LedgerEntry) -> Self {
        grpc_model::LedgerEntry {
            balance: value.balance.to_raw(),
            bytecode: value.bytecode.0,
            entries: value
                .datastore
                .into_iter()
                .map(|(key, value)| grpc_model::BytesMapFieldEntry { key, value })
                .collect(),
        }
    }
}

impl From<LedgerEntryUpdate> for grpc_model::LedgerEntryUpdate {
    fn from(value: LedgerEntryUpdate) -> Self {
        grpc_model::LedgerEntryUpdate {
            balance: match value.balance {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepBalance {
                    r#type: grpc_model::LedgerChangeType::Set as i32,
                    balance: Some(value.to_raw()),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepBalance {
                    r#type: grpc_model::LedgerChangeType::Delete as i32,
                    balance: None,
                }),
            },
            bytecode: match value.bytecode {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepBytecode {
                    r#type: grpc_model::LedgerChangeType::Set as i32,
                    bytecode: Some(value.0),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepBytecode {
                    r#type: grpc_model::LedgerChangeType::Delete as i32,
                    bytecode: None,
                }),
            },
            datastore: value
                .datastore
                .into_iter()
                .map(|entry| match entry.1 {
                    SetOrDelete::Set(value) => grpc_model::SetOrDeleteDatastoreEntry {
                        r#type: grpc_model::LedgerChangeType::Set as i32,
                        datastore_entry: Some(grpc_model::BytesMapFieldEntry {
                            key: entry.0,
                            value,
                        }),
                    },
                    SetOrDelete::Delete => grpc_model::SetOrDeleteDatastoreEntry {
                        r#type: grpc_model::LedgerChangeType::Delete as i32,
                        datastore_entry: None,
                    },
                })
                .collect(),
        }
    }
}
