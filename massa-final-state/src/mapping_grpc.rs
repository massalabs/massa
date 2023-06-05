// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::StateChanges;
use massa_async_pool::AsyncMessageId;
use massa_ledger_exports::SetUpdateOrDelete;
use massa_proto::massa::api::v1 as grpc;

impl From<StateChanges> for grpc::StateChanges {
    fn from(value: StateChanges) -> Self {
        grpc::StateChanges {
            executed_ops_changes: value
                .executed_ops_changes
                .into_iter()
                .map(|(op_id, (op_exec_status, op_valid_until_slot))| {
                    grpc::ExecutedOpsChangeEntry {
                        operation_id: op_id.to_string(),
                        value: Some(grpc::ExecutedOpsChangeValue {
                            status: if op_exec_status {
                                vec![grpc::OperationExecutionStatus::Success as i32]
                            } else {
                                vec![grpc::OperationExecutionStatus::Failed as i32]
                            },
                            slot: Some(op_valid_until_slot.into()),
                        }),
                    }
                })
                .collect(),
            async_pool_changes: value
                .async_pool_changes
                .0
                .into_iter()
                .map(|(async_msg_id, change)| match change {
                    SetUpdateOrDelete::Set(async_msg) => grpc::AsyncPoolChangeEntry {
                        async_message_id: async_msg_id_to_string(async_msg_id),
                        value: Some(grpc::AsyncPoolChangeValue {
                            r#type: grpc::AsyncPoolChangeType::Set as i32,
                            message: Some(grpc::async_pool_change_value::Message::CreatedMessage(
                                async_msg.into(),
                            )),
                        }),
                    },
                    SetUpdateOrDelete::Update(async_msg_update) => grpc::AsyncPoolChangeEntry {
                        async_message_id: async_msg_id_to_string(async_msg_id),
                        value: Some(grpc::AsyncPoolChangeValue {
                            r#type: grpc::AsyncPoolChangeType::Update as i32,
                            message: Some(grpc::async_pool_change_value::Message::UpdatedMessage(
                                async_msg_update.into(),
                            )),
                        }),
                    },
                    SetUpdateOrDelete::Delete => grpc::AsyncPoolChangeEntry {
                        async_message_id: async_msg_id_to_string(async_msg_id),
                        value: Some(grpc::AsyncPoolChangeValue {
                            r#type: grpc::AsyncPoolChangeType::Delete as i32,
                            message: None,
                        }),
                    },
                })
                .collect(),
            ledger_changes: value
                .ledger_changes
                .0
                .into_iter()
                .map(|(key, value)| grpc::LedgerChangeEntry {
                    address: key.to_string(),
                    value: Some(match value {
                        SetUpdateOrDelete::Set(value) => grpc::LedgerChangeValue {
                            r#type: grpc::LedgerChangeType::Set as i32,
                            entry: Some(grpc::ledger_change_value::Entry::CreatedEntry(
                                value.into(),
                            )),
                        },
                        SetUpdateOrDelete::Update(value) => grpc::LedgerChangeValue {
                            r#type: grpc::LedgerChangeType::Update as i32,
                            entry: Some(grpc::ledger_change_value::Entry::UpdatedEntry(
                                value.into(),
                            )),
                        },
                        SetUpdateOrDelete::Delete => grpc::LedgerChangeValue {
                            r#type: grpc::LedgerChangeType::Delete as i32,
                            entry: None,
                        },
                    }),
                })
                .collect(),
            executed_denunciations_changes: value
                .executed_denunciations_changes
                .into_iter()
                .map(|de_idx| de_idx.into())
                .collect(),
        }
    }
}

fn async_msg_id_to_string(id: AsyncMessageId) -> String {
    bs58::encode(format!("{}{}{}{}", id.0 .0.numer(), id.0 .0.denom(), id.1, id.2).as_bytes())
        .with_check()
        .into_string()
}
