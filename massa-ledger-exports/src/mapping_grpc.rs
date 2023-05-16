// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{LedgerEntry, LedgerEntryUpdate, SetOrDelete, SetOrKeep};
use massa_proto::massa::api::v1 as grpc;

impl From<LedgerEntry> for grpc::LedgerEntry {
    fn from(value: LedgerEntry) -> Self {
        grpc::LedgerEntry {
            balance: value.balance.to_raw(),
            bytecode: value.bytecode.0,
            entries: value
                .datastore
                .into_iter()
                .map(|(k, v)| grpc::DatastoreEntry {
                    final_value: k,
                    candidate_value: v,
                })
                .collect(),
        }
    }
}

impl From<LedgerEntryUpdate> for grpc::LedgerEntryUpdate {
    fn from(value: LedgerEntryUpdate) -> Self {
        grpc::LedgerEntryUpdate {
            balance: match value.balance {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepBalance {
                    r#type: grpc::LedgerChangeType::Set as i32,
                    balance: Some(value.to_raw()),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepBalance {
                    r#type: grpc::LedgerChangeType::Delete as i32,
                    balance: None,
                }),
            },
            bytecode: match value.bytecode {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepBytecode {
                    r#type: grpc::LedgerChangeType::Set as i32,
                    bytecode: Some(value.0),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepBytecode {
                    r#type: grpc::LedgerChangeType::Delete as i32,
                    bytecode: None,
                }),
            },
            datastore: value
                .datastore
                .into_iter()
                .map(|entry| match entry.1 {
                    SetOrDelete::Set(value) => grpc::SetOrDeleteDatastoreEntry {
                        r#type: grpc::LedgerChangeType::Set as i32,
                        datastore_entry: Some(grpc::DatastoreEntry {
                            final_value: entry.0,
                            candidate_value: value,
                        }),
                    },
                    SetOrDelete::Delete => grpc::SetOrDeleteDatastoreEntry {
                        r#type: grpc::LedgerChangeType::Delete as i32,
                        datastore_entry: None,
                    },
                })
                .collect(),
        }
    }
}
