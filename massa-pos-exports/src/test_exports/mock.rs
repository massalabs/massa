// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use massa_hash::Hash;
use massa_models::{
    address::Address,
    slot::{IndexedSlot, Slot},
};

use crate::{PosResult, Selection, SelectorController};

/// All events that can be sent by the selector to your callbacks.
#[derive(Debug)]
pub(crate)  enum MockSelectorControllerMessage {
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

/// Mock implementation of the `SelectorController` trait.
/// This mock will be called by the others modules and you will receive events in the receiver.
/// You can choose to manage them how you want.
#[derive(Clone)]
pub(crate)  struct MockSelectorController(Arc<Mutex<Sender<MockSelectorControllerMessage>>>);

impl MockSelectorController {
    /// Create a new pair (mock execution controller, mpsc receiver for emitted messages)
    /// Note that unbounded mpsc channels are used
    pub(crate)  fn new_with_receiver() -> (
        Box<dyn SelectorController>,
        Receiver<MockSelectorControllerMessage>,
    ) {
        let (tx, rx) = crossbeam_channel::unbounded();
        (
            Box::new(MockSelectorController(Arc::new(Mutex::new(tx)))),
            rx,
        )
    }
}

impl SelectorController for MockSelectorController {
    fn feed_cycle(
        &self,
        cycle: u64,
        lookback_rolls: BTreeMap<Address, u64>,
        lookback_seed: Hash,
    ) -> PosResult<()> {
        self.0
            .lock()
            .send(MockSelectorControllerMessage::FeedCycle {
                cycle,
                lookback_rolls,
                lookback_seed,
            })
            .unwrap();
        Ok(())
    }

    /// Get every [Selection]
    ///
    /// Only used for testing
    ///
    /// TODO: limit usage
    #[cfg(feature = "testing")]
    fn get_entire_selection(&self) -> VecDeque<(u64, HashMap<Slot, Selection>)> {
        let (response_tx, response_rx) = crossbeam_channel::unbounded();
        self.0
            .lock()
            .send(MockSelectorControllerMessage::GetEntireSelection { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn wait_for_draws(&self, cycle: u64) -> PosResult<u64> {
        let (response_tx, response_rx) = crossbeam_channel::unbounded();
        self.0
            .lock()
            .send(MockSelectorControllerMessage::WaitForDraws { cycle, response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_address_selections(
        &self,
        address: &Address,
        start: Slot,
        end: Slot,
    ) -> PosResult<(Vec<Slot>, Vec<IndexedSlot>)> {
        let (response_tx, response_rx) = crossbeam_channel::unbounded();
        self.0
            .lock()
            .send(MockSelectorControllerMessage::GetAddressSelections {
                address: *address,
                start,
                end,
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_producer(&self, slot: Slot) -> PosResult<Address> {
        let (response_tx, response_rx) = crossbeam_channel::unbounded();
        self.0
            .lock()
            .send(MockSelectorControllerMessage::GetProducer { slot, response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_selection(&self, slot: Slot) -> PosResult<Selection> {
        let (response_tx, response_rx) = crossbeam_channel::unbounded();
        self.0
            .lock()
            .send(MockSelectorControllerMessage::GetSelection { slot, response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn clone_box(&self) -> Box<dyn SelectorController> {
        Box::new(self.clone())
    }
}
