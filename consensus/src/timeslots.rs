use std::convert::TryInto;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, Instant};

// warning: assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0

pub fn get_current_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time overflow")
        .as_millis()
        .try_into()
        .expect("time overflow")
}

pub fn get_block_slot_timestamp_millis(
    thread_count: u8,
    t0_millis: u64,
    genesis_timestamp_millis: u64,
    thread: u8,
    slot: u64,
) -> u64 {
    let base: u64 = (thread as u64)
        .checked_mul(t0_millis / (thread_count as u64))
        .expect("time overflow");
    let shift: u64 = t0_millis.checked_mul(slot).expect("time overflow");
    genesis_timestamp_millis
        .checked_add(base)
        .expect("time overflow")
        .checked_add(shift)
        .expect("time overflow")
}

// return the thread and block slot index of the latest block slot (inclusive), if any happened yet
pub fn get_current_latest_block_slot(
    thread_count: u8,
    t0_millis: u64,
    genesis_timestamp_millis: u64,
) -> Option<(u8, u64)> {
    get_current_timestamp_millis()
        .checked_sub(genesis_timestamp_millis)
        .and_then(|millis_since_genesis| {
            Some((
                ((millis_since_genesis % t0_millis) / (t0_millis / (thread_count as u64))) as u8,
                millis_since_genesis / t0_millis,
            ))
        })
}

// return the (thread, slot) of the next block slot
pub fn get_next_block_slot(thread_count: u8, thread: u8, slot: u64) -> (u8, u64) {
    if thread == thread_count - 1 {
        (0u8, slot.checked_add(1u64).expect("slot overflow"))
    } else {
        (thread.checked_add(1u8).expect("thread overflow"), slot)
    }
}

pub fn estimate_instant_from_timestamp(timestamp_millis: u64) -> Instant {
    let (cur_timestamp_millis, cur_instant): (u64, Instant) =
        (get_current_timestamp_millis(), Instant::now());
    cur_instant
        .checked_sub(Duration::from_millis(cur_timestamp_millis))
        .expect("time underflow")
        .checked_add(Duration::from_millis(timestamp_millis))
        .expect("time overflow")
}
