// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::{collections::BTreeMap, str::FromStr, sync::Arc};

use crate::{
    pool::ASYNC_POOL_CF, AsyncMessage, AsyncMessageDeserializer, AsyncMessageId, AsyncPool,
    AsyncPoolConfig,
};
use massa_models::{
    address::Address,
    amount::Amount,
    config::{MAX_ASYNC_MESSAGE_DATA, MAX_DATASTORE_KEY_LENGTH, THREAD_COUNT},
    slot::Slot,
};
use massa_serialization::{DeserializeError, Deserializer};
use massa_signature::KeyPair;
use parking_lot::RwLock;
use rand::Rng;
use rocksdb::{IteratorMode, DB};

/// This file defines tools to test the asynchronous pool bootstrap

/// Creates a `AsyncPool` from pre-set values
pub fn create_async_pool(
    db: Arc<RwLock<DB>>,
    config: AsyncPoolConfig,
    messages: BTreeMap<AsyncMessageId, AsyncMessage>,
) -> AsyncPool {
    let mut async_pool = AsyncPool::new(config, db);
    async_pool.set_pool_part(messages);
    async_pool
}

fn get_random_address() -> Address {
    let keypair = KeyPair::generate();
    Address::from_public_key(&keypair.get_public_key())
}

pub fn get_random_message(fee: Option<Amount>, thread_count: u8) -> AsyncMessage {
    let mut rng = rand::thread_rng();
    AsyncMessage::new_with_hash(
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
    let message_deserializer = AsyncMessageDeserializer::new(
        THREAD_COUNT,
        MAX_ASYNC_MESSAGE_DATA,
        MAX_DATASTORE_KEY_LENGTH as u32,
        false,
    );
    let db1 = v1.db.read();
    let db2 = v2.db.read();
    let handle1 = db1.cf_handle(ASYNC_POOL_CF).unwrap();
    let handle2 = db2.cf_handle(ASYNC_POOL_CF).unwrap();
    assert_eq!(
        db1.iterator_cf(handle1, IteratorMode::Start).count(),
        db2.iterator_cf(handle2, IteratorMode::Start).count(),
        "message count mismatch"
    );
    for (val1, val2) in db1
        .iterator_cf(handle1, IteratorMode::Start)
        .zip(db2.iterator_cf(handle2, IteratorMode::Start))
    {
        let val1 = val1.unwrap();
        let val2 = val2.unwrap();

        let (_rest, message1) = message_deserializer
            .deserialize::<DeserializeError>(&val1.1)
            .unwrap();
        let (_rest, message2) = message_deserializer
            .deserialize::<DeserializeError>(&val2.1)
            .unwrap();
        assert_eq_async_message(&message1, &message2);
    }
}
