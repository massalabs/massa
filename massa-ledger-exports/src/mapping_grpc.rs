// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{LedgerEntry, LedgerEntryUpdate, SetOrDelete, SetOrKeep};
use massa_proto_rs::massa::model::v1 as grpc_model;

impl From<LedgerEntry> for grpc_model::LedgerEntry {
    fn from(value: LedgerEntry) -> Self {
        grpc_model::LedgerEntry {
            balance: Some(value.balance.into()),
            bytecode: value.bytecode.0,
            datastore: value
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
                    change: Some(grpc_model::set_or_keep_balance::Change::Set(value.into())),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepBalance {
                    change: Some(grpc_model::set_or_keep_balance::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            bytecode: match value.bytecode {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepBytes {
                    change: Some(grpc_model::set_or_keep_bytes::Change::Set(value.0)),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepBytes {
                    change: Some(grpc_model::set_or_keep_bytes::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            datastore: value
                .datastore
                .into_iter()
                .map(|entry| match entry.1 {
                    SetOrDelete::Set(value) => grpc_model::SetOrDeleteDatastoreEntry {
                        change: Some(grpc_model::set_or_delete_datastore_entry::Change::Set(
                            grpc_model::BytesMapFieldEntry {
                                key: entry.0,
                                value,
                            },
                        )),
                    },
                    SetOrDelete::Delete => grpc_model::SetOrDeleteDatastoreEntry {
                        change: Some(grpc_model::set_or_delete_datastore_entry::Change::Delete(
                            grpc_model::Empty {},
                        )),
                    },
                })
                .collect(),
        }
    }
}
