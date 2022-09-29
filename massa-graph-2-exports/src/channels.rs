use std::sync::mpsc::Receiver;
use massa_pos_exports::SelectorController;

/// Contains a reference to the pool, selector and execution controller
/// Contains a channel to send info to protocol
#[derive(Clone)]
pub struct GraphChannels {
    pub selector_controller: Box<dyn SelectorController>,
}
