use displaydoc::Display;
use thiserror::Error;

/// pos error
pub type PosResult<T, E = PosError> = core::result::Result<T, E>;

/// pos error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum PosError {
    /// Generic error: {0}
    GenericError(String),
    /// EmptyCycleState: empty states on try to compute C+2 draws for cycle {0}
    EmptyCycleState(u64),
    /** CycleUnavailable: trying to get PoS draw rolls/seed for cycle {0}
    which is unavailable */
    CycleUnavailable(u64),
    /** CycleUnfinalised: trying to get PoS draw rolls/seed for cycle {0}
    which is not yet finalized */
    CycleUnfinalised(u64),
    /** InitCycleUnavailable: trying to get PoS initial draw rolls/seed for
    negative cycle, which is unavailable */
    InitCycleUnavailable,
    /// EmptyContainerInconsistency: draw cum_sum is empty
    EmptyContainerInconsistency,
    /// CannotComputeSeed: could not seed RNG with computed seed
    CannotComputeSeed,
    /** Invalid number of initial rolls, {0} found and we expect a number >=
    loop to the loopback_cycle number {1} */
    InvalidInitialRolls(usize, usize),
    /// serde error
    SerdeError(#[from] serde_json::Error),
    /// Io error
    IoError(#[from] std::io::Error),
}
