//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to the asynchronous message pool
use std::{
    collections::{btree_map::Entry, BTreeMap},
    ops::Bound::Included,
};

use massa_models::{
    async_msg::{
        AsyncMessage, AsyncMessageDeserializer, AsyncMessageSerializer, AsyncMessageUpdate,
        AsyncMessageUpdateDeserializer, AsyncMessageUpdateSerializer,
    },
    async_msg_id::{AsyncMessageId, AsyncMessageIdDeserializer, AsyncMessageIdSerializer},
    types::{
        Applicable, SetOrKeep, SetUpdateOrDelete, SetUpdateOrDeleteDeserializer,
        SetUpdateOrDeleteSerializer,
    },
};

use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
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

/// `AsyncPoolChanges` serializer
pub struct AsyncPoolChangesSerializer {
    u64_serializer: U64VarIntSerializer,
    id_serializer: AsyncMessageIdSerializer,
    set_update_or_delete_message_serializer: SetUpdateOrDeleteSerializer<
        AsyncMessage,
        AsyncMessageUpdate,
        AsyncMessageSerializer,
        AsyncMessageUpdateSerializer,
    >,
}

impl AsyncPoolChangesSerializer {
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            id_serializer: AsyncMessageIdSerializer::new(),
            set_update_or_delete_message_serializer: SetUpdateOrDeleteSerializer::new(
                AsyncMessageSerializer::new(false),
                AsyncMessageUpdateSerializer::new(false),
            ),
        }
    }
}

impl Default for AsyncPoolChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<AsyncPoolChanges> for AsyncPoolChangesSerializer {
    fn serialize(
        &self,
        value: &AsyncPoolChanges,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.u64_serializer.serialize(
            &(value.0.len().try_into().map_err(|_| {
                SerializeError::GeneralError("Fail to transform usize to u64".to_string())
            })?),
            buffer,
        )?;
        for (id, change) in &value.0 {
            self.id_serializer.serialize(id, buffer)?;
            self.set_update_or_delete_message_serializer
                .serialize(change, buffer)?;
        }
        Ok(())
    }
}

pub struct AsyncPoolChangesDeserializer {
    async_pool_changes_length: U64VarIntDeserializer,
    id_deserializer: AsyncMessageIdDeserializer,
    set_update_or_delete_message_deserializer: SetUpdateOrDeleteDeserializer<
        AsyncMessage,
        AsyncMessageUpdate,
        AsyncMessageDeserializer,
        AsyncMessageUpdateDeserializer,
    >,
}

impl AsyncPoolChangesDeserializer {
    pub fn new(
        thread_count: u8,
        max_async_pool_changes: u64,
        max_function_length: u16,
        max_function_params_length: u64,
        max_key_length: u32,
    ) -> Self {
        Self {
            async_pool_changes_length: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_async_pool_changes),
            ),
            id_deserializer: AsyncMessageIdDeserializer::new(thread_count),
            set_update_or_delete_message_deserializer: SetUpdateOrDeleteDeserializer::new(
                AsyncMessageDeserializer::new(
                    thread_count,
                    max_function_length,
                    max_function_params_length,
                    max_key_length,
                    false,
                ),
                AsyncMessageUpdateDeserializer::new(
                    thread_count,
                    max_function_length,
                    max_function_params_length,
                    max_key_length,
                    false,
                ),
            ),
        }
    }
}

impl Deserializer<AsyncPoolChanges> for AsyncPoolChangesDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], AsyncPoolChanges, E> {
        context(
            "Failed AsyncPoolChanges deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.async_pool_changes_length.deserialize(input)
                }),
                |input: &'a [u8]| {
                    tuple((
                        context("Failed id deserialization", |input| {
                            self.id_deserializer.deserialize(input)
                        }),
                        context(
                            "Failed set_update_or_delete_message deserialization",
                            |input| {
                                self.set_update_or_delete_message_deserializer
                                    .deserialize(input)
                            },
                        ),
                    ))(input)
                },
            ),
        )
        .map(|vec| AsyncPoolChanges(vec.into_iter().map(|data| (data.0, data.1)).collect()))
        .parse(buffer)
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
    use massa_serialization::{DeserializeError, Deserializer, Serializer};

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
    fn test_changes_ser_deser() {
        // Async pool changes serialization && deserialization

        let message = get_message();
        let mut changes = AsyncPoolChanges::default();
        changes.0.insert(
            message.compute_id(),
            SetUpdateOrDelete::Set(message.clone()),
        );

        let mut message2 = message.clone();
        message2.fee = Amount::from_str("2").unwrap();
        assert_ne!(message.compute_id(), message2.compute_id());

        let mut message3 = message.clone();
        message3.fee = Amount::from_str("3").unwrap();
        assert_ne!(message.compute_id(), message3.compute_id());

        changes
            .0
            .insert(message2.compute_id(), SetUpdateOrDelete::Delete);

        let update3 = AsyncMessageUpdate {
            coins: SetOrKeep::Set(Amount::from_str("3").unwrap()),
            ..Default::default()
        };

        changes
            .0
            .insert(message3.compute_id(), SetUpdateOrDelete::Update(update3));

        assert_eq!(changes.0.len(), 3);

        let mut serialized = Vec::new();
        let serializer = AsyncPoolChangesSerializer::new();
        let deserializer = AsyncPoolChangesDeserializer::new(32, 10000, 10000, 100000, 100000);
        serializer.serialize(&changes, &mut serialized).unwrap();
        let (rest, changes_deser) = deserializer
            .deserialize::<DeserializeError>(&serialized)
            .unwrap();
        assert!(rest.is_empty());
        assert_eq!(changes, changes_deser);
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
