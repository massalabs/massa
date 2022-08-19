// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::{
    mpsc::{self, Receiver},
    Arc, Mutex,
};

use anyhow::Result;
use massa_models::{api::IndexedSlot, Address, Slot};

use crate::{CycleInfo, Selection, SelectorController};

/// All events that can be sent by the selector to your callbacks.
#[derive(Debug)]
pub enum MockSelectorControllerMessage {
    /// Feed a new cycle info to the selector
    FeedCycle {
        /// cycle infos
        cycle_info: CycleInfo,
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
        response_tx: mpsc::Sender<(Vec<Slot>, Vec<IndexedSlot>)>,
    },
    /// Get the producer for a block at a specific slot
    GetProducer {
        /// Slot to search
        slot: Slot,
        /// Receiver to send the result to
        response_tx: mpsc::Sender<Result<Address>>,
    },
    /// Get the selection for a block at a specific slot
    GetSelection {
        /// Slot to search
        slot: Slot,
        /// Receiver to send the result to
        response_tx: mpsc::Sender<Result<Selection>>,
    },
}

/// Mock implementation of the SelectorController trait.
/// This mock will be called by the others modules and you will receive events in the receiver.
/// You can choose to manage them how you want.
#[derive(Clone)]
pub struct MockSelectorController(Arc<Mutex<mpsc::Sender<MockSelectorControllerMessage>>>);

impl MockSelectorController {
    /// Create a new pair (mock execution controller, mpsc receiver for emitted messages)
    /// Note that unbounded mpsc channels are used
    pub fn new_with_receiver() -> (
        Box<dyn SelectorController>,
        Receiver<MockSelectorControllerMessage>,
    ) {
        let (tx, rx) = mpsc::channel();
        (
            Box::new(MockSelectorController(Arc::new(Mutex::new(tx)))),
            rx,
        )
    }
}

impl SelectorController for MockSelectorController {
    fn feed_cycle(&self, cycle_info: CycleInfo) {
        self.0
            .lock()
            .unwrap()
            .send(MockSelectorControllerMessage::FeedCycle { cycle_info })
            .unwrap();
    }

    fn get_address_selections(
        &self,
        address: &Address,
        start: Slot,
        end: Slot,
    ) -> (Vec<Slot>, Vec<IndexedSlot>) {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockSelectorControllerMessage::GetAddressSelections {
                address: *address,
                start,
                end,
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_producer(&self, slot: Slot) -> Result<Address> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockSelectorControllerMessage::GetProducer { slot, response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_selection(&self, slot: Slot) -> Result<Selection> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockSelectorControllerMessage::GetSelection { slot, response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn clone_box(&self) -> Box<dyn SelectorController> {
        Box::new(self.clone())
    }
}
