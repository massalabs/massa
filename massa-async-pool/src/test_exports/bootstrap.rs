// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::{cmp::Reverse, collections::BTreeMap, str::FromStr};

use crate::{AsyncMessage, AsyncPool, AsyncPoolConfig};
use massa_models::{address::Address, amount::Amount, config::THREAD_COUNT, slot::Slot};
use massa_signature::KeyPair;
use rand::Rng;

/// This file defines tools to test the asynchronous pool bootstrap

/// Creates a `AsyncPool` from pre-set values
pub fn create_async_pool(
    config: AsyncPoolConfig,
    messages: BTreeMap<(Reverse<Amount>, Slot, u64), AsyncMessage>,
) -> AsyncPool {
    let mut async_pool = AsyncPool::new(config);
    async_pool.messages = messages;
    async_pool
}

fn get_random_address() -> Address {
    let keypair = KeyPair::generate();
    Address::from_public_key(&keypair.get_public_key())
}

pub fn get_random_message() -> AsyncMessage {
    let mut rng = rand::thread_rng();
    AsyncMessage {
        emission_slot: Slot::new(rng.gen_range(0..100_000), rng.gen_range(0..THREAD_COUNT)),
        emission_index: 0,
        sender: get_random_address(),
        destination: get_random_address(),
        handler: String::from("test"),
        max_gas: 10_000,
        gas_price: Amount::from_str("100").unwrap(),
        coins: Amount::from_str("100").unwrap(),
        validity_start: Slot::new(2, 0),
        validity_end: Slot::new(4, 0),
        data: vec![1, 2, 3],
    }
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

/// asserts that two `AsyncPool` are equal
pub fn assert_eq_async_pool_bootstrap_state(v1: &AsyncPool, v2: &AsyncPool) {
    assert_eq!(
        v1.messages.len(),
        v2.messages.len(),
        "message count mismatch"
    );
    for (val1, val2) in v1.messages.iter().zip(v2.messages.iter()) {
        assert_eq_async_message(val1.1, val2.1);
    }
}
