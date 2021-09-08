// Copyright (c) 2021 MASSA LABS <info@massa.net>

//! warning: assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0
use crate::error::ConsensusError;
use models::Slot;
use std::convert::TryInto;
use time::UTime;

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
    genesis_timestamp
        .checked_add(base)
        .or(Err(ConsensusError::TimeOverflowError))?
        .checked_add(shift)
        .or(Err(ConsensusError::TimeOverflowError))
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
    clock_compensation: i64,
) -> Result<Option<Slot>, ConsensusError> {
    get_latest_block_slot_at_timestamp(
        thread_count,
        t0,
        genesis_timestamp,
        UTime::now(clock_compensation)?,
    )
}

/// Turns an UTime range [start, end) with optional start/end to a Slot range [start, end) with optional start/end
///
/// # Arguments
/// * thread_count: number of threads.
/// * t0: time in millis between two periods in the same thread.
/// * genesis_timestamp: when the blockclique first started, in millis
/// * start_time: optional start time
/// * end_time: optional end time
/// # Returns
/// (Option<Slot>, Option<Slot>) pair of options representing the start (included) and end (excluded) slots
/// or ConsensusError on error
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
                    .ok_or(ConsensusError::TimeOverflowError)?
                    .try_into()
                    .map_err(|_| ConsensusError::ThreadOverflowError)?,
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
                    .ok_or(ConsensusError::TimeOverflowError)?
                    .try_into()
                    .map_err(|_| ConsensusError::ThreadOverflowError)?,
            ))
        }
    };

    Ok((start_slot, end_slot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use time::UTime;

    #[test]
    #[serial]
    fn test_time_range_to_slot_range() {
        let thread_count = 3u8;
        let t0: UTime = 30.into();
        let genesis_timestamp: UTime = 100.into();
        /* slots:   (0, 0)  (0, 1)  (0, 2)  (1, 0)  (1, 1)  (1, 2)  (2, 0)  (2, 1)  (2, 2)
            time:    100      110     120    130      140    150     160     170     180
        */

        // time [111, 115) => empty slot range
        let (out_start, out_end) = time_range_to_slot_range(
            thread_count,
            t0,
            genesis_timestamp,
            Some(111.into()),
            Some(115.into()),
        )
        .unwrap();
        assert_eq!(out_start, out_end);

        // time [10, 100) => empty slot range
        let (out_start, out_end) = time_range_to_slot_range(
            thread_count,
            t0,
            genesis_timestamp,
            Some(10.into()),
            Some(100.into()),
        )
        .unwrap();
        assert_eq!(out_start, out_end);

        // time [115, 145) => slots [(0,2), (1,2))
        let (out_start, out_end) = time_range_to_slot_range(
            thread_count,
            t0,
            genesis_timestamp,
            Some(115.into()),
            Some(145.into()),
        )
        .unwrap();
        assert_eq!(out_start, Some(Slot::new(0, 2)));
        assert_eq!(out_end, Some(Slot::new(1, 2)));

        // time [110, 160) => slots [(0,1), (2,0))
        let (out_start, out_end) = time_range_to_slot_range(
            thread_count,
            t0,
            genesis_timestamp,
            Some(110.into()),
            Some(160.into()),
        )
        .unwrap();
        assert_eq!(out_start, Some(Slot::new(0, 1)));
        assert_eq!(out_end, Some(Slot::new(2, 0)));
    }
}
