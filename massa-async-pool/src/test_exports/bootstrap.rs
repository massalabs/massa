// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{AsyncMessage, AsyncPoolBootstrap};

/// This file defines tools to test the asynchronous pool bootstrap

/// Creates an asynchronous pool bootstrap state from components
pub fn make_bootstrap_state(messages: Vec<AsyncMessage>) -> AsyncPoolBootstrap {
    AsyncPoolBootstrap { messages }
}

/// Asserts that two instances of `AsyncMessage` are the same
pub fn assert_eq_async_message(v1: &AsyncMessage, v2: &AsyncMessage) {
    assert_eq!(v1.emission_slot, v2.emission_slot, "emission_slot mismatch");
    assert_eq!(
        v1.emission_index, v2.emission_index,
        "emission_index mismatch"
    );
    assert_eq!(v1.sender, v2.sender, "sender mismatch");
    assert_eq!(v1.destination, v2.destination, "destination mismatch");
    assert_eq!(v1.handler, v2.handler, "handler mismatch");
    assert_eq!(v1.max_gas, v2.max_gas, "max_gas mismatch");
    assert_eq!(v1.gas_price, v2.gas_price, "gas_price mismatch");
    assert_eq!(v1.coins, v2.coins, "coins mismatch");
    assert_eq!(
        v1.validity_start, v2.validity_start,
        "validity_start mismatch"
    );
    assert_eq!(v1.validity_end, v2.validity_end, "validity_end mismatch");
    assert_eq!(v1.data, v2.data, "data mismatch");
}

/// asserts that two `AsyncPoolBootstrap` are equal
pub fn assert_eq_async_pool_bootstrap_state(v1: &AsyncPoolBootstrap, v2: &AsyncPoolBootstrap) {
    assert_eq!(
        v1.messages.len(),
        v2.messages.len(),
        "message count mismatch"
    );
    for (val1, val2) in v1.messages.iter().zip(v2.messages.iter()) {
        assert_eq_async_message(val1, val2);
    }
}
