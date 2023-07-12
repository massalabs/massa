// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{ExecutionOutput, ExecutionQueryError, SlotExecutionOutput};
use massa_proto_rs::massa::model::v1 as grpc_model;

impl From<SlotExecutionOutput> for grpc_model::SlotExecutionOutput {
    fn from(value: SlotExecutionOutput) -> Self {
        match value {
            SlotExecutionOutput::ExecutedSlot(execution_output) => {
                grpc_model::SlotExecutionOutput {
                    status: grpc_model::ExecutionOutputStatus::Candidate as i32,
                    execution_output: Some(execution_output.into()),
                }
            }
            SlotExecutionOutput::FinalizedSlot(execution_output) => {
                grpc_model::SlotExecutionOutput {
                    status: grpc_model::ExecutionOutputStatus::Final as i32,
                    execution_output: Some(execution_output.into()),
                }
            }
        }
    }
}

impl From<ExecutionOutput> for grpc_model::ExecutionOutput {
    fn from(value: ExecutionOutput) -> Self {
        grpc_model::ExecutionOutput {
            slot: Some(value.slot.into()),
            block_id: value.block_info.map(|i| i.block_id.to_string()),
            events: value
                .events
                .0
                .into_iter()
                .map(|event| event.into())
                .collect(),
            state_changes: Some(value.state_changes.into()),
        }
    }
}

impl From<ExecutionQueryError> for grpc_model::Error {
    fn from(value: ExecutionQueryError) -> Self {
        match value {
            ExecutionQueryError::NotFound(error) => grpc_model::Error {
                //TODO to be defined
                code: 404,
                message: error,
            },
        }
    }
}
