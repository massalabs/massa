// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Definition and exports of the PoS types and errors.
//!
//! Define also the Selector worker that compute in background the draws for
//! the future cycles

#![warn(missing_docs)]
#![feature(let_chains)]

mod config;
mod controller_traits;
mod cycle_info;
mod deferred_credits;
mod error;
mod pos_changes;
mod pos_final_state;
mod settings;

pub use config::PoSConfig;

pub use controller_traits::Selection;
pub use controller_traits::SelectorController;
pub use controller_traits::SelectorManager;
pub use error::{PosError, PosResult};

pub use cycle_info::{
    CycleHistoryDeserializer, CycleHistorySerializer, CycleInfo, CycleInfoDeserializer,
    CycleInfoSerializer, ProductionStats,
};
pub use deferred_credits::{
    DeferredCredits, DeferredCreditsDeserializer, DeferredCreditsSerializer,
};
pub use pos_changes::{PoSChanges, PoSChangesDeserializer, PoSChangesSerializer};
// {ProductionStats,
//  ProductionStatsDeserializer, ProductionStatsSerializer, RollsDeserializer}
// pub(crate) use cycle_info::*;
// pub(crate) use deferred_credits::*;
// pub(crate) use pos_changes::*;
pub use pos_final_state::PoSFinalState;
pub use settings::SelectorConfig;

#[cfg(feature = "testing")]
pub(crate) mod test_exports;
