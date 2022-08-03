use massa_models::{Address, Amount, OperationId, WrappedOperation};
use massa_pool_exports::PoolOperationCursor;
use num::rational::Ratio;
use std::cmp::Reverse;
use std::ops::RangeInclusive;

#[derive(Debug, Clone)]
pub struct OperationInfo {
    pub id: OperationId,
    pub size: usize,
    pub max_gas: u64,
    pub creator_address: Address,
    pub thread: u8,
    pub fee: Amount,
    /// max amount that the op might spend from the sender's sequential balance
    pub max_sequential_spending: Amount,
    pub validity_period_range: RangeInclusive<u64>,
}

impl OperationInfo {
    pub fn from_op(op: &WrappedOperation, operation_validity_periods: u64) -> Self {
        OperationInfo {
            id: op.id,
            size: op.serialized_size(),
            max_gas: op.get_gas_usage(),
            creator_address: op.creator_address,
            fee: op.get_total_fee(),
            thread: op.thread,
            validity_period_range: op.get_validity_range(operation_validity_periods),
            max_sequential_spending: op.get_max_sequential_spending(),
        }
    }
}

pub fn build_cursor(op: &WrappedOperation) -> PoolOperationCursor {
    let quality = Ratio::new(op.get_total_fee().to_raw(), op.serialized_size() as u64);
    let inner = (Reverse(quality), op.id);
    // TODO take into account max_gas as well in the future (multi-dimensional packing)
    PoolOperationCursor::new(inner)
}
