use displaydoc::Display;
use thiserror::Error;

/// Proof-of-Stake result
pub type PosResult<T, E = PosError> = core::result::Result<T, E>;

/// Proof-of-Stake error
#[non_exhaustive]
#[derive(Display, Error, Debug, Clone)]
pub enum PosError {
    /// Container inconsistency: {0}
    ContainerInconsistency(String),
    /// Invalid roll distribution: {0}
    InvalidRollDistribution(String),
    /// Overflow error: {0}
    OverflowError(String),
    /// `CycleUnavailable`: PoS cycle {0} is needed but is absent from cache
    CycleUnavailable(u64),
    /// `CycleUnfinished`: PoS cycle {0} is needed but is not complete yet
    CycleUnfinished(u64),
    /// Error while loading initial rolls file: {0}
    RollsFileLoadingError(String),
    /// Error while loading initial deferred credits file: {0}
    DeferredCreditsFileLoadingError(String),
    /// Communication channel was down: {0}
    ChannelDown(String),
}
