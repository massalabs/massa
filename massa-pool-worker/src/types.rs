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
    /// The maximum amount of gas that can be used by an operation.
    pub max_gas_usage: u64,
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
        base_operation_gas_cost: u64,
    ) -> Self {
        OperationInfo {
            id: op.id,
            size: op.serialized_size(),
            max_gas_usage: op.get_gas_usage(base_operation_gas_cost),
            creator_address: op.content_creator_address,
            fee: op.content.fee,
            thread: op.content_creator_address.get_thread(thread_count),
            validity_period_range: op.get_validity_range(operation_validity_periods),
            max_spending: op.get_max_spending(roll_price),
        }
    }
}
