// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::AsyncMessage;
use massa_models::{address::Address, amount::Amount, slot::Slot};
use massa_signature::KeyPair;
use rand::Rng;
use std::str::FromStr;

/// This file defines tools to test the asynchronous pool bootstrap

fn get_random_address() -> Address {
    let keypair = KeyPair::generate(0).unwrap();
    Address::from_public_key(&keypair.get_public_key())
}

pub fn get_random_message(fee: Option<Amount>, thread_count: u8) -> AsyncMessage {
    let mut rng = rand::thread_rng();
    AsyncMessage::new(
        Slot::new(rng.gen_range(0..100_000), rng.gen_range(0..thread_count)),
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
        None,
    )
}
