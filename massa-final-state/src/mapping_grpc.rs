// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::StateChanges;
use massa_async_pool::AsyncMessageId;
use massa_ledger_exports::SetUpdateOrDelete;
use massa_proto_rs::massa::model::v1 as grpc_model;

impl From<StateChanges> for grpc_model::StateChanges {
    fn from(value: StateChanges) -> Self {
        grpc_model::StateChanges {
            executed_ops_changes: value
                .executed_ops_changes
                .into_iter()
                .map(|(op_id, (op_exec_status, op_valid_until_slot))| {
                    grpc_model::ExecutedOpsChangeEntry {
                        operation_id: op_id.to_string(),
                        value: Some(grpc_model::ExecutedOpsChangeValue {
                            status: if op_exec_status {
                                vec![grpc_model::OperationExecutionStatus::Success as i32]
                            } else {
                                vec![grpc_model::OperationExecutionStatus::Failed as i32]
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
                    SetUpdateOrDelete::Set(async_msg) => grpc_model::AsyncPoolChangeEntry {
                        async_message_id: async_msg_id_to_string(async_msg_id),
                        value: Some(grpc_model::AsyncPoolChangeValue {
                            r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                            message: Some(
                                grpc_model::async_pool_change_value::Message::CreatedMessage(
                                    async_msg.into(),
                                ),
                            ),
                        }),
                    },
                    SetUpdateOrDelete::Update(async_msg_update) => {
                        grpc_model::AsyncPoolChangeEntry {
                            async_message_id: async_msg_id_to_string(async_msg_id),
                            value: Some(grpc_model::AsyncPoolChangeValue {
                                r#type: grpc_model::AsyncPoolChangeType::Update as i32,
                                message: Some(
                                    grpc_model::async_pool_change_value::Message::UpdatedMessage(
                                        async_msg_update.into(),
                                    ),
                                ),
                            }),
                        }
                    }
                    SetUpdateOrDelete::Delete => grpc_model::AsyncPoolChangeEntry {
                        async_message_id: async_msg_id_to_string(async_msg_id),
                        value: Some(grpc_model::AsyncPoolChangeValue {
                            r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                            message: None,
                        }),
                    },
                })
                .collect(),
            ledger_changes: value
                .ledger_changes
                .0
                .into_iter()
                .map(|(key, value)| grpc_model::LedgerChangeEntry {
                    address: key.to_string(),
                    value: Some(match value {
                        SetUpdateOrDelete::Set(value) => grpc_model::LedgerChangeValue {
                            r#type: grpc_model::LedgerChangeType::Set as i32,
                            entry: Some(grpc_model::ledger_change_value::Entry::CreatedEntry(
                                value.into(),
                            )),
                        },
                        SetUpdateOrDelete::Update(value) => grpc_model::LedgerChangeValue {
                            r#type: grpc_model::LedgerChangeType::Update as i32,
                            entry: Some(grpc_model::ledger_change_value::Entry::UpdatedEntry(
                                value.into(),
                            )),
                        },
                        SetUpdateOrDelete::Delete => grpc_model::LedgerChangeValue {
                            r#type: grpc_model::LedgerChangeType::Delete as i32,
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
