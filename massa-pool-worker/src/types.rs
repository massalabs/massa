use massa_models::{
    address::Address,
    amount::Amount,
    operation::{OperationId, SecureShareOperation},
};
use std::ops::RangeInclusive;

#[derive(Debug, Clone)]
pub struct OperationInfo {
    pub id: OperationId,
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
            size: op.serialized_size(),
            max_gas: op.get_gas_usage(),
            creator_address: op.content_creator_address,
            fee: op.content.get_fee(),
            thread: op.content_creator_address.get_thread(thread_count),
            validity_period_range: op.get_validity_range(operation_validity_periods),
            max_spending: op.get_max_spending(roll_price),
        }
    }
}
