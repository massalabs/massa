use displaydoc::Display;
use thiserror::Error;

/// pos result
pub type PosResult<T, E = PosError> = core::result::Result<T, E>;

/// pos error
#[non_exhaustive]
#[derive(Display, Error, Debug, Clone)]
pub enum PosError {
    /// Container inconsistency: {0}
    ContainerInconsistency(String),
    /// Invalid roll distribution: {0}
    InvalidRollDistribution(String),
    /// Overflow error: {0}
    OverflowError(String),
    /// CycleUnavailable: PoS cycle {0} is needed but is absent from cache
    CycleUnavailable(u64),
    /// CycleUnfinalised: PoS cycle {0} is needed but is not complete yet
    CycleUnfinalised(u64),
    /// Error while loading initial rolls file: {0}
    RollsFileLoadingError(String),
}
