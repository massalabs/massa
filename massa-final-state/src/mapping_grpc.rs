// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::StateChanges;
use massa_async_pool::{AsyncMessage, AsyncMessageId, Change};
use massa_ledger_exports::SetUpdateOrDelete;
use massa_proto::massa::api::v1 as grpc;

impl From<StateChanges> for grpc::StateChanges {
    fn from(value: StateChanges) -> Self {
        grpc::StateChanges {
            executed_ops_changes: Some(grpc::ExecutedOpsChanges {
                executed_ops: value
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
            }),
            async_pool_changes: value
                .async_pool_changes
                .0
                .into_iter()
                .map(
                    |change: Change<AsyncMessageId, AsyncMessage>| match change {
                        Change::Add(async_msg_id, async_msg) => grpc::AsyncPoolChangeEntry {
                            async_message_id: async_msg_id_to_string(async_msg_id),
                            value: Some(grpc::AsyncPoolChangeValue {
                                r#type: grpc::AsyncPoolChangeType::Add as i32,
                                async_message: Some(async_msg.into()),
                            }),
                        },
                        Change::Activate(async_msg_id) => grpc::AsyncPoolChangeEntry {
                            async_message_id: async_msg_id_to_string(async_msg_id),
                            value: Some(grpc::AsyncPoolChangeValue {
                                r#type: grpc::AsyncPoolChangeType::Activate as i32,
                                async_message: None,
                            }),
                        },
                        Change::Delete(async_msg_id) => grpc::AsyncPoolChangeEntry {
                            async_message_id: async_msg_id_to_string(async_msg_id),
                            value: Some(grpc::AsyncPoolChangeValue {
                                r#type: grpc::AsyncPoolChangeType::Delete as i32,
                                async_message: None,
                            }),
                        },
                    },
                )
                .collect(),
            ledger_changes: Some(grpc::LedgerChanges {
                entries: value
                    .ledger_changes
                    .0
                    .into_iter()
                    .map(|(key, value)| grpc::LedgerChangeEntry {
                        address: key.to_string(),
                        value: Some(match value {
                            SetUpdateOrDelete::Set(value) => grpc::LedgerChangeValue {
                                r#type: grpc::LedgerChangeType::Set as i32,
                                created_entry: Some(value.into()),
                                updated_entry: None,
                            },
                            SetUpdateOrDelete::Update(value) => grpc::LedgerChangeValue {
                                r#type: grpc::LedgerChangeType::Update as i32,
                                created_entry: None,
                                updated_entry: Some(value.into()),
                            },
                            SetUpdateOrDelete::Delete => grpc::LedgerChangeValue {
                                r#type: grpc::LedgerChangeType::Delete as i32,
                                created_entry: None,
                                updated_entry: None,
                            },
                        }),
                    })
                    .collect(),
            }),
        }
    }
}

fn async_msg_id_to_string(id: AsyncMessageId) -> String {
    bs58::encode(format!("{}{}{}{}", id.0 .0.numer(), id.0 .0.denom(), id.1, id.2).as_bytes())
        .with_check()
        .into_string()
}
