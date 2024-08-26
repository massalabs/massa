use massa_models::config::{
    MAX_ASYNC_GAS, MAX_FUNCTION_NAME_LENGTH, MAX_PARAMETERS_SIZE, THREAD_COUNT,
};

#[derive(Debug, Clone)]
pub struct DeferredCallsConfig {
    /// thread count
    pub thread_count: u8,
    /// max function name length
    pub max_function_name_length: u16,
    /// max parameter size
    pub max_parameter_size: u32,

    pub max_deferred_calls_pool_changes: u64,

    pub max_gas: u64,
}

impl Default for DeferredCallsConfig {
    fn default() -> Self {
        Self {
            thread_count: THREAD_COUNT,
            max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
            max_parameter_size: MAX_PARAMETERS_SIZE,
            // TODO: set to a reasonable value
            max_deferred_calls_pool_changes: 1000000,
            max_gas: MAX_ASYNC_GAS,
        }
    }
}
