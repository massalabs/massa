use massa_models::{
    address::Address,
    amount::Amount,
    operation::{OperationId, SecureShareOperation},
};
use num::rational::Ratio;
use std::cmp::Reverse;
use std::ops::RangeInclusive;

pub type OperationCursorInner = (Reverse<Ratio<u64>>, OperationId);
/// A cursor for pool operations, sorted by increasing quality
#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Debug)]
pub struct PoolOperationCursor(OperationCursorInner);

impl PoolOperationCursor {
    /// Create a new pool operation cursor based on inner data
    pub fn new(inner: OperationCursorInner) -> Self {
        Self(inner)
    }

    /// Get the ID of the operation
    pub fn get_id(&self) -> OperationId {
        self.0 .1
    }
}

#[derive(Debug, Clone)]
pub struct OperationInfo {
    pub id: OperationId,
    pub cursor: PoolOperationCursor,
    pub size: usize,
    pub max_gas: u64,
    pub creator_address: Address,
    pub thread: u8,
    pub fee: Amount,
    /// max amount that the op might spend from the sender's balance
    pub max_spending: Amount,
    pub validity_period_range: RangeInclusive<u64>,
}

impl OperationInfo {
    pub fn from_op(
        op: &SecureShareOperation,
        operation_validity_periods: u64,
        roll_price: Amount,
        thread_count: u8,
    ) -> Self {
        OperationInfo {
            id: op.id,
            cursor: build_operation_cursor(op),
            size: op.serialized_size(),
            max_gas: op.get_gas_usage(),
            creator_address: op.content_creator_address,
            fee: op.content.fee,
            thread: op.content_creator_address.get_thread(thread_count),
            validity_period_range: op.get_validity_range(operation_validity_periods),
            max_spending: op.get_max_spending(roll_price),
        }
    }
}

/// build a cursor from an operation
fn build_operation_cursor(op: &SecureShareOperation) -> PoolOperationCursor {
    let quality = Ratio::new(op.content.fee.to_raw(), op.serialized_size() as u64);
    let inner = (Reverse(quality), op.id);
    // TODO take into account max_gas as well in the future (multi-dimensional packing)
    PoolOperationCursor::new(inner)
}
