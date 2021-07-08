//! warning: assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0
use models::slot::Slot;
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
    slot: Slot,
) -> Result<UTime, ConsensusError> {
    let base: UTime = t0
        .checked_div_u64(thread_count as u64)
        .or(Err(ConsensusError::TimeOverflowError))?
        .checked_mul(slot.thread as u64)
        .or(Err(ConsensusError::TimeOverflowError))?;
    let shift: UTime = t0
        .checked_mul(slot.period)
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
) -> Result<Option<Slot>, ConsensusError> {
    if let Ok(time_since_genesis) = timestamp.checked_sub(genesis_timestamp) {
        let thread: u8 = time_since_genesis
            .checked_rem_time(t0)?
            .checked_div_time(t0.checked_div_u64(thread_count as u64)?)?
            as u8;
        return Ok(Some(Slot::new(
            time_since_genesis.checked_div_time(t0)?,
            thread,
        )));
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
) -> Result<Option<Slot>, ConsensusError> {
    get_latest_block_slot_at_timestamp(thread_count, t0, genesis_timestamp, UTime::now()?)
}

pub fn time_range_to_slot_range(
    thread_count: u8,
    t0: UTime,
    genesis_timestamp: UTime,
    start_time: Option<UTime>,
    end_time: Option<UTime>,
) -> Result<(Option<Slot>, Option<Slot>), ConsensusError> {
    let start_slot = match start_time {
        None => None,
        Some(t) => {
            let inter_slot = t0.checked_div_u64(thread_count as u64)?;
            let slot_number: u64 = t
                .saturating_sub(genesis_timestamp)
                .checked_add(inter_slot)?
                .saturating_sub(UTime::EPSILON)
                .checked_div_time(inter_slot)?;
            Some(Slot::new(
                slot_number
                    .checked_div(thread_count as u64)
                    .ok_or(ConsensusError::TimeOverflowError)?,
                slot_number
                    .checked_rem(thread_count as u64)
                    .ok_or(ConsensusError::TimeOverflowError)?,
            ))
        }
    };

    let end_slot = match end_time {
        None => None,
        Some(t) => {
            let inter_slot = t0.checked_div_u64(thread_count as u64)?;
            let slot_number: u64 = t
                .saturating_sub(genesis_timestamp)
                .checked_add(inter_slot)?
                .saturating_sub(UTime::EPSILON)
                .checked_div_time(inter_slot)?;
            Some(Slot::new(
                slot_number
                    .checked_div(thread_count as u64)
                    .ok_or(ConsensusError::TimeOverflowError)?,
                slot_number
                    .checked_rem(thread_count as u64)
                    .ok_or(ConsensusError::TimeOverflowError)?,
            ))
        }
    };

    Ok((start_slot, end_slot))
}
