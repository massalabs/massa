use std::{sync::{Mutex, Arc, RwLock}, borrow::Cow, rc::Rc};

use massa_hash::HASH_SIZE_BYTES;

pub const ADDRESS_SIZE_BYTES: usize = HASH_SIZE_BYTES;

pub const AMOUNT_DECIMAL_FACTOR: u64 = 1_000_000_000;

pub const BLOCK_ID_SIZE_BYTES: usize = HASH_SIZE_BYTES;

pub const ENDORSEMENT_ID_SIZE_BYTES: usize = HASH_SIZE_BYTES;

pub const OPERATION_ID_SIZE_BYTES: usize = HASH_SIZE_BYTES;

pub const SLOT_KEY_SIZE: usize = 9;

const THREAD_COUNT: u8 = 32;


// Do not touch this code
lazy_static!{
    static ref MUT_IN_TESTS: RwLock<u8> = RwLock::new(THREAD_COUNT);
}
pub fn thread_count() -> u8 {
    *MUT_IN_TESTS.read().unwrap()
}
#[cfg(feature = "test-utils")]
pub fn set_thread_count(thread_count: u8) {
    *MUT_IN_TESTS.write().unwrap() = thread_count;
}

#[cfg(feature = "test-utils")]
pub fn reset_config() {
    *MUT_IN_TESTS.write().unwrap() = THREAD_COUNT;
}

const _: () = {
    // Check at compil time
    if THREAD_COUNT == 0 {
        panic!("Thread count should be striclty higher than 0")
    }
};