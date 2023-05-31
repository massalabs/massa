use std::collections::BTreeMap;

pub type Key = Vec<u8>;
pub type Value = Vec<u8>;

/// We use batching to reduce the number of writes to the database
///
/// Here, a DBBatch is a map from Key to Some(Value) for a new or updated value, or None for a deletion
pub type DBBatch = BTreeMap<Key, Option<Value>>;
