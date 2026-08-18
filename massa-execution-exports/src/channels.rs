// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::types::SlotExecutionOutput;

#[cfg(feature = "execution-trace")]
use crate::types_trace_info::{SlotAbiCallStack, Transfer};

#[cfg(feature = "execution-info")]
use crate::execution_info::ExecutionInfoForSlot;

/// channels used by the execution worker
#[derive(Clone)]
pub struct ExecutionChannels {
    /// Broadcast channel for new slot execution outputs
    pub slot_execution_output_sender: tokio::sync::broadcast::Sender<SlotExecutionOutput>,
    /// Broadcast channel for execution traces (abi call stacks, the slot's transfers, boolean true if the slot is finalized, false otherwise).
    /// The transfers are bound to the same execution instance as the abi call stacks so subscribers never need a separate slot-keyed lookup (which could be overwritten by a re-execution of the same slot).
    #[cfg(feature = "execution-trace")]
    pub slot_execution_traces_sender:
        tokio::sync::broadcast::Sender<(SlotAbiCallStack, Vec<Transfer>, bool)>,
    /// Broadcast channel for execution info
    #[cfg(feature = "execution-info")]
    pub slot_execution_info_sender: tokio::sync::broadcast::Sender<ExecutionInfoForSlot>,
}
