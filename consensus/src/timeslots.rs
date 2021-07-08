use std::convert::TryInto;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, Instant};

use crate::error::ConsensusError;

// warning: assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0

pub fn get_current_timestamp_millis() -> Result<u64, ConsensusError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ConsensusError::TimeOverflowError)?
        .as_millis()
        .try_into()
        .map_err(|_| ConsensusError::TimeOverflowError)?)
}

pub fn get_block_slot_timestamp_millis(
    thread_count: u8,
    t0_millis: u64,
    genesis_timestamp_millis: u64,
    thread: u8,
    slot: u64,
) -> Result<u64, ConsensusError> {
    let base: u64 = (thread as u64)
        .checked_mul(t0_millis / (thread_count as u64))
        .ok_or(ConsensusError::TimeOverflowError)?;
    let shift: u64 = t0_millis
        .checked_mul(slot)
        .ok_or(ConsensusError::TimeOverflowError)?;
    Ok(genesis_timestamp_millis
        .checked_add(base)
        .ok_or(ConsensusError::TimeOverflowError)?
        .checked_add(shift)
        .ok_or(ConsensusError::TimeOverflowError)?)
}

// return the thread and block slot index of the latest block slot (inclusive), if any happened yet
pub fn get_current_latest_block_slot(
    thread_count: u8,
    t0_millis: u64,
    genesis_timestamp_millis: u64,
) -> Result<Option<(u8, u64)>, ConsensusError> {
    Ok(get_current_timestamp_millis()?
        .checked_sub(genesis_timestamp_millis)
        .and_then(|millis_since_genesis| {
            Some((
                ((millis_since_genesis % t0_millis) / (t0_millis / (thread_count as u64))) as u8,
                millis_since_genesis / t0_millis,
            ))
        }))
}

// return the (thread, slot) of the next block slot
pub fn get_next_block_slot(
    thread_count: u8,
    thread: u8,
    slot: u64,
) -> Result<(u8, u64), ConsensusError> {
    if thread == thread_count - 1 {
        Ok((
            0u8,
            slot.checked_add(1u64)
                .ok_or(ConsensusError::SlotOverflowError)?,
        ))
    } else {
        Ok((
            thread
                .checked_add(1u8)
                .ok_or(ConsensusError::ThreadOverflowError)?,
            slot,
        ))
    }
}

pub fn estimate_instant_from_timestamp(timestamp_millis: u64) -> Result<Instant, ConsensusError> {
    let (cur_timestamp_millis, cur_instant): (u64, Instant) =
        (get_current_timestamp_millis()?, Instant::now());
    Ok(cur_instant
        .checked_sub(Duration::from_millis(cur_timestamp_millis))
        .ok_or(ConsensusError::TimeOverflowError)?
        .checked_add(Duration::from_millis(timestamp_millis))
        .ok_or(ConsensusError::TimeOverflowError)?)
}
