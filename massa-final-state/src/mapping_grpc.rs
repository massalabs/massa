// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::StateChanges;
use massa_async_pool::{AsyncMessage, AsyncMessageId, Change};
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
                        Change::Add(async_msg_id, async_msg) => grpc::AsynPoolChange {
                            async_pool_change: Some(grpc::AsyncPoolChangeEntry {
                                async_message_id: async_msg_id_to_string(async_msg_id),
                                value: Some(grpc::AsyncPoolChangeValue {
                                    r#type: grpc::AsyncPoolChangeType::Add as i32,
                                    async_message: Some(async_msg.into()),
                                }),
                            }),
                        },
                        Change::Activate(async_msg_id) => grpc::AsynPoolChange {
                            async_pool_change: Some(grpc::AsyncPoolChangeEntry {
                                async_message_id: async_msg_id_to_string(async_msg_id),
                                value: Some(grpc::AsyncPoolChangeValue {
                                    r#type: grpc::AsyncPoolChangeType::Activate as i32,
                                    async_message: None,
                                }),
                            }),
                        },
                        Change::Delete(async_msg_id) => grpc::AsynPoolChange {
                            async_pool_change: Some(grpc::AsyncPoolChangeEntry {
                                async_message_id: async_msg_id_to_string(async_msg_id),
                                value: Some(grpc::AsyncPoolChangeValue {
                                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                                    async_message: None,
                                }),
                            }),
                        },
                    },
                )
                .collect(),
        }
    }
}

fn async_msg_id_to_string(id: AsyncMessageId) -> String {
    bs58::encode(format!("{}{}{}{}", id.0 .0.numer(), id.0 .0.denom(), id.1, id.2).as_bytes())
        .with_check()
        .into_string()
}
