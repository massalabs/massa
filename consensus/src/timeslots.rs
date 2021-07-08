/// warning: assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0
use time::UTime;

use crate::error::ConsensusError;

/// Gets timestamp in millis for given slot.
///
/// # Arguments
/// * thread_count: number of threads.
/// * t0: time in millis between two periods in the same thread.
/// * slot: the considered slot.
pub fn get_block_slot_timestamp(
    thread_count: u8,
    t0: UTime,
    genesis_timestamp: UTime,
    slot: (u64, u8),
) -> Result<UTime, ConsensusError> {
    let base: UTime = t0
        .checked_div_u64(thread_count as u64)
        .or(Err(ConsensusError::TimeOverflowError))?
        .checked_mul(slot.1 as u64)
        .or(Err(ConsensusError::TimeOverflowError))?;
    let shift: UTime = t0
        .checked_mul(slot.0)
        .or(Err(ConsensusError::TimeOverflowError))?;
    Ok(genesis_timestamp
        .checked_add(base)
        .or(Err(ConsensusError::TimeOverflowError))?
        .checked_add(shift)
        .or(Err(ConsensusError::TimeOverflowError))?)
}

/// Returns the thread and block period index of the latest block slot at a given timstamp (inclusive), if any happened
///
/// # Arguments
/// * thread_count: number of threads.
/// * t0: time in millis between two periods in the same thread.
/// * genesis_timestamp: when the blockclique first started, in millis.
/// * timestamp: target timestamp in millis.
pub fn get_latest_block_slot_at_timestamp(
    thread_count: u8,
    t0: UTime,
    genesis_timestamp: UTime,
    timestamp: UTime,
) -> Result<Option<(u64, u8)>, ConsensusError> {
    if let Ok(time_since_genesis) = timestamp.checked_sub(genesis_timestamp) {
        let thread: u8 = time_since_genesis
            .checked_rem_time(t0)?
            .checked_div_time(t0.checked_div_u64(thread_count as u64)?)?
            as u8;
        return Ok(Some((time_since_genesis.checked_div_time(t0)?, thread)));
    }
    Ok(None)
}

/// Returns the thread and block slot index of the current block slot (inclusive), if any happened yet
///
/// # Arguments
/// * thread_count: number of threads.
/// * t0: time in millis between two periods in the same thread.
/// * genesis_timestamp: when the blockclique first started, in millis.
pub fn get_current_latest_block_slot(
    thread_count: u8,
    t0: UTime,
    genesis_timestamp: UTime,
) -> Result<Option<(u64, u8)>, ConsensusError> {
    get_latest_block_slot_at_timestamp(thread_count, t0, genesis_timestamp, UTime::now()?)
}

/// Returns the (period, thread) of the next block slot
///
/// # Arguments
/// * thread_count: number of threads.
/// * slot: previous slot.
pub fn get_next_block_slot(
    thread_count: u8,
    slot: (u64, u8), // period, thread
) -> Result<(u64, u8), ConsensusError> {
    if slot.1 == thread_count - 1 {
        Ok((
            slot.0
                .checked_add(1u64)
                .ok_or(ConsensusError::SlotOverflowError)?,
            0u8,
        ))
    } else {
        Ok((
            slot.0,
            slot.1
                .checked_add(1u8)
                .ok_or(ConsensusError::ThreadOverflowError)?,
        ))
    }
}
