// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::str::FromStr;

use crate::{AsyncMessage, AsyncPool, AsyncPoolBootstrap};
use massa_models::{Address, Amount, Slot};
use massa_signature::{derive_public_key, generate_random_private_key};
use rand::Rng;

/// This file defines tools to test the asynchronous pool bootstrap

fn get_random_address() -> Address {
    let priv_key = generate_random_private_key();
    let pub_key = derive_public_key(&priv_key);
    Address::from_public_key(&pub_key)
}

pub fn get_random_message() -> AsyncMessage {
    let mut rng = rand::thread_rng();
    AsyncMessage {
        emission_slot: Slot::new(rng.gen::<u64>(), rng.gen::<u8>()),
        emission_index: 0,
        sender: get_random_address(),
        destination: get_random_address(),
        handler: String::from("test"),
        max_gas: rng.gen::<u64>(),
        gas_price: Amount::from_str("100000").unwrap(),
        coins: Amount::from_str("100000").unwrap(),
        validity_start: Slot::new(2, 0),
        validity_end: Slot::new(4, 0),
        data: vec![1, 2, 3],
    }
}

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
