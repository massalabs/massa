// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{ExecutionOutput, SlotExecutionOutput};
use massa_proto::massa::api::v1 as grpc;

impl From<SlotExecutionOutput> for grpc::SlotExecutionOutput {
    fn from(value: SlotExecutionOutput) -> Self {
        match value {
            SlotExecutionOutput::ExecutedSlot(execution_output) => grpc::SlotExecutionOutput {
                status: vec![grpc::ExecutionOutputStatus::Candidate as i32],
                execution_output: Some(execution_output.into()),
            },
            SlotExecutionOutput::FinalizedSlot(execution_output) => grpc::SlotExecutionOutput {
                status: vec![grpc::ExecutionOutputStatus::Final as i32],
                execution_output: Some(execution_output.into()),
            },
        }
    }
}

impl From<ExecutionOutput> for grpc::ExecutionOutput {
    fn from(value: ExecutionOutput) -> Self {
        grpc::ExecutionOutput {
            slot: Some(value.slot.into()),
            block_id: value.block_id.map(|id| id.to_string()),
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
