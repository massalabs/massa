// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crossbeam_channel::Sender;
use std::collections::{BTreeMap, HashMap, VecDeque};

use massa_hash::Hash;
use massa_models::{
    address::Address,
    slot::{IndexedSlot, Slot},
};

use crate::{PosResult, Selection};

/// All events that can be sent by the selector to your callbacks.
#[derive(Debug)]
pub enum MockSelectorControllerMessage {
    /// Feed a new cycle info to the selector
    FeedCycle {
        /// cycle
        cycle: u64,
        /// look back rolls
        lookback_rolls: BTreeMap<Address, u64>,
        /// look back seed
        lookback_seed: Hash,
    },
    /// Get a list of slots where address has been chosen to produce a block and a list where he is chosen for the endorsements.
    /// Look from the start slot to the end slot.
    GetAddressSelections {
        /// Address to search
        address: Address,
        /// Start of the search range
        start: Slot,
        /// End of the search range
        end: Slot,
        /// Receiver to send the result to
        response_tx: Sender<PosResult<(Vec<Slot>, Vec<IndexedSlot>)>>,
    },
    /// Get the entire selection of PoS. used for testing only
    GetEntireSelection {
        /// response channel
        response_tx: Sender<VecDeque<(u64, HashMap<Slot, Selection>)>>,
    },
    /// Get the producer for a block at a specific slot
    GetProducer {
        /// Slot to search
        slot: Slot,
        /// Receiver to send the result to
        response_tx: Sender<PosResult<Address>>,
    },
    /// Get the selection for a block at a specific slot
    GetSelection {
        /// Slot to search
        slot: Slot,
        /// Receiver to send the result to
        response_tx: Sender<PosResult<Selection>>,
    },
    /// Wait for draws
    WaitForDraws {
        /// Cycle to wait for
        cycle: u64,
        /// Receiver to send the result to
        response_tx: Sender<PosResult<u64>>,
    },
}
