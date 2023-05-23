// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::{collections::BTreeMap, str::FromStr};

use crate::{AsyncMessage, AsyncMessageId, AsyncPool, AsyncPoolConfig};
use massa_models::{address::Address, amount::Amount, config::THREAD_COUNT, slot::Slot};
use massa_signature::KeyPair;
use rand::Rng;

/// This file defines tools to test the asynchronous pool bootstrap

/// Creates a `AsyncPool` from pre-set values
pub fn create_async_pool(
    config: AsyncPoolConfig,
    messages: BTreeMap<AsyncMessageId, AsyncMessage>,
) -> AsyncPool {
    let mut async_pool = AsyncPool::new(config);
    async_pool.messages = messages;
    async_pool
}

fn get_random_address() -> Address {
    let keypair = KeyPair::generate(0).unwrap();
    Address::from_public_key(&keypair.get_public_key())
}

pub fn get_random_message(fee: Option<Amount>) -> AsyncMessage {
    let mut rng = rand::thread_rng();
    AsyncMessage::new_with_hash(
        Slot::new(rng.gen_range(0..100_000), rng.gen_range(0..THREAD_COUNT)),
        0,
        get_random_address(),
        get_random_address(),
        String::from("test"),
        10_000,
        fee.unwrap_or_default(),
        Amount::from_str("100").unwrap(),
        Slot::new(2, 0),
        Slot::new(4, 0),
        vec![1, 2, 3],
        None,
    )
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
    assert_eq!(v1.fee, v2.fee, "fee mismatch");
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
