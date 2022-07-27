//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::WrappedOperation;
use num::rational::Ratio;
use std::cmp::{Ord, PartialOrd, Reverse};

/// A cursor for pool operations, sorted by increasing quality
#[Derive(PartialOrd, Ord)]
pub struct PoolOperationCursor((Reverse<Ratio<u64>>, OperationId));

pub struct OperationInfo {
    pub id: OperationId,
    pub size: u64,
    pub max_gas: u64,
    pub creator_address: Address,
    pub thread: u8,
    pub fee: u64,
    pub validity_start: Slot,
    pub validity_end: Slot
}

impl From<&WrappedOperation> for OperationInfo {
    fn from(op: &WrappedOperation) -> Self {
        OperationInfo {
            id: op.id,
            size: op.serialized_size(),
            max_gas: op.get_gas_usage(),
            creator_address: op.creator_address,
            fee: op.get_total_fee(),
            thread: op.thread,
            validity_start: op.validity_start,
            validity_end: op.validity_end
        }
    }
}

impl From<&OperationInfo> for PoolOperationCursor {
    fn from(opinfo: &OperationInfo) -> Self {
        let quality = Ratio::new(
            opinfo.fee,
            opinfo.size
        );
        // TODO take into account max_gas as well
        PoolOperationCursor((
            Reverse(quality),
            opinfo.id
        ))
    }
}