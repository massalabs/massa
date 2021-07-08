use time::UTime;

use crate::error::ConsensusError;

// warning: assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0

pub fn get_block_slot_timestamp(
    thread_count: u8,
    t0: UTime,
    genesis_timestamp: UTime,
    thread: u8,
    slot: u64,
) -> Result<UTime, ConsensusError> {
    let base: UTime = t0
        .checked_div(UTime::from(thread_count as u64))
        .or(Err(ConsensusError::TimeOverflowError))?
        .checked_mul(UTime::from(thread as u64))
        .or(Err(ConsensusError::TimeOverflowError))?;
    let shift: UTime = t0
        .checked_mul(UTime::from(slot))
        .or(Err(ConsensusError::TimeOverflowError))?;
    Ok(genesis_timestamp
        .checked_add(base)
        .or(Err(ConsensusError::TimeOverflowError))?
        .checked_add(shift)
        .or(Err(ConsensusError::TimeOverflowError))?)
}

// return the thread and block slot index of the latest block slot (inclusive), if any happened yet
pub fn get_current_latest_block_slot(
    thread_count: u8,
    t0: UTime,
    genesis_timestamp: UTime,
) -> Result<Option<(u8, u64)>, ConsensusError> {
    if let Ok(time_since_genesis) = UTime::now()?.checked_sub(genesis_timestamp) {
        let thread: u8 = time_since_genesis
            .checked_rem(t0)?
            .checked_div(t0.checked_div(UTime::from(thread_count as u64))?)?
            .to_millis() as u8;
        return Ok(Some((
            thread,
            time_since_genesis.checked_div(t0)?.to_millis(),
        )));
    }
    Ok(None)
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
