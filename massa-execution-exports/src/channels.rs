// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::types::SlotExecutionOutput;

#[cfg(feature = "execution-trace")]
use crate::types_trace_info::SlotAbiCallStack;

#[cfg(feature = "execution-info")]
use crate::execution_info::ExecutionInfoForSlot;

/// channels used by the execution worker
#[derive(Clone)]
pub struct ExecutionChannels {
    /// Broadcast channel for new slot execution outputs
    pub slot_execution_output_sender: tokio::sync::broadcast::Sender<SlotExecutionOutput>,
    /// Broadcast channel for execution traces (abi call stacks, boolean true if the slot is finalized, false otherwise)
    #[cfg(feature = "execution-trace")]
    pub slot_execution_traces_sender: tokio::sync::broadcast::Sender<(SlotAbiCallStack, bool)>,
    /// Broadcast channel for execution info
    #[cfg(feature = "execution-info")]
    pub slot_execution_info_sender: tokio::sync::broadcast::Sender<ExecutionInfoForSlot>,
}
