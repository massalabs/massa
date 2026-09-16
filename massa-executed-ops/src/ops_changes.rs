//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{operation::OperationId, prehash::PreHashMap, slot::Slot};

/// Changes for ExecutedOps (was_successful, op_expiry_slot)
pub type ExecutedOpsChanges = PreHashMap<OperationId, (bool, Slot)>;
