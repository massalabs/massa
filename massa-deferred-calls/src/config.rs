use massa_models::{
    amount::Amount,
    config::{
        DEFERRED_CALL_BASE_FEE_MAX_CHANGE_DENOMINATOR, DEFERRED_CALL_CST_GAS_COST,
        DEFERRED_CALL_GLOBAL_OVERBOOKING_PENALTY, DEFERRED_CALL_MAX_ASYNC_GAS,
        DEFERRED_CALL_MAX_FUTURE_SLOTS, DEFERRED_CALL_MAX_POOL_CHANGES, DEFERRED_CALL_MIN_GAS_COST,
        DEFERRED_CALL_MIN_GAS_INCREMENT, DEFERRED_CALL_SLOT_OVERBOOKING_PENALTY,
        LEDGER_COST_PER_BYTE, MAX_FUNCTION_NAME_LENGTH, MAX_PARAMETERS_SIZE, THREAD_COUNT,
    },
};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct DeferredCallsConfig {
    /// thread count
    pub thread_count: u8,
    /// max function name length
    pub max_function_name_length: u16,
    /// Maximum size of deferred call future slots (1 week)
    pub max_future_slots: u64,
    /// base fee max max change denominator
    pub base_fee_max_max_change_denominator: usize,
    /// min gas increment (1 nanomassa)
    pub min_gas_increment: u64,
    /// min gas cost (10 nanomassa)
    pub min_gas_cost: u64,
    /// call gas cost
    pub call_cst_gas_cost: u64,
    /// global overbooking penalty
    pub global_overbooking_penalty: Amount,
    /// slot overbooking penalty
    pub slot_overbooking_penalty: Amount,

    /// max parameter size
    pub max_parameter_size: u32,

    pub max_pool_changes: u64,

    pub max_gas: u64,

    pub ledger_cost_per_byte: Amount,
}

impl Default for DeferredCallsConfig {
    fn default() -> Self {
        Self {
            thread_count: THREAD_COUNT,
            max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
            max_future_slots: DEFERRED_CALL_MAX_FUTURE_SLOTS,
            max_parameter_size: MAX_PARAMETERS_SIZE,
            max_pool_changes: DEFERRED_CALL_MAX_POOL_CHANGES,
            max_gas: DEFERRED_CALL_MAX_ASYNC_GAS,
            base_fee_max_max_change_denominator: DEFERRED_CALL_BASE_FEE_MAX_CHANGE_DENOMINATOR,
            min_gas_increment: DEFERRED_CALL_MIN_GAS_INCREMENT,
            min_gas_cost: DEFERRED_CALL_MIN_GAS_COST,
            global_overbooking_penalty: DEFERRED_CALL_GLOBAL_OVERBOOKING_PENALTY,
            slot_overbooking_penalty: DEFERRED_CALL_SLOT_OVERBOOKING_PENALTY,
            call_cst_gas_cost: DEFERRED_CALL_CST_GAS_COST,
            ledger_cost_per_byte: LEDGER_COST_PER_BYTE,
        }
    }
}
