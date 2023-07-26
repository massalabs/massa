// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{
    AsyncMessage, AsyncMessageDeserializer, AsyncMessageId, AsyncMessageIdDeserializer, AsyncPool,
};
use massa_db_exports::{ASYNC_POOL_PREFIX, STATE_CF};
use massa_models::{
    address::Address,
    amount::Amount,
    config::{MAX_ASYNC_MESSAGE_DATA, MAX_DATASTORE_KEY_LENGTH, THREAD_COUNT},
    slot::Slot,
};
use massa_serialization::{DeserializeError, Deserializer};
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
    let message_id_deserializer = AsyncMessageIdDeserializer::new(THREAD_COUNT);
    let message_deserializer = AsyncMessageDeserializer::new(
        THREAD_COUNT,
        MAX_ASYNC_MESSAGE_DATA,
        MAX_DATASTORE_KEY_LENGTH as u32,
        false,
    );
    let db1 = v1.db.read();
    let db2 = v2.db.read();

    let iter_1 = db1
        .prefix_iterator_cf(STATE_CF, ASYNC_POOL_PREFIX.as_bytes())
        .take_while(|(k, _v)| k.starts_with(ASYNC_POOL_PREFIX.as_bytes()));
    let iter_2 = db2
        .prefix_iterator_cf(STATE_CF, ASYNC_POOL_PREFIX.as_bytes())
        .take_while(|(k, _v)| k.starts_with(ASYNC_POOL_PREFIX.as_bytes()));

    assert_eq!(
        iter_1.count(),
        iter_2.count(),
        "message values count mismatch"
    );

    // Iterates over the whole database
    let mut current_id: Option<AsyncMessageId> = None;
    let mut current_message_1: Vec<u8> = Vec::new();
    let mut current_message_2: Vec<u8> = Vec::new();
    let mut current_count = 0u8;
    const TOTAL_FIELDS_COUNT: u8 = 13;

    let iter_1 = db1
        .prefix_iterator_cf(STATE_CF, ASYNC_POOL_PREFIX.as_bytes())
        .take_while(|(k, _v)| k.starts_with(ASYNC_POOL_PREFIX.as_bytes()));
    let iter_2 = db2
        .prefix_iterator_cf(STATE_CF, ASYNC_POOL_PREFIX.as_bytes())
        .take_while(|(k, _v)| k.starts_with(ASYNC_POOL_PREFIX.as_bytes()));

    for (val1, val2) in iter_1.zip(iter_2) {
        let (_, message_id_1) = message_id_deserializer
            .deserialize::<DeserializeError>(&val1.0)
            .unwrap();
        let (_, message_id_2) = message_id_deserializer
            .deserialize::<DeserializeError>(&val2.0)
            .unwrap();

        if Some(message_id_1) == current_id && message_id_1 == message_id_2 {
            current_count += 1;
            current_message_1.extend(val1.1.iter());
            current_message_2.extend(val2.1.iter());
            if current_count == TOTAL_FIELDS_COUNT {
                let (_rest, message1) = message_deserializer
                    .deserialize::<DeserializeError>(&current_message_1)
                    .unwrap();
                let (_rest, message2) = message_deserializer
                    .deserialize::<DeserializeError>(&current_message_2)
                    .unwrap();
                assert_eq_async_message(&message1, &message2);

                current_count = 0;
                current_message_1.clear();
                current_message_2.clear();
                current_id = None;
            }
        } else {
            // We reset the current values
            current_id = Some(message_id_1);
            current_count = 1;
            current_message_1.clear();
            current_message_1.extend(val1.1.iter());
            current_message_2.clear();
            current_message_2.extend(val2.1.iter());
        }
    }
}
