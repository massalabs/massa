// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::StateChanges;
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
        }
    }
}
