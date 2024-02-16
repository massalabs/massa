// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::types::SlotExecutionOutput;

#[cfg(feature = "execution-trace")]
use crate::types::SlotAbiCallStack;

/// channels used by the execution worker
#[derive(Clone)]
pub struct ExecutionChannels {
    /// Broadcast channel for new slot execution outputs
    pub slot_execution_output_sender: tokio::sync::broadcast::Sender<SlotExecutionOutput>,
    #[cfg(feature = "execution-trace")]
    /// Broadcast channel for execution traces
    pub slot_execution_traces_sender: tokio::sync::broadcast::Sender<SlotAbiCallStack>,
}
