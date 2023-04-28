// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::types::SlotExecutionOutput;

/// channels used by the execution worker
#[derive(Clone)]
pub struct ExecutionChannels {
    /// Broadcast channel for new smart contract execution outputs
    pub sc_execution_output_sender: tokio::sync::broadcast::Sender<SlotExecutionOutput>,
}
