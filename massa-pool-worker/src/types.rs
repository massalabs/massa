use massa_models::{
    address::Address,
    amount::Amount,
    operation::{OperationId, SecureShareOperation},
};
use num::rational::Ratio;
use std::cmp::Reverse;
use std::ops::RangeInclusive;

pub(crate)  type OperationCursorInner = (Reverse<Ratio<u64>>, OperationId);
/// A cursor for pool operations, sorted by increasing quality
#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Debug)]
pub(crate)  struct PoolOperationCursor(OperationCursorInner);

impl PoolOperationCursor {
    /// Create a new pool operation cursor based on inner data
    pub(crate)  fn new(inner: OperationCursorInner) -> Self {
        Self(inner)
    }

    /// Get the ID of the operation
    pub(crate)  fn get_id(&self) -> OperationId {
        self.0 .1
    }
}

#[derive(Debug, Clone)]
pub(crate)  struct OperationInfo {
    pub(crate)  id: OperationId,
    pub(crate)  cursor: PoolOperationCursor,
    pub(crate)  size: usize,
    pub(crate)  max_gas: u64,
    pub(crate)  creator_address: Address,
    pub(crate)  thread: u8,
    pub(crate)  fee: Amount,
    /// max amount that the op might spend from the sender's balance
    pub(crate)  max_spending: Amount,
    pub(crate)  validity_period_range: RangeInclusive<u64>,
}

impl OperationInfo {
    pub(crate)  fn from_op(
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
