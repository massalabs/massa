use std::collections::BTreeMap;

pub type Key = Vec<u8>;
pub type Value = Vec<u8>;

/// We use batching to reduce the number of writes to the database
///
/// Here, a DBBatch is a map from Key to Some(Value) for a new or updated value, or None for a deletion
pub type DBBatch = BTreeMap<Key, Option<Value>>;

/// A Batch of elements from the database, used by a bootstrap server.
#[derive(Debug, Clone)]
pub struct StreamBatch<ChangeID: PartialOrd + Ord + PartialEq + Eq + Clone + std::fmt::Debug> {
    /// New elements to be streamed to the client.
    pub new_elements: BTreeMap<Key, Value>,
    /// The changes made to previously streamed keys. Note that a None value can delete a given key.
    pub updates_on_previous_elements: BTreeMap<Key, Option<Value>>,
    /// The ChangeID associated with this batch, useful for syncing the changes not streamed yet to the client.
    pub change_id: ChangeID,
}

impl<ChangeID: PartialOrd + Ord + PartialEq + Eq + Clone + std::fmt::Debug> StreamBatch<ChangeID> {
    /// Helper function used to know if the main bootstrap state step is finished.
    ///
    /// Note: even after having an empty StreamBatch, we still need to send the updates on previous elements while bootstrap has not finished.
    pub fn is_empty(&self) -> bool {
        self.updates_on_previous_elements.is_empty() && self.new_elements.is_empty()
    }
}
