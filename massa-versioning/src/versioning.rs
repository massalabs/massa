use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

use machine::{machine, transitions};
use num_enum::{FromPrimitive, IntoPrimitive, TryFromPrimitive};
use parking_lot::RwLock;
use thiserror::Error;
use tracing::warn;

use massa_db::{DBBatch, MassaDB, MIP_STORE_PREFIX, STATE_CF, VERSIONING_CF};
use massa_models::error::ModelsError;
use massa_models::slot::Slot;
use massa_models::timeslots::get_block_slot_timestamp;
use massa_models::{amount::Amount, config::VERSIONING_THRESHOLD_TRANSITION_ACCEPTED};
use massa_serialization::{DeserializeError, Deserializer, SerializeError, Serializer};
use massa_time::MassaTime;

use crate::versioning_ser_der::{
    MipInfoDeserializer, MipInfoSerializer, MipStateDeserializer, MipStateSerializer,
};

/// Versioning component enum
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, FromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum MipComponent {
    // Address and KeyPair versions are directly related
    Address,
    KeyPair,
    Block,
    VM,
    #[doc(hidden)]
    #[num_enum(default)]
    __Nonexhaustive,
}

/// MIP info (name & versions & time range for a MIP)
#[derive(Clone, Debug)]
pub struct MipInfo {
    /// MIP name or descriptive name
    pub name: String,
    /// Network (or global) version (to be included in block header)
    pub version: u32,
    /// Components concerned by this versioning (e.g. a new Block version), and the associated component_version
    pub components: HashMap<MipComponent, u32>,
    /// a timestamp at which the version gains its meaning (e.g. announced in block header)
    pub start: MassaTime,
    /// a timestamp at the which the deployment is considered failed
    pub timeout: MassaTime,
    /// Once deployment has been locked, wait for this duration before deployment is considered active
    pub activation_delay: MassaTime,
}

// Need Ord / PartialOrd so it is properly sorted in BTreeMap

impl Ord for MipInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.start, &self.timeout).cmp(&(other.start, &other.timeout))
    }
}

impl PartialOrd for MipInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for MipInfo {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.version == other.version
            && self.components == other.components
            && self.start == other.start
            && self.timeout == other.timeout
            && self.activation_delay == other.activation_delay
    }
}

impl Eq for MipInfo {}

// Need to impl this manually otherwise clippy is angry :-P
impl Hash for MipInfo {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.version.hash(state);
        self.components.iter().for_each(|c| c.hash(state));
        self.start.hash(state);
        self.timeout.hash(state);
    }
}

machine!(
    /// State machine for a Versioning component that tracks the deployment state
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) enum ComponentState {
        /// Initial state
        Defined,
        /// Past start, can only go to LockedIn after the threshold is above a given value
        Started { pub(crate) threshold: Amount },
        /// Locked but wait for some time before going to active (to let users the time to upgrade)
        LockedIn { pub(crate) at: MassaTime },
        /// After LockedIn, deployment is considered successful (after activation delay)
        Active { pub(crate) at: MassaTime },
        /// Past the timeout, if LockedIn is not reach
        Failed,
    }
);

impl Default for ComponentState {
    fn default() -> Self {
        Self::Defined(Defined {})
    }
}

#[allow(missing_docs)]
#[derive(IntoPrimitive, Debug, Clone, Eq, PartialEq, TryFromPrimitive, PartialOrd, Ord)]
#[repr(u32)]
pub enum ComponentStateTypeId {
    Error = 0,
    Defined = 1,
    Started = 2,
    LockedIn = 3,
    Active = 4,
    Failed = 5,
}

impl From<&ComponentState> for ComponentStateTypeId {
    fn from(value: &ComponentState) -> Self {
        match value {
            ComponentState::Error => ComponentStateTypeId::Error,
            ComponentState::Defined(_) => ComponentStateTypeId::Defined,
            ComponentState::Started(_) => ComponentStateTypeId::Started,
            ComponentState::LockedIn(_) => ComponentStateTypeId::LockedIn,
            ComponentState::Active(_) => ComponentStateTypeId::Active,
            ComponentState::Failed(_) => ComponentStateTypeId::Failed,
        }
    }
}

/// A message to update the `ComponentState`
#[derive(Clone, Debug)]
pub struct Advance {
    /// from MipInfo.start
    pub start_timestamp: MassaTime,
    /// from MipInfo.timeout
    pub timeout: MassaTime,
    /// from MipInfo.activation_delay
    pub activation_delay: MassaTime,

    /// % of past blocks with this version
    pub threshold: Amount,
    /// Current time (timestamp)
    pub now: MassaTime,
}

// Need Ord / PartialOrd so it is properly sorted in BTreeMap

impl Ord for Advance {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.now).cmp(&other.now)
    }
}

impl PartialOrd for Advance {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Advance {
    fn eq(&self, other: &Self) -> bool {
        self.start_timestamp == other.start_timestamp
            && self.timeout == other.timeout
            && self.threshold == other.threshold
            && self.now == other.now
    }
}

impl Eq for Advance {}

transitions!(ComponentState,
    [
        (Defined, Advance) => [Defined, Started, Failed],
        (Started, Advance) => [Started, LockedIn, Failed],
        (LockedIn, Advance) => [LockedIn, Active],
        (Active, Advance) => Active,
        (Failed, Advance) => Failed
    ]
);

impl Defined {
    /// Update state from state Defined
    pub fn on_advance(self, input: Advance) -> ComponentState {
        match input.now {
            n if n >= input.timeout => ComponentState::failed(),
            n if n >= input.start_timestamp => ComponentState::started(Amount::zero()),
            _ => ComponentState::Defined(Defined {}),
        }
    }
}

impl Started {
    /// Update state from state Started
    pub fn on_advance(self, input: Advance) -> ComponentState {
        if input.now > input.timeout {
            return ComponentState::failed();
        }

        if input.threshold >= VERSIONING_THRESHOLD_TRANSITION_ACCEPTED {
            ComponentState::locked_in(input.now)
        } else {
            ComponentState::started(input.threshold)
        }
    }
}

impl LockedIn {
    /// Update state from state LockedIn ...
    pub fn on_advance(self, input: Advance) -> ComponentState {
        if input.now > self.at.saturating_add(input.activation_delay) {
            ComponentState::active(input.now)
        } else {
            ComponentState::locked_in(self.at)
        }
    }
}

impl Active {
    /// Update state (will always stay in state Active)
    pub fn on_advance(self, _input: Advance) -> Active {
        Active { at: self.at }
    }
}

impl Failed {
    /// Update state (will always stay in state Failed)
    pub fn on_advance(self, _input: Advance) -> Failed {
        Failed {}
    }
}

/// Wrapper of ComponentState (in order to keep state history)
#[derive(Debug, Clone, PartialEq)]
pub struct MipState {
    pub(crate) state: ComponentState,
    pub(crate) history: BTreeMap<Advance, ComponentStateTypeId>,
}

impl MipState {
    /// Create
    pub fn new(defined: MassaTime) -> Self {
        let state: ComponentState = Default::default(); // Default is Defined
        let state_id = ComponentStateTypeId::from(&state);
        // Build a 'dummy' advance msg for state Defined, this is to avoid using an
        // Option<Advance> in MipStateHistory::history
        let advance = Advance {
            start_timestamp: MassaTime::from_millis(0),
            timeout: MassaTime::from_millis(0),
            threshold: Default::default(),
            now: defined,
            activation_delay: MassaTime::from_millis(0),
        };

        let history = BTreeMap::from([(advance, state_id)]);
        Self { state, history }
    }

    /// Create a new state from an existing state - resulting state will be at state "Defined"
    pub fn reset_from(&self) -> Option<Self> {
        match self.history.first_key_value() {
            Some((advance, state_id)) if *state_id == ComponentStateTypeId::Defined => {
                Some(MipState::new(advance.now))
            }
            _ => None,
        }
    }

    /// Advance the state
    /// Can be called as multiple times as it will only store what changes the state in history
    pub fn on_advance(&mut self, input: &Advance) {
        let now = input.now;
        // Check that input.now is after last item in history
        // We don't want to go backward
        let is_forward = self
            .history
            .last_key_value()
            .map(|(adv, _)| adv.now < now)
            .unwrap_or(false);

        if is_forward {
            // machines crate (for state machine) does not support passing ref :-/
            let state = self.state.on_advance(input.clone());
            // Update history as well
            if state != self.state {
                let state_id = ComponentStateTypeId::from(&state);

                // Avoid storing too much things in history
                // Here we avoid storing for every threshold update
                if !(matches!(state, ComponentState::Started(Started { .. }))
                    && matches!(self.state, ComponentState::Started(Started { .. })))
                {
                    self.history.insert(input.clone(), state_id);
                }
                self.state = state;
            }
        }
    }

    /// Given a corresponding VersioningInfo, check if state is coherent
    /// it is coherent
    ///   if state can be at this position (e.g. can it be at state "Started" according to given time range)
    ///   if history is coherent with current state
    /// Return false for state == ComponentState::Error
    pub fn is_coherent_with(&self, versioning_info: &MipInfo) -> bool {
        // TODO: rename versioning_info -> mip_info

        // Always return false for state Error or if history is empty
        if matches!(&self.state, &ComponentState::Error) || self.history.is_empty() {
            return false;
        }

        // safe to unwrap (already tested if empty or not)
        let (initial_ts, initial_state_id) = self.history.first_key_value().unwrap();
        if *initial_state_id != ComponentStateTypeId::Defined {
            // self.history does not start with Defined -> (always) false
            return false;
        }

        // Build a new VersionStateHistory from initial state, replaying the whole history
        // but with given versioning info then compare
        let mut vsh = MipState::new(initial_ts.now);
        let mut advance_msg = Advance {
            start_timestamp: versioning_info.start,
            timeout: versioning_info.timeout,
            threshold: Amount::zero(),
            now: initial_ts.now,
            activation_delay: versioning_info.activation_delay,
        };

        for (adv, _state) in self.history.iter().skip(1) {
            advance_msg.now = adv.now;
            advance_msg.threshold = adv.threshold;
            vsh.on_advance(&advance_msg);
        }

        vsh == *self
    }

    /// Query state at given timestamp
    /// TODO: add doc for start & timeout parameter? why do we need them?
    pub fn state_at(
        &self,
        ts: MassaTime,
        start: MassaTime,
        timeout: MassaTime,
    ) -> Result<ComponentStateTypeId, StateAtError> {
        if self.history.is_empty() {
            return Err(StateAtError::EmptyHistory);
        }

        // Optim: this avoids iterating over history (cheap to retrieve first item)
        let first = self.history.first_key_value().unwrap(); // safe to unwrap
        if ts < first.0.now {
            // Before initial state
            return Err(StateAtError::BeforeInitialState(first.1.clone(), ts));
        }

        // At this point, we are >= the first state in history
        let mut lower_bound = None;
        let mut higher_bound = None;
        let mut is_after_last = false;

        // Optim: this avoids iterating over history (cheap to retrieve first item)
        let last = self.history.last_key_value().unwrap(); // safe to unwrap
        if ts > last.0.now {
            lower_bound = Some(last);
            is_after_last = true;
        }

        if !is_after_last {
            // We are in between two states in history, find bounds
            for (adv, state_id) in self.history.iter() {
                if adv.now <= ts {
                    lower_bound = Some((adv, state_id));
                }
                if adv.now >= ts && higher_bound.is_none() {
                    higher_bound = Some((adv, state_id));
                    break;
                }
            }
        }

        match (lower_bound, higher_bound) {
            (Some((_adv_1, st_id_1)), Some((_adv_2, _st_id_2))) => {
                // Between 2 states (e.g. between Defined && Started) -> return Defined
                Ok(st_id_1.clone())
            }
            (Some((adv, st_id)), None) => {
                // After the last state in history -> need to advance the state and return
                let threshold_for_transition = VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
                // Note: Please update this if MipState transitions change as it might not hold true
                if *st_id == ComponentStateTypeId::Started
                    && adv.threshold < threshold_for_transition
                    && ts < adv.timeout
                {
                    Err(StateAtError::Unpredictable)
                } else {
                    let msg = Advance {
                        start_timestamp: start,
                        timeout,
                        threshold: adv.threshold,
                        now: ts,
                        activation_delay: adv.activation_delay,
                    };
                    // Return the resulting state after advance
                    let state = self.state.on_advance(msg);
                    Ok(ComponentStateTypeId::from(&state))
                }
            }
            _ => {
                // 1. Before the first state in history: already covered
                // 2. None, None: already covered - empty history
                Err(StateAtError::EmptyHistory)
            }
        }
    }

    /// Return the time when state will go from LockedIn to Active, None if not already LockedIn
    pub fn activation_at(&self, mip_info: &MipInfo) -> Option<MassaTime> {
        match self.state {
            ComponentState::LockedIn(LockedIn { at }) => {
                Some(at.saturating_add(mip_info.activation_delay))
            }
            _ => None,
        }
    }
}

/// Error returned by MipStateHistory::state_at
#[allow(missing_docs)]
#[derive(Error, Debug, PartialEq)]
pub enum StateAtError {
    #[error("Initial state ({0:?}) only defined after timestamp: {1}")]
    BeforeInitialState(ComponentStateTypeId, MassaTime),
    #[error("Empty history, should never happen")]
    EmptyHistory,
    #[error("Cannot predict in the future (~ threshold not reach yet)")]
    Unpredictable,
}

// Store

/// Database for all MIP info
#[derive(Debug, Clone)]
pub struct MipStore(pub Arc<RwLock<MipStoreRaw>>);

impl MipStore {
    /// Retrieve the current network version to set in block header
    pub fn get_network_version_current(&self) -> u32 {
        let lock = self.0.read();
        let store = lock.deref();
        // Current version == last active
        store
            .store
            .iter()
            .rev()
            .find_map(|(k, v)| (matches!(v.state, ComponentState::Active(_))).then_some(k.version))
            .unwrap_or(0)
    }

    /// Retrieve the last active version at the given timestamp
    pub fn get_network_version_active_at(&self, ts: MassaTime) -> u32 {
        let lock = self.0.read();
        let store = lock.deref();
        store
            .store
            .iter()
            .rev()
            .find_map(|(k, v)| match v.state {
                ComponentState::Active(Active { at }) if at <= ts => Some(k.version),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// Retrieve the network version number to announce in block header
    /// return 0 is there is nothing to announce
    pub fn get_network_version_to_announce(&self) -> u32 {
        let lock = self.0.read();
        let store = lock.deref();
        // Announce the latest versioning info in Started / LockedIn state
        // Defined == Not yet ready to announce
        // Active == current version
        store
            .store
            .iter()
            .rev()
            .find_map(|(k, v)| {
                matches!(
                    &v.state,
                    &ComponentState::Started(_) | &ComponentState::LockedIn(_)
                )
                .then_some(k.version)
            })
            .unwrap_or(0)
    }

    pub fn update_network_version_stats(
        &mut self,
        slot_timestamp: MassaTime,
        network_versions: Option<(u32, u32)>,
    ) {
        let mut lock = self.0.write();
        lock.update_network_version_stats(slot_timestamp, network_versions);
    }

    #[allow(clippy::result_large_err)]
    pub fn update_with(
        &mut self,
        mip_store: &MipStore,
    ) -> Result<(Vec<MipInfo>, BTreeMap<MipInfo, MipState>), UpdateWithError> {
        let mut lock = self.0.write();
        let lock_other = mip_store.0.read();
        lock.update_with(lock_other.deref())
    }

    // GRPC

    /// Retrieve a list of MIP info with their corresponding state (as id) - used for grpc API
    pub fn get_mip_status(&self) -> BTreeMap<MipInfo, ComponentStateTypeId> {
        let guard = self.0.read();
        guard
            .store
            .iter()
            .map(|(mip_info, mip_state)| {
                (
                    mip_info.clone(),
                    ComponentStateTypeId::from(&mip_state.state),
                )
            })
            .collect()
    }

    // Network restart
    pub fn is_coherent_with_shutdown_period(
        &self,
        shutdown_start: Slot,
        shutdown_end: Slot,
        thread_count: u8,
        t0: MassaTime,
        genesis_timestamp: MassaTime,
    ) -> Result<bool, ModelsError> {
        let guard = self.0.read();
        guard.is_coherent_with_shutdown_period(
            shutdown_start,
            shutdown_end,
            thread_count,
            t0,
            genesis_timestamp,
        )
    }

    // DB
    pub fn update_batches(
        &self,
        db_batch: &mut DBBatch,
        db_versioning_batch: &mut DBBatch,
        between: (&MassaTime, &MassaTime),
    ) -> Result<(), SerializeError> {
        let guard = self.0.read();
        guard.update_batches(db_batch, db_versioning_batch, between)
    }

    pub fn extend_from_db(&mut self, db: Arc<RwLock<MassaDB>>) -> Result<(), ExtendFromDbError> {
        let mut guard = self.0.write();
        guard.extend_from_db(db)
    }

    pub fn reset_db(&self, db: Arc<RwLock<MassaDB>>) {
        {
            let mut guard = db.write();
            guard.delete_prefix(MIP_STORE_PREFIX, STATE_CF, None);
            guard.delete_prefix(MIP_STORE_PREFIX, VERSIONING_CF, None);
        }
    }
}

impl<const N: usize> TryFrom<([(MipInfo, MipState); N], MipStatsConfig)> for MipStore {
    type Error = ();

    fn try_from(
        (value, cfg): ([(MipInfo, MipState); N], MipStatsConfig),
    ) -> Result<Self, Self::Error> {
        MipStoreRaw::try_from((value, cfg)).map(|store_raw| Self(Arc::new(RwLock::new(store_raw))))
    }
}

/// Statistics in MipStoreRaw
#[derive(Debug, Clone, PartialEq)]
pub struct MipStatsConfig {
    pub block_count_considered: usize,
    pub counters_max: usize,
}

/// In order for a MIP to be accepted, we compute statistics about other node 'network' version announcement
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MipStoreStats {
    pub(crate) config: MipStatsConfig,
    pub(crate) latest_announcements: VecDeque<u32>,
    pub(crate) network_version_counters: BTreeMap<u32, u64>,
}

impl MipStoreStats {
    pub(crate) fn new(config: MipStatsConfig) -> Self {
        Self {
            config: config.clone(),
            latest_announcements: VecDeque::with_capacity(config.block_count_considered),
            network_version_counters: Default::default(),
        }
    }

    // reset stats - used in `update_for_network_shutdown` function
    fn reset(&mut self) {
        self.latest_announcements.clear();
        self.network_version_counters.clear();
    }
}

/// Error returned by
#[derive(Error, Debug, PartialEq)]
pub enum UpdateWithError {
    // State is not coherent with associated MipInfo, ex: State is active but MipInfo.start was not reach yet
    #[error("MipInfo {0:?} is not coherent with state: {1:?}")]
    NonCoherent(MipInfo, MipState),
    // ex: State is already started but received state is only defined
    #[error("For MipInfo {0:?}, trying to downgrade from state {1:?} to {2:?}")]
    Downgrade(MipInfo, ComponentState, ComponentState),
    // ex: MipInfo 2 start is before MipInfo 1 timeout (MipInfo timings should only be sequential)
    #[error("MipInfo {0:?} has overlapping data of MipInfo {1:?}")]
    Overlapping(MipInfo, MipInfo),
}

/// Error returned by 'extend_from_db`
#[derive(Error, Debug)]
pub enum ExtendFromDbError {
    #[error("Unable to get an handle over db column: {0}")]
    UnknownDbColumn(String),
    #[error("{0}")]
    Update(#[from] UpdateWithError),
    #[error("{0}")]
    Deserialize(String),
}

/// Store of all versioning info
#[derive(Debug, Clone, PartialEq)]
pub struct MipStoreRaw {
    pub(crate) store: BTreeMap<MipInfo, MipState>,
    pub(crate) stats: MipStoreStats,
}

impl MipStoreRaw {
    /// Update our store with another (usually after a bootstrap where we received another store)
    /// Return list of updated / added if successful, UpdateWithError otherwise
    #[allow(clippy::result_large_err)]
    pub fn update_with(
        &mut self,
        store_raw: &MipStoreRaw,
    ) -> Result<(Vec<MipInfo>, BTreeMap<MipInfo, MipState>), UpdateWithError> {
        // iter over items in given store:
        // -> 2 cases:
        // * MipInfo is already in self store -> add to 'to_update' list
        // * MipInfo is not in self.store -> We received a new MipInfo so we are running an out dated version
        //                                   of the software
        //                                   We then return the list of new MipInfo so we can warn and ask
        //                                   to update the software

        let mut component_versions: HashMap<MipComponent, u32> = self
            .store
            .iter()
            .flat_map(|c| {
                c.0.components
                    .iter()
                    .map(|(mip_component, component_version)| {
                        (mip_component.clone(), *component_version)
                    })
            })
            .collect();
        let mut names: BTreeSet<String> = self.store.iter().map(|i| i.0.name.clone()).collect();
        let mut to_update: BTreeMap<MipInfo, MipState> = Default::default();
        let mut to_add: BTreeMap<MipInfo, MipState> = Default::default();
        let mut has_error: Option<UpdateWithError> = None;

        for (v_info, v_state) in store_raw.store.iter() {
            if !v_state.is_coherent_with(v_info) {
                // As soon as we found one non coherent state we abort the merge
                has_error = Some(UpdateWithError::NonCoherent(
                    v_info.clone(),
                    v_state.clone(),
                ));
                break;
            }

            if let Some(v_state_orig) = self.store.get(v_info) {
                // Versioning info (from right) is already in self (left)
                // Need to check if we add this to 'to_update' list
                let v_state_id: u32 = ComponentStateTypeId::from(&v_state.state).into();
                let v_state_orig_id: u32 = ComponentStateTypeId::from(&v_state_orig.state).into();

                // Note: we do not check for state: active OR failed OR error as they cannot change
                if matches!(
                    v_state_orig.state,
                    ComponentState::Defined(_)
                        | ComponentState::Started(_)
                        | ComponentState::LockedIn(_)
                ) {
                    // Only accept 'higher' state
                    // (e.g. 'started' if 'defined', 'locked in' if 'started'...)
                    if v_state_id >= v_state_orig_id {
                        to_update.insert(v_info.clone(), v_state.clone());
                    } else {
                        // Trying to downgrade state' (e.g. trying to go from 'active' -> 'defined')
                        has_error = Some(UpdateWithError::Downgrade(
                            v_info.clone(),
                            v_state_orig.state,
                            v_state.state,
                        ));
                        break;
                    }
                }
            } else {
                // Versioning info (from right) is not in self.0 (left)
                // Need to check if we add this to 'to_add' list

                let last_v_info_ = to_add
                    .last_key_value()
                    .map(|i| i.0)
                    .or(self.store.last_key_value().map(|i| i.0));

                if let Some(last_v_info) = last_v_info_ {
                    // check for versions of all components in v_info
                    let mut component_version_compatible = true;
                    for component in v_info.components.iter() {
                        if component.1 <= component_versions.get(component.0).unwrap_or(&0) {
                            component_version_compatible = false;
                            break;
                        }
                    }

                    if v_info.start > last_v_info.timeout
                        && v_info.timeout > v_info.start
                        && v_info.version > last_v_info.version
                        && !names.contains(&v_info.name)
                        && component_version_compatible
                    {
                        // Time range is ok / version is ok / name is unique, let's add it
                        to_add.insert(v_info.clone(), v_state.clone());
                        names.insert(v_info.name.clone());
                        for component in v_info.components.iter() {
                            component_versions.insert(component.0.clone(), *component.1);
                        }
                    } else {
                        // Something is wrong (time range not ok? / version not incr? / names?
                        // or component version not incr?)
                        has_error = Some(UpdateWithError::Overlapping(
                            v_info.clone(),
                            last_v_info.clone(),
                        ));
                        break;
                    }
                } else {
                    // to_add is empty && self.0 is empty
                    to_add.insert(v_info.clone(), v_state.clone());
                    names.insert(v_info.name.clone());
                }
            }
        }

        match has_error {
            None => {
                let updated: Vec<MipInfo> = to_update.keys().cloned().collect();

                // Note: we only update the store with to_update collection
                //       having something in the to_add collection means that we need to update
                //       the Massa node software
                self.store.append(&mut to_update);
                Ok((updated, to_add))
            }
            Some(e) => Err(e),
        }
    }

    fn update_network_version_stats(
        &mut self,
        slot_timestamp: MassaTime,
        network_versions: Option<(u32, u32)>,
    ) {
        if let Some((_current_network_version, announced_network_version)) = network_versions {
            let removed_version_ = match self.stats.latest_announcements.len() {
                n if n > self.stats.config.block_count_considered => {
                    self.stats.latest_announcements.pop_front()
                }
                _ => None,
            };
            self.stats
                .latest_announcements
                .push_back(announced_network_version);

            // We update the count of the received version
            *self
                .stats
                .network_version_counters
                .entry(announced_network_version)
                .or_default() += 1;

            if let Some(removed_version) = removed_version_ {
                *self
                    .stats
                    .network_version_counters
                    .entry(removed_version)
                    .or_insert(1) -= 1;
            }

            // Cleanup for the counters
            if self.stats.network_version_counters.len() > self.stats.config.counters_max {
                if let Some((version, count)) = self.stats.network_version_counters.pop_first() {
                    // TODO: return version / count for unit tests?
                    warn!(
                        "MipStoreStats removed counter for version {}, count was: {}",
                        version, count
                    )
                }
            }

            self.advance_states_on_updated_stats(slot_timestamp);
        }
    }

    /// Used internally by `update_network_version_stats`
    fn advance_states_on_updated_stats(&mut self, slot_timestamp: MassaTime) {
        for (mi, state) in self.store.iter_mut() {
            let network_version_count = *self
                .stats
                .network_version_counters
                .get(&mi.version)
                .unwrap_or(&0) as f32;
            let block_count_considered = self.stats.config.block_count_considered as f32;

            let vote_ratio_ = 100.0 * network_version_count / block_count_considered;

            let vote_ratio = Amount::from_mantissa_scale(vote_ratio_.round() as u64, 0);

            let advance_msg = Advance {
                start_timestamp: mi.start,
                timeout: mi.timeout,
                threshold: vote_ratio,
                now: slot_timestamp,
                activation_delay: mi.activation_delay,
            };

            // TODO / OPTIM: filter the store to avoid advancing on failed and active versions
            state.on_advance(&advance_msg.clone());
        }
    }

    /// Check if store is coherent with given last network shutdown
    /// On a network shutdown, the MIP infos will be edited but we still need to check if this is coherent
    fn is_coherent_with_shutdown_period(
        &self,
        shutdown_start: Slot,
        shutdown_end: Slot,
        thread_count: u8,
        t0: MassaTime,
        genesis_timestamp: MassaTime,
    ) -> Result<bool, ModelsError> {
        let mut is_coherent = true;

        let shutdown_start_ts =
            get_block_slot_timestamp(thread_count, t0, genesis_timestamp, shutdown_start)?;
        let shutdown_end_ts =
            get_block_slot_timestamp(thread_count, t0, genesis_timestamp, shutdown_end)?;
        let shutdown_range = shutdown_start_ts..=shutdown_end_ts;

        for (mip_info, mip_state) in &self.store {
            match mip_state.state {
                ComponentState::Defined(..) => {
                    // all good if it does not start / timeout during shutdown period
                    if shutdown_range.contains(&mip_info.start)
                        || shutdown_range.contains(&mip_info.timeout)
                    {
                        is_coherent = false;
                        break;
                    }
                }
                ComponentState::Started(..) => {
                    // assume this should have been reset
                    is_coherent = false;
                    break;
                }
                _ => {
                    // active / failed, error, nothing to do
                    // locked in, nothing to do (might go from 'locked in' to 'active' during shutdown)
                }
            }
        }

        Ok(is_coherent)
    }

    #[allow(dead_code)]
    fn update_for_network_shutdown(
        &mut self,
        shutdown_start: Slot,
        shutdown_end: Slot,
        thread_count: u8,
        t0: MassaTime,
        genesis_timestamp: MassaTime,
    ) -> Result<(), ModelsError> {
        let shutdown_start_ts =
            get_block_slot_timestamp(thread_count, t0, genesis_timestamp, shutdown_start)?;
        let shutdown_end_ts =
            get_block_slot_timestamp(thread_count, t0, genesis_timestamp, shutdown_end)?;
        let shutdown_range = shutdown_start_ts..=shutdown_end_ts;

        let mut new_store: BTreeMap<MipInfo, MipState> = Default::default();
        let mut new_stats = self.stats.clone();
        new_stats.reset();

        let next_valid_start_ = shutdown_end.get_next_slot(thread_count)?;
        let next_valid_start =
            get_block_slot_timestamp(thread_count, t0, genesis_timestamp, next_valid_start_)?;

        let mut offset: Option<MassaTime> = None;

        for (mip_info, mip_state) in &self.store {
            match mip_state.state {
                ComponentState::Defined(..) => {
                    // Defined: offset start & timeout

                    let mut new_mip_info = mip_info.clone();

                    if shutdown_range.contains(&new_mip_info.start) {
                        let offset_ts = match offset {
                            Some(offset_ts) => offset_ts,
                            None => {
                                let offset_ts = next_valid_start.saturating_sub(mip_info.start);
                                offset = Some(offset_ts);
                                offset_ts
                            }
                        };

                        new_mip_info.start = new_mip_info.start.saturating_add(offset_ts);
                        new_mip_info.timeout = new_mip_info
                            .start
                            .saturating_add(mip_info.timeout.saturating_sub(mip_info.start));
                    }
                    new_store.insert(new_mip_info, mip_state.clone());
                }
                ComponentState::Started(..) => {
                    // Started -> Reset to Defined, offset start & timeout

                    let mut new_mip_info = mip_info.clone();

                    let offset_ts = match offset {
                        Some(offset_ts) => offset_ts,
                        None => {
                            let offset_ts = next_valid_start.saturating_sub(mip_info.start);
                            offset = Some(offset_ts);
                            offset_ts
                        }
                    };

                    new_mip_info.start = new_mip_info.start.saturating_add(offset_ts);
                    new_mip_info.timeout = new_mip_info
                        .start
                        .saturating_add(mip_info.timeout.saturating_sub(mip_info.start));

                    // Need to reset state to 'Defined'
                    let new_mip_state = MipState::reset_from(mip_state)
                        .ok_or(ModelsError::from("Unable to reset state"))?;
                    // Note: statistics are already reset
                    new_store.insert(new_mip_info, new_mip_state.clone());
                }
                _ => {
                    // active / failed, error, nothing to do
                    // locked in, nothing to do (might go from 'locked in' to 'active' during shutdown)
                    new_store.insert(mip_info.clone(), mip_state.clone());
                }
            }
        }

        self.store = new_store;
        self.stats = new_stats;
        Ok(())
    }

    // DB methods
    fn update_batches(
        &self,
        batch: &mut DBBatch,
        versioning_batch: &mut DBBatch,
        between: (&MassaTime, &MassaTime),
    ) -> Result<(), SerializeError> {
        let mip_info_ser = MipInfoSerializer::new();
        let mip_state_ser = MipStateSerializer::new();

        let bounds = (*between.0)..=(*between.1);
        let mut key = Vec::new();
        let mut value = Vec::new();

        for (mip_info, mip_state) in self.store.iter() {
            if let Some((advance, state_id)) = mip_state.history.last_key_value() {
                if bounds.contains(&advance.now) {
                    key.extend(MIP_STORE_PREFIX.as_bytes().to_vec());
                    mip_info_ser.serialize(mip_info, &mut key)?;
                    mip_state_ser.serialize(mip_state, &mut value)?;
                    match state_id {
                        ComponentStateTypeId::Active => {
                            batch.insert(key.clone(), Some(value.clone()));
                        }
                        _ => {
                            versioning_batch.insert(key.clone(), Some(value.clone()));
                        }
                    }
                    key.clear();
                    value.clear();
                }
            }
        }

        Ok(())
    }

    fn extend_from_db(&mut self, db: Arc<RwLock<MassaDB>>) -> Result<(), ExtendFromDbError> {
        let mip_info_deser = MipInfoDeserializer::new();
        let mip_state_deser = MipStateDeserializer::new();

        let db = db.read();
        let handle = db
            .db
            .cf_handle(STATE_CF)
            .ok_or(ExtendFromDbError::UnknownDbColumn(STATE_CF.to_string()))?;

        // Get data from state cf handle
        let mut update_data: BTreeMap<MipInfo, MipState> = Default::default();
        for (ser_mip_info, ser_mip_state) in
            db.db.prefix_iterator_cf(handle, MIP_STORE_PREFIX).flatten()
        {
            if !ser_mip_info.starts_with(MIP_STORE_PREFIX.as_bytes()) {
                break;
            }

            // deser
            let (_, mip_info) = mip_info_deser
                .deserialize::<DeserializeError>(&ser_mip_info[MIP_STORE_PREFIX.len()..])
                .map_err(|e| ExtendFromDbError::Deserialize(e.to_string()))?;

            let (_, mip_state) = mip_state_deser
                .deserialize::<DeserializeError>(&ser_mip_state)
                .map_err(|e| ExtendFromDbError::Deserialize(e.to_string()))?;

            update_data.insert(mip_info, mip_state);
        }

        // FIXME: stats
        let store_raw_ = MipStoreRaw {
            store: update_data,
            stats: MipStoreStats {
                config: MipStatsConfig {
                    block_count_considered: 5,
                    counters_max: 10,
                },
                latest_announcements: Default::default(),
                network_version_counters: Default::default(),
            },
        };
        let (_updated, _added) = self
            .update_with(&store_raw_)
            // .expect("update with state cf data");
            ?;

        let mut update_data: BTreeMap<MipInfo, MipState> = Default::default();
        let versioning_handle = db
            .db
            .cf_handle(VERSIONING_CF)
            // .expect(CF_ERROR);
            .ok_or(ExtendFromDbError::UnknownDbColumn(
                VERSIONING_CF.to_string(),
            ))?;

        // Get data from state cf handle
        for (ser_mip_info, ser_mip_state) in db
            .db
            .prefix_iterator_cf(versioning_handle, MIP_STORE_PREFIX)
            .flatten()
        {
            // deser
            if !ser_mip_info.starts_with(MIP_STORE_PREFIX.as_bytes()) {
                break;
            }

            let (_, mip_info) = mip_info_deser
                .deserialize::<DeserializeError>(&ser_mip_info[MIP_STORE_PREFIX.len()..])
                .map_err(|e| ExtendFromDbError::Deserialize(e.to_string()))?;

            let (_, mip_state) = mip_state_deser
                .deserialize::<DeserializeError>(&ser_mip_state)
                .map_err(|e| ExtendFromDbError::Deserialize(e.to_string()))?;

            update_data.insert(mip_info, mip_state);
        }

        // FIXME: stats
        let store_raw_ = MipStoreRaw {
            store: update_data,
            stats: MipStoreStats {
                config: MipStatsConfig {
                    block_count_considered: 5,
                    counters_max: 10,
                },
                latest_announcements: Default::default(),
                network_version_counters: Default::default(),
            },
        };
        self.update_with(&store_raw_)?;
        Ok(())
    }
}

impl<const N: usize> TryFrom<([(MipInfo, MipState); N], MipStatsConfig)> for MipStoreRaw {
    type Error = ();

    fn try_from(
        (value, cfg): ([(MipInfo, MipState); N], MipStatsConfig),
    ) -> Result<Self, Self::Error> {
        // Build an empty store
        let mut store = Self {
            store: Default::default(),
            stats: MipStoreStats::new(cfg.clone()),
        };

        // Build another one with given value
        let other_store = Self {
            store: BTreeMap::from(value),
            stats: MipStoreStats::new(cfg),
        };

        // Use update_with ensuring that we have no overlapping time range, unique names & ...
        match store.update_with(&other_store) {
            Ok((_updated, mut added)) => {
                store.store.append(&mut added);
                Ok(store)
            }
            Err(_) => Err(()),
        }
    }
}

// End Store

#[cfg(test)]
mod test {
    use super::*;
    use std::assert_matches::assert_matches;

    use std::str::FromStr;

    use chrono::{Days, NaiveDate, NaiveDateTime};
    use massa_db::MassaDBConfig;
    use tempfile::tempdir;

    use crate::test_helpers::versioning_helpers::advance_state_until;

    use massa_models::config::{
        MIP_STORE_STATS_BLOCK_CONSIDERED, MIP_STORE_STATS_COUNTERS_MAX, T0, THREAD_COUNT,
    };
    use massa_models::timeslots::get_closest_slot_to_timestamp;

    // Only for unit tests
    impl PartialEq<ComponentState> for MipState {
        fn eq(&self, other: &ComponentState) -> bool {
            self.state == *other
        }
    }

    impl From<(&MipInfo, &Amount, &MassaTime)> for Advance {
        fn from((mip_info, threshold, now): (&MipInfo, &Amount, &MassaTime)) -> Self {
            Self {
                start_timestamp: mip_info.start,
                timeout: mip_info.timeout,
                threshold: *threshold,
                now: *now,
                activation_delay: mip_info.activation_delay,
            }
        }
    }

    fn get_a_version_info() -> (NaiveDateTime, NaiveDateTime, MipInfo) {
        // A helper function to provide a  default VersioningInfo
        // Models a Massa Improvements Proposal (MIP-0002), transitioning component address to v2

        let start: NaiveDateTime = NaiveDate::from_ymd_opt(2017, 11, 01)
            .unwrap()
            .and_hms_opt(7, 33, 44)
            .unwrap();

        let timeout: NaiveDateTime = NaiveDate::from_ymd_opt(2017, 11, 11)
            .unwrap()
            .and_hms_opt(7, 33, 44)
            .unwrap();

        return (
            start,
            timeout,
            MipInfo {
                name: "MIP-0002".to_string(),
                version: 2,
                components: HashMap::from([(MipComponent::Address, 1)]),
                start: MassaTime::from_millis(start.timestamp() as u64),
                timeout: MassaTime::from_millis(timeout.timestamp() as u64),
                activation_delay: MassaTime::from_millis(20),
            },
        );
    }

    #[test]
    fn test_state_advance_from_defined() {
        // Test Versioning state transition (from state: Defined)
        let (_, _, mi) = get_a_version_info();
        let mut state: ComponentState = Default::default();
        assert_eq!(state, ComponentState::defined());

        let now = mi.start.saturating_sub(MassaTime::from_millis(1));
        let mut advance_msg = Advance::from((&mi, &Amount::zero(), &now));

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::defined());

        let now = mi.start.saturating_add(MassaTime::from_millis(5));
        advance_msg.now = now;
        state = state.on_advance(advance_msg);

        // println!("state: {:?}", state);
        assert_eq!(
            state,
            ComponentState::Started(Started {
                threshold: Amount::zero()
            })
        );
    }

    #[test]
    fn test_state_advance_from_started() {
        // Test Versioning state transition (from state: Started)
        let (_, _, mi) = get_a_version_info();
        let mut state: ComponentState = ComponentState::started(Default::default());

        let now = mi.start;
        let threshold_too_low = Amount::from_str("74.9").unwrap();
        let threshold_ok = Amount::from_str("82.42").unwrap();
        let mut advance_msg = Advance::from((&mi, &threshold_too_low, &now));

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::started(threshold_too_low));
        advance_msg.threshold = threshold_ok;
        state = state.on_advance(advance_msg);
        assert_eq!(state, ComponentState::locked_in(now));
    }

    #[test]
    fn test_state_advance_from_locked_in() {
        // Test Versioning state transition (from state: LockedIn)
        let (_, _, mi) = get_a_version_info();

        let locked_in_at = mi.start.saturating_add(MassaTime::from_millis(1));
        let mut state: ComponentState = ComponentState::locked_in(locked_in_at);

        let now = mi.start;
        let mut advance_msg = Advance::from((&mi, &Amount::zero(), &now));

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::locked_in(locked_in_at));

        advance_msg.now = advance_msg
            .timeout
            .saturating_add(MassaTime::from_millis(1));
        state = state.on_advance(advance_msg);
        assert!(matches!(state, ComponentState::Active(_)));
    }

    #[test]
    fn test_state_advance_from_active() {
        // Test Versioning state transition (from state: Active)
        let (start, _, mi) = get_a_version_info();
        let mut state = ComponentState::active(MassaTime::from_millis(start.timestamp() as u64));
        let now = mi.start;
        let advance = Advance::from((&mi, &Amount::zero(), &now));

        state = state.on_advance(advance);
        assert!(matches!(state, ComponentState::Active(_)));
    }

    #[test]
    fn test_state_advance_from_failed() {
        // Test Versioning state transition (from state: Failed)
        let (_, _, mi) = get_a_version_info();
        let mut state = ComponentState::failed();
        let now = mi.start;
        let advance = Advance::from((&mi, &Amount::zero(), &now));
        state = state.on_advance(advance);
        assert_eq!(state, ComponentState::failed());
    }

    #[test]
    fn test_state_advance_to_failed() {
        // Test Versioning state transition (to state: Failed)
        let (_, _, mi) = get_a_version_info();
        let now = mi.timeout.saturating_add(MassaTime::from_millis(1));
        let advance_msg = Advance::from((&mi, &Amount::zero(), &now));

        let mut state: ComponentState = Default::default(); // Defined
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::Failed(Failed {}));

        let mut state = ComponentState::started(Default::default());
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::Failed(Failed {}));
    }

    #[test]
    fn test_state_with_history() {
        // Test MipStateHistory::state_at() function

        let (start, _, mi) = get_a_version_info();
        let now_0 = MassaTime::from_millis(start.timestamp() as u64);
        let mut state = MipState::new(now_0);

        assert_eq!(state, ComponentState::defined());

        let now = mi.start.saturating_add(MassaTime::from_millis(15));
        let mut advance_msg = Advance::from((&mi, &Amount::zero(), &now));

        // Move from Defined -> Started
        state.on_advance(&advance_msg);
        assert_eq!(state, ComponentState::started(Amount::zero()));

        // Check history
        assert_eq!(state.history.len(), 2);
        assert!(matches!(
            state.history.first_key_value(),
            Some((&Advance { .. }, &ComponentStateTypeId::Defined))
        ));
        assert!(matches!(
            state.history.last_key_value(),
            Some((&Advance { .. }, &ComponentStateTypeId::Started))
        ));

        // Query with timestamp

        // Before Defined
        let state_id_ = state.state_at(
            mi.start.saturating_sub(MassaTime::from_millis(5)),
            mi.start,
            mi.timeout,
        );
        assert!(matches!(
            state_id_,
            Err(StateAtError::BeforeInitialState(_, _))
        ));
        // After Defined timestamp
        let state_id = state.state_at(mi.start, mi.start, mi.timeout).unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Defined);
        // At Started timestamp
        let state_id = state.state_at(now, mi.start, mi.timeout).unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Started);

        // After Started timestamp but before timeout timestamp
        let after_started_ts = now.saturating_add(MassaTime::from_millis(15));
        let state_id_ = state.state_at(after_started_ts, mi.start, mi.timeout);
        assert_eq!(state_id_, Err(StateAtError::Unpredictable));

        // After Started timestamp and after timeout timestamp
        let after_timeout_ts = mi.timeout.saturating_add(MassaTime::from_millis(15));
        let state_id = state
            .state_at(after_timeout_ts, mi.start, mi.timeout)
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Failed);

        // Move from Started to LockedIn
        let threshold = VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
        advance_msg.threshold = threshold.saturating_add(Amount::from_str("1.0").unwrap());
        advance_msg.now = now.saturating_add(MassaTime::from_millis(1));
        state.on_advance(&advance_msg);
        assert_eq!(state, ComponentState::locked_in(advance_msg.now));

        // Query with timestamp
        // After LockedIn timestamp and before timeout timestamp
        let after_locked_in_ts = now.saturating_add(MassaTime::from_millis(10));
        let state_id = state
            .state_at(after_locked_in_ts, mi.start, mi.timeout)
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::LockedIn);
        // After LockedIn timestamp and after timeout timestamp
        let state_id = state
            .state_at(after_timeout_ts, mi.start, mi.timeout)
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Active);
    }

    #[test]
    fn test_versioning_store_announce_current() {
        // Test VersioningInfo::get_version_to_announce() & ::get_version_current()

        let (start, timeout, mi) = get_a_version_info();

        let mut mi_2 = mi.clone();
        mi_2.version += 1;
        mi_2.start = MassaTime::from_millis(
            timeout.checked_add_days(Days::new(2)).unwrap().timestamp() as u64,
        );
        mi_2.timeout = MassaTime::from_millis(
            timeout.checked_add_days(Days::new(5)).unwrap().timestamp() as u64,
        );

        // Can only build such object in test - history is empty :-/
        let vs_1 = MipState {
            state: ComponentState::active(MassaTime::from_millis(start.timestamp() as u64)),
            history: Default::default(),
        };
        let vs_2 = MipState {
            state: ComponentState::started(Amount::zero()),
            history: Default::default(),
        };

        // TODO: Have VersioningStore::from ?
        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };
        let vs_raw = MipStoreRaw {
            store: BTreeMap::from([(mi.clone(), vs_1), (mi_2.clone(), vs_2)]),
            stats: MipStoreStats::new(mip_stats_cfg.clone()),
        };
        // let vs_raw = MipStoreRaw::try_from([(vi.clone(), vs_1), (vi_2.clone(), vs_2)]).unwrap();
        let vs = MipStore(Arc::new(RwLock::new(vs_raw)));

        assert_eq!(vs.get_network_version_current(), mi.version);
        assert_eq!(vs.get_network_version_to_announce(), mi_2.version);

        // Test also an empty versioning store
        let vs_raw = MipStoreRaw {
            store: Default::default(),
            stats: MipStoreStats::new(mip_stats_cfg),
        };
        let vs = MipStore(Arc::new(RwLock::new(vs_raw)));
        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), 0);
    }

    #[test]
    fn test_is_coherent_with() {
        // Test MipStateHistory::is_coherent_with

        // Given the following versioning info, we expect state
        // Defined @ time <= 2
        // Started @ time > 2 && <= 5
        // LockedIn @ time > time(Started) && <= 5
        // Active @time > 5
        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(2),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };
        // Another versioning info (from an attacker) for testing
        let vi_2 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(7),
            timeout: MassaTime::from_millis(10),
            activation_delay: MassaTime::from_millis(2),
        };

        let vsh = MipState {
            state: ComponentState::Error,
            history: Default::default(),
        };
        // At state Error -> (always) false
        assert_eq!(vsh.is_coherent_with(&vi_1), false);

        let vsh = MipState {
            state: ComponentState::defined(),
            history: Default::default(),
        };
        // At state Defined but no history -> false
        assert_eq!(vsh.is_coherent_with(&vi_1), false);

        let mut vsh = MipState::new(MassaTime::from_millis(1));
        // At state Defined at time 1 -> true, given vi_1 @ time 1
        assert_eq!(vsh.is_coherent_with(&vi_1), true);
        // At state Defined at time 1 -> false given vi_1 @ time 3 (state should be Started)
        // assert_eq!(vsh.is_coherent_with(&vi_1, MassaTime::from_millis(3)), false);

        // Advance to Started
        let now = MassaTime::from_millis(3);
        let adv = Advance::from((&vi_1, &Amount::zero(), &now));
        vsh.on_advance(&adv);

        // At state Started at time now -> true
        assert_eq!(vsh.state, ComponentState::started(Amount::zero()));
        assert_eq!(vsh.is_coherent_with(&vi_1), true);
        // Now with another versioning info
        assert_eq!(vsh.is_coherent_with(&vi_2), false);

        // Advance to LockedIn
        let now = MassaTime::from_millis(4);
        let adv = Advance::from((&vi_1, &VERSIONING_THRESHOLD_TRANSITION_ACCEPTED, &now));
        vsh.on_advance(&adv);

        // At state LockedIn at time now -> true
        assert_eq!(vsh.state, ComponentState::locked_in(now));
        assert_eq!(vsh.is_coherent_with(&vi_1), true);
        assert_eq!(vsh.is_coherent_with(&vi_1), true);

        // edge cases
        // TODO: history all good but does not start with Defined, start with Started
    }

    #[test]
    fn test_merge_with() {
        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(2),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };

        let _time = MassaTime::now().unwrap();
        let vs_1 = advance_state_until(ComponentState::active(_time), &vi_1);
        assert!(matches!(vs_1.state, ComponentState::Active(_)));

        let vi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: HashMap::from([(MipComponent::Address, 2)]),
            start: MassaTime::from_millis(17),
            timeout: MassaTime::from_millis(27),
            activation_delay: MassaTime::from_millis(2),
        };
        let vs_2 = advance_state_until(ComponentState::defined(), &vi_2);

        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };
        let mut vs_raw_1 = MipStoreRaw::try_from((
            [(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())],
            mip_stats_cfg.clone(),
        ))
        .unwrap();

        let vs_2_2 = advance_state_until(ComponentState::active(_time), &vi_2);
        assert!(matches!(vs_2_2.state, ComponentState::Active(_)));

        let vs_raw_2 = MipStoreRaw::try_from((
            [(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2_2.clone())],
            mip_stats_cfg,
        ))
        .unwrap();

        println!("update with:");
        let (updated, added) = vs_raw_1.update_with(&vs_raw_2).unwrap();

        // Check update_with result
        assert!(added.is_empty());
        assert_eq!(updated, vec![vi_2.clone()]);

        // Expect state 1 (for vi_1) no change, state 2 (for vi_2) updated to "Active"
        assert_eq!(vs_raw_1.store.get(&vi_1).unwrap().state, vs_1.state);
        assert_eq!(vs_raw_1.store.get(&vi_2).unwrap().state, vs_2_2.state);
    }

    #[test]
    fn test_merge_with_invalid() {
        // Test updating a versioning store with another invalid:
        // 1- overlapping time range
        // 2- overlapping versioning component

        // part 0 - defines data for the test
        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(0),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };
        let _time = MassaTime::now().unwrap();
        let vs_1 = advance_state_until(ComponentState::active(_time), &vi_1);
        assert!(matches!(vs_1.state, ComponentState::Active(_)));

        let vi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: HashMap::from([(MipComponent::Address, 2)]),
            start: MassaTime::from_millis(17),
            timeout: MassaTime::from_millis(27),
            activation_delay: MassaTime::from_millis(2),
        };
        let vs_2 = advance_state_until(ComponentState::defined(), &vi_2);
        assert_eq!(vs_2, ComponentState::defined());

        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };

        // part 1
        {
            let mut vs_raw_1 = MipStoreRaw::try_from((
                [(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())],
                mip_stats_cfg.clone(),
            ))
            .unwrap();

            let mut vi_2_2 = vi_2.clone();
            // Make mip info invalid (because start == vi_1.timeout)
            vi_2_2.start = vi_1.timeout;
            let vs_2_2 = advance_state_until(ComponentState::defined(), &vi_2_2);
            let vs_raw_2 = MipStoreRaw {
                store: BTreeMap::from([
                    (vi_1.clone(), vs_1.clone()),
                    (vi_2_2.clone(), vs_2_2.clone()),
                ]),
                stats: MipStoreStats::new(mip_stats_cfg.clone()),
            };

            assert_matches!(
                vs_raw_1.update_with(&vs_raw_2),
                Err(UpdateWithError::Overlapping(..))
            );
            assert_eq!(vs_raw_1.store.get(&vi_1).unwrap().state, vs_1.state);
            assert_eq!(vs_raw_1.store.get(&vi_2).unwrap().state, vs_2.state);

            // Check that try_from fails too (because it uses update_with internally)
            {
                let _vs_raw_2_ = MipStoreRaw::try_from((
                    [
                        (vi_1.clone(), vs_1.clone()),
                        (vi_2_2.clone(), vs_2_2.clone()),
                    ],
                    mip_stats_cfg.clone(),
                ));
                assert_eq!(_vs_raw_2_.is_err(), true);
            }
        }

        // part 2
        {
            let mut vs_raw_1 = MipStoreRaw::try_from((
                [(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())],
                mip_stats_cfg.clone(),
            ))
            .unwrap();

            let mut vi_2_2 = vi_2.clone();
            vi_2_2.components = vi_1.components.clone();

            let vs_2_2 = advance_state_until(ComponentState::defined(), &vi_2_2);
            let vs_raw_2 = MipStoreRaw {
                store: BTreeMap::from([
                    (vi_1.clone(), vs_1.clone()),
                    (vi_2_2.clone(), vs_2_2.clone()),
                ]),
                stats: MipStoreStats::new(mip_stats_cfg.clone()),
            };

            // Component states being equal should produce an Ok result
            // We also have vi.1.components == vi_2_2.components ~ overlapping versions
            // TODO: clarify how this is supposed to behave
            assert_matches!(vs_raw_1.update_with(&vs_raw_2), Ok(_));
        }
    }

    #[test]
    fn test_empty_mip_store() {
        // Test if we can init an empty MipStore

        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };

        let mip_store = MipStore::try_from(([], mip_stats_config));
        assert_eq!(mip_store.is_ok(), true);
    }

    #[test]
    fn test_update_with_unknown() {
        // Test update_with with unknown MipComponent

        // data
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };

        let mut mip_store_raw_1 = MipStoreRaw::try_from(([], mip_stats_config.clone())).unwrap();

        let mi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::__Nonexhaustive, 1)]),
            start: MassaTime::from_millis(0),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };
        let ms_1 = advance_state_until(ComponentState::defined(), &mi_1);
        assert_eq!(ms_1, ComponentState::defined());
        let mip_store_raw_2 = MipStoreRaw {
            store: BTreeMap::from([(mi_1.clone(), ms_1.clone())]),
            stats: MipStoreStats::new(mip_stats_config.clone()),
        };

        let (updated, added) = mip_store_raw_1.update_with(&mip_store_raw_2).unwrap();

        assert_eq!(updated.len(), 0);
        assert_eq!(added.len(), 1);
        assert_eq!(added.get(&mi_1).unwrap().state, ComponentState::defined());
    }

    #[test]
    fn test_mip_store_network_restart() {
        // Test if we can get a coherent MipStore after a network shutdown

        let genesis_timestamp = MassaTime::from_millis(0);

        let shutdown_start = Slot::new(2, 0);
        let shutdown_end = Slot::new(8, 0);

        // helper to make the easy to read
        let get_slot_ts =
            |slot| get_block_slot_timestamp(THREAD_COUNT, T0, genesis_timestamp, slot).unwrap();
        let is_coherent = |store: &MipStoreRaw, shutdown_start, shutdown_end| {
            store
                .is_coherent_with_shutdown_period(
                    shutdown_start,
                    shutdown_end,
                    THREAD_COUNT,
                    T0,
                    genesis_timestamp,
                )
                .unwrap()
        };
        let update_store = |store: &mut MipStoreRaw, shutdown_start, shutdown_end| {
            store
                .update_for_network_shutdown(
                    shutdown_start,
                    shutdown_end,
                    THREAD_COUNT,
                    T0,
                    genesis_timestamp,
                )
                .unwrap()
        };
        let _dump_store = |store: &MipStoreRaw| {
            println!("Dump store:");
            for (mip_info, mip_state) in store.store.iter() {
                println!(
                    "mip_info {} {} - start: {} - timeout: {}: state: {:?}",
                    mip_info.name,
                    mip_info.version,
                    get_closest_slot_to_timestamp(
                        THREAD_COUNT,
                        T0,
                        genesis_timestamp,
                        mip_info.start
                    ),
                    get_closest_slot_to_timestamp(
                        THREAD_COUNT,
                        T0,
                        genesis_timestamp,
                        mip_info.timeout
                    ),
                    mip_state.state
                );
            }
        };
        // end helpers

        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };
        let mut mi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(2),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(100),
        };
        let mut mi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: HashMap::from([(MipComponent::Address, 2)]),
            start: MassaTime::from_millis(7),
            timeout: MassaTime::from_millis(11),
            activation_delay: MassaTime::from_millis(100),
        };

        // MipInfo 1 @ state 'Defined' should start during shutdown
        {
            mi_1.start = get_slot_ts(Slot::new(3, 7));
            mi_1.timeout = get_slot_ts(Slot::new(5, 7));
            mi_2.start = get_slot_ts(Slot::new(7, 7));
            mi_2.timeout = get_slot_ts(Slot::new(10, 7));

            let ms_1 = advance_state_until(ComponentState::defined(), &mi_1);
            let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);
            let mut store = MipStoreRaw::try_from((
                [(mi_1.clone(), ms_1), (mi_2.clone(), ms_2)],
                mip_stats_cfg.clone(),
            ))
            .unwrap();

            assert_eq!(is_coherent(&store, shutdown_start, shutdown_end), false);
            update_store(&mut store, shutdown_start, shutdown_end);
            assert_eq!(is_coherent(&store, shutdown_start, shutdown_end), true);
            // _dump_store(&store);
        }

        // MipInfo 1 @ state 'Defined' will start AFTER shutdown
        {
            mi_1.start = get_slot_ts(Slot::new(9, 7));
            mi_1.timeout = get_slot_ts(Slot::new(11, 7));
            mi_2.start = get_slot_ts(Slot::new(12, 7));
            mi_2.timeout = get_slot_ts(Slot::new(19, 7));

            let ms_1 = advance_state_until(ComponentState::defined(), &mi_1);
            let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);
            let mut store = MipStoreRaw::try_from((
                [(mi_1.clone(), ms_1), (mi_2.clone(), ms_2)],
                mip_stats_cfg.clone(),
            ))
            .unwrap();
            let store_orig = store.clone();

            // Already ok even with a shutdown but let's check it
            assert_eq!(is_coherent(&store, shutdown_start, shutdown_end), true);
            // _dump_store(&store);
            update_store(&mut store, shutdown_start, shutdown_end);
            assert_eq!(is_coherent(&store, shutdown_start, shutdown_end), true);
            // _dump_store(&store);

            // Check that nothing has changed
            assert_eq!(store_orig, store);
        }

        // MipInfo 1 @ state 'Started' before shutdown
        {
            mi_1.start = get_slot_ts(Slot::new(1, 7));
            mi_1.timeout = get_slot_ts(Slot::new(5, 7));
            mi_2.start = get_slot_ts(Slot::new(7, 7));
            mi_2.timeout = get_slot_ts(Slot::new(10, 7));

            let ms_1 = advance_state_until(ComponentState::started(Amount::zero()), &mi_1);
            let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);
            let mut store = MipStoreRaw::try_from((
                [(mi_1.clone(), ms_1), (mi_2.clone(), ms_2)],
                mip_stats_cfg.clone(),
            ))
            .unwrap();

            assert_eq!(is_coherent(&store, shutdown_start, shutdown_end), false);
            update_store(&mut store, shutdown_start, shutdown_end);
            assert_eq!(is_coherent(&store, shutdown_start, shutdown_end), true);
            // _dump_store(&store);
        }

        // MipInfo 1 @ state 'LockedIn' with transition during shutdown
        {
            let shutdown_range = shutdown_start..=shutdown_end;

            mi_1.start = get_slot_ts(Slot::new(1, 7));
            mi_1.timeout = get_slot_ts(Slot::new(5, 7));

            // Just before shutdown
            let locked_in_at = Slot::new(1, 9);
            assert!(locked_in_at < shutdown_start);
            let activate_at = Slot::new(4, 0);
            assert!(shutdown_range.contains(&activate_at));
            mi_1.activation_delay =
                get_slot_ts(activate_at).saturating_sub(get_slot_ts(locked_in_at));

            // Note: 2 states will transition right after shutdown_end
            //       mi_1, ms_1 -> 'LockedIn' -> 'Active'
            //       mi_2 -> 'Defined' -> 'Started'
            mi_2.start = get_slot_ts(Slot::new(7, 7));
            mi_2.timeout = get_slot_ts(Slot::new(10, 7));

            let ms_1 = advance_state_until(
                ComponentState::locked_in(get_slot_ts(Slot::new(1, 9))),
                &mi_1,
            );
            let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);
            let mut store = MipStoreRaw::try_from((
                [(mi_1.clone(), ms_1), (mi_2.clone(), ms_2)],
                mip_stats_cfg.clone(),
            ))
            .unwrap();

            assert_eq!(is_coherent(&store, shutdown_start, shutdown_end), false);
            // _dump_store(&store);
            update_store(&mut store, shutdown_start, shutdown_end);
            assert_eq!(is_coherent(&store, shutdown_start, shutdown_end), true);
            // _dump_store(&store);

            // Update stats - so should force 2 version transitions
            store.update_network_version_stats(
                get_slot_ts(shutdown_end.get_next_slot(THREAD_COUNT).unwrap()),
                Some((1, 0)),
            );

            let (first_mi_info, first_mi_state) = store.store.first_key_value().unwrap();
            assert_eq!(*first_mi_info.name, mi_1.name);
            assert_eq!(
                ComponentStateTypeId::from(&first_mi_state.state),
                ComponentStateTypeId::Active
            );
            let (last_mi_info, last_mi_state) = store.store.last_key_value().unwrap();
            assert_eq!(*last_mi_info.name, mi_2.name);
            assert_eq!(
                ComponentStateTypeId::from(&last_mi_state.state),
                ComponentStateTypeId::Started
            );
        }
    }

    #[test]
    fn test_mip_store_db() {
        // Test interaction of MIP store with MassaDB
        // 1- init from db (empty disk)
        // 2- update state
        // 3- write changes to db
        // 4- init a new mip store from disk and compare

        let genesis_timestamp = MassaTime::from_millis(0);
        // helpers
        let get_slot_ts =
            |slot| get_block_slot_timestamp(THREAD_COUNT, T0, genesis_timestamp, slot).unwrap();

        // Db init

        let temp_dir = tempdir().expect("Unable to create a temp folder");
        // println!("Using temp dir: {:?}", temp_dir.path());

        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 100,
            max_new_elements: 100,
            thread_count: THREAD_COUNT,
        };
        let db = Arc::new(RwLock::new(MassaDB::new(db_config)));

        // MIP info / store init

        let mi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: get_slot_ts(Slot::new(2, 0)),
            timeout: get_slot_ts(Slot::new(3, 0)),
            activation_delay: MassaTime::from_millis(10),
        };
        let ms_1 = advance_state_until(ComponentState::defined(), &mi_1);

        let mi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: HashMap::from([(MipComponent::Address, 2)]),
            start: get_slot_ts(Slot::new(4, 2)),
            timeout: get_slot_ts(Slot::new(7, 2)),
            activation_delay: MassaTime::from_millis(10),
        };
        let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);

        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mut mip_store = MipStore::try_from((
            [(mi_1.clone(), ms_1.clone()), (mi_2.clone(), ms_2.clone())],
            mip_stats_config.clone(),
        ))
        .expect("Cannot create an empty MIP store");

        // Step 1

        mip_store.extend_from_db(db.clone()).unwrap();
        // Check that we extend from an empty folder
        assert_eq!(mip_store.0.read().store.len(), 2);
        assert_eq!(
            mip_store.0.read().store.first_key_value(),
            Some((&mi_1, &ms_1))
        );
        assert_eq!(
            mip_store.0.read().store.last_key_value(),
            Some((&mi_2, &ms_2))
        );

        // Step 2
        let active_at = get_slot_ts(Slot::new(2, 5));
        let ms_1_ = advance_state_until(ComponentState::active(active_at), &mi_1);

        let mip_store_ =
            MipStore::try_from(([(mi_1.clone(), ms_1_.clone())], mip_stats_config.clone()))
                .expect("Cannot create an empty MIP store");

        let (updated, added) = mip_store.update_with(&mip_store_).unwrap();

        assert_eq!(updated.len(), 1);
        assert_eq!(added.len(), 0);
        assert_eq!(mip_store.0.read().store.len(), 2);
        assert_eq!(
            mip_store.0.read().store.first_key_value(),
            Some((&mi_1, &ms_1_))
        );
        assert_eq!(
            mip_store.0.read().store.last_key_value(),
            Some((&mi_2, &ms_2))
        );

        // Step 3

        let mut db_batch = DBBatch::new();
        let mut db_versioning_batch = DBBatch::new();

        // FIXME: get slot right after active at - no hardcode
        let slot_bounds_ = (&Slot::new(1, 0), &Slot::new(4, 2));
        let between = (&get_slot_ts(*slot_bounds_.0), &get_slot_ts(*slot_bounds_.1));

        mip_store
            .update_batches(&mut db_batch, &mut db_versioning_batch, between)
            .unwrap();

        assert_eq!(db_batch.len(), 1);
        assert_eq!(db_versioning_batch.len(), 1);

        let mut guard_db = db.write();
        // FIXME / TODO: no slot hardcoding?
        guard_db.write_batch(db_batch, db_versioning_batch, Some(Slot::new(3, 0)));
        drop(guard_db);

        // Step 4
        let mut mip_store_2 = MipStore::try_from((
            [(mi_1.clone(), ms_1.clone()), (mi_2.clone(), ms_2.clone())],
            mip_stats_config.clone(),
        ))
        .expect("Cannot create an empty MIP store");
        // assert_eq!(mip_store_2.0.read().store.len(), 0);

        mip_store_2.extend_from_db(db.clone()).unwrap();

        let guard_1 = mip_store.0.read();
        let guard_2 = mip_store_2.0.read();
        let st1_raw = guard_1.deref();
        let st2_raw = guard_2.deref();

        // println!("st1_raw: {:?}", st1_raw);
        // println!("st2_raw: {:?}", st2_raw);
        assert_eq!(st1_raw, st2_raw);
    }
}
