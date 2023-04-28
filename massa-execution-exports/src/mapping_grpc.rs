// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{ExecutionOutput, SlotExecutionOutput};
use massa_proto::massa::api::v1 as grpc;

impl From<SlotExecutionOutput> for grpc::SlotExecutionOutput {
    fn from(value: SlotExecutionOutput) -> Self {
        match value {
            SlotExecutionOutput::ExecutedSlot(execution_output) => grpc::SlotExecutionOutput {
                message: Some(grpc::slot_execution_output::Message::ExecutionOutput(
                    execution_output.into(),
                )),
            },
            SlotExecutionOutput::FinalizedSlot(finalized_slot) => grpc::SlotExecutionOutput {
                message: Some(grpc::slot_execution_output::Message::FinalExecutionOutput(
                    grpc::FinalizedExecutionOutput {
                        slot: Some(finalized_slot.into()),
                    },
                )),
            },
        }
    }
}

impl From<ExecutionOutput> for grpc::ExecutionOutput {
    fn from(value: ExecutionOutput) -> Self {
        grpc::ExecutionOutput {
            slot: Some(value.slot.into()),
            block_id: value.block_id.map(|id| id.to_string()).unwrap_or_default(),
            events: value
                .events
                .0
                .into_iter()
                .map(|event| event.into())
                .collect(),
        }
    }
}
