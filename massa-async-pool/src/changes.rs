//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to the asynchronous message pool
use std::collections::{btree_map::Entry, BTreeMap};

use massa_models::{
    async_msg::{AsyncMessage, AsyncMessageUpdate},
    async_msg_id::AsyncMessageId,
    types::{Applicable, SetOrKeep, SetUpdateOrDelete},
};

use serde::{Deserialize, Serialize};
use serde_with::serde_as;

/// Consolidated changes to the asynchronous message pool
#[serde_as]
#[derive(Default, Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AsyncPoolChanges(
    #[serde_as(as = "Vec<(_, _)>")]
    pub  BTreeMap<AsyncMessageId, SetUpdateOrDelete<AsyncMessage, AsyncMessageUpdate>>,
);

impl Applicable<AsyncPoolChanges> for AsyncPoolChanges {
    /// extends the current `AsyncPoolChanges` with another one
    fn apply(&mut self, changes: AsyncPoolChanges) {
        for (id, msg_change) in changes.0 {
            match self.0.entry(id) {
                Entry::Occupied(mut occ) => {
                    // apply incoming change if a change on this entry already exists
                    occ.get_mut().apply(msg_change);
                }
                Entry::Vacant(vac) => {
                    // otherwise insert the incoming change
                    vac.insert(msg_change);
                }
            }
        }
    }
}

impl AsyncPoolChanges {
    /// Pushes a message addition to the list of changes.
    /// No add/delete compensations are done.
    ///
    /// Arguments:
    /// * `msg_id`: ID of the message to push as added to the list of changes
    /// * `msg`: message to push as added to the list of changes
    pub fn push_add(&mut self, msg_id: AsyncMessageId, msg: AsyncMessage) {
        let mut change = AsyncPoolChanges::default();
        change.0.insert(msg_id, SetUpdateOrDelete::Set(msg));
        self.apply(change);
    }

    /// Pushes a message deletion to the list of changes.
    /// No add/delete compensations are done.
    ///
    /// Arguments:
    /// * `msg_id`: ID of the message to push as deleted to the list of changes
    pub fn push_delete(&mut self, msg_id: AsyncMessageId) {
        let mut change = AsyncPoolChanges::default();
        change.0.insert(msg_id, SetUpdateOrDelete::Delete);
        self.apply(change);
    }

    /// Pushes a message activation to the list of changes.
    ///
    /// Arguments:
    /// * `msg_id`: ID of the message to push as ready to be executed to the list of changes
    pub fn push_activate(&mut self, msg_id: AsyncMessageId) {
        let mut change = AsyncPoolChanges::default();

        let msg_update = AsyncMessageUpdate {
            can_be_executed: SetOrKeep::Set(true),
            ..Default::default()
        };

        change
            .0
            .insert(msg_id, SetUpdateOrDelete::Update(msg_update));
        self.apply(change);
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use massa_models::types::SetUpdateOrDelete;
    use massa_models::{
        address::Address, amount::Amount, async_msg::AsyncMessageTrigger, slot::Slot,
    };

    use assert_matches::assert_matches;

    use super::*;

    fn get_message() -> AsyncMessage {
        AsyncMessage::new(
            Slot::new(1, 0),
            0,
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            String::from("test"),
            10000000,
            Amount::from_str("1").unwrap(),
            Amount::from_str("1").unwrap(),
            Slot::new(2, 0),
            Slot::new(3, 0),
            vec![1, 2, 3, 4],
            Some(AsyncMessageTrigger {
                address: Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x")
                    .unwrap(),
                datastore_key: Some(vec![1, 2, 3, 4]),
            }),
            None,
        )
    }

    #[test]
    fn test_pool_changes_push() {
        // AsyncPoolChanges, push_add/push_delete/push_activate

        let message = get_message();
        assert!(!message.can_be_executed);

        let mut changes = AsyncPoolChanges::default();

        changes.push_add(message.compute_id(), message.clone());
        assert_eq!(changes.0.len(), 1);
        assert_matches!(
            changes.0.get(&message.compute_id()),
            Some(&SetUpdateOrDelete::Set(..))
        );

        changes.push_activate(message.compute_id());
        assert_eq!(changes.0.len(), 1);
        let value = changes.0.get(&message.compute_id()).unwrap();
        match value {
            SetUpdateOrDelete::Set(msg) => {
                assert!(msg.can_be_executed);
            }
            _ => {
                panic!("Unexpected value");
            }
        }

        changes.push_delete(message.compute_id());
        // Len is still 1, but value has changed
        assert_eq!(changes.0.len(), 1);
        assert_eq!(
            changes.0.get(&message.compute_id()),
            Some(&SetUpdateOrDelete::Delete)
        );
    }
}
