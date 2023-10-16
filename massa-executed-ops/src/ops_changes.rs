//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    operation::{OperationId, OperationIdDeserializer, OperationIdSerializer},
    prehash::PreHashMap,
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_serialization::{
    BoolDeserializer, BoolSerializer, Deserializer, SerializeError, Serializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use std::ops::Bound::{Excluded, Included};

/// Changes for ExecutedOps (was_successful, op_expiry_slot)
pub type ExecutedOpsChanges = PreHashMap<OperationId, (bool, Slot)>;

/// `ExecutedOps` Serializer
pub struct ExecutedOpsChangesSerializer {
    u64_serializer: U64VarIntSerializer,
    operation_id_serializer: OperationIdSerializer,
    op_execution: BoolSerializer,
    slot_serializer: SlotSerializer,
}

impl Default for ExecutedOpsChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutedOpsChangesSerializer {
    /// Create a new `ExecutedOps` Serializer
    pub fn new() -> ExecutedOpsChangesSerializer {
        ExecutedOpsChangesSerializer {
            u64_serializer: U64VarIntSerializer::new(),
            operation_id_serializer: OperationIdSerializer::new(),
            op_execution: BoolSerializer::new(),
            slot_serializer: SlotSerializer::new(),
        }
    }
}

impl Serializer<ExecutedOpsChanges> for ExecutedOpsChangesSerializer {
    fn serialize(
        &self,
        value: &ExecutedOpsChanges,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.u64_serializer
            .serialize(&(value.len() as u64), buffer)?;
        for (op_id, (op_execution_succeeded, slot)) in value {
            self.operation_id_serializer.serialize(op_id, buffer)?;
            self.op_execution
                .serialize(op_execution_succeeded, buffer)?;
            self.slot_serializer.serialize(slot, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `ExecutedOps`
pub struct ExecutedOpsChangesDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    operation_id_deserializer: OperationIdDeserializer,
    op_execution_deserializer: BoolDeserializer,
    slot_deserializer: SlotDeserializer,
}

impl ExecutedOpsChangesDeserializer {
    /// Create a new deserializer for `ExecutedOps`
    pub fn new(thread_count: u8, max_ops_changes_length: u64) -> ExecutedOpsChangesDeserializer {
        ExecutedOpsChangesDeserializer {
            u64_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_ops_changes_length),
            ),
            operation_id_deserializer: OperationIdDeserializer::new(),
            op_execution_deserializer: BoolDeserializer::new(),
            slot_deserializer: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
        }
    }
}

impl Deserializer<ExecutedOpsChanges> for ExecutedOpsChangesDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ExecutedOpsChanges, E> {
        context(
            "ExecutedOpsChanges",
            length_count(
                context("ExecutedOpsChanges length", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((
                    context("operation id", |input| {
                        self.operation_id_deserializer.deserialize(input)
                    }),
                    context("operation execution status", |input| {
                        self.op_execution_deserializer.deserialize(input)
                    }),
                    context("expiration slot", |input| {
                        self.slot_deserializer.deserialize(input)
                    }),
                )),
            ),
        )
        .map(|ids| {
            ids.into_iter()
                .map(|(id, op_exec_status, slot)| (id, (op_exec_status, slot)))
                .collect()
        })
        .parse(buffer)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use massa_hash::Hash;
    use massa_models::config::{KEEP_EXECUTED_HISTORY_EXTRA_PERIODS, THREAD_COUNT};
    use massa_models::secure_share::Id;
    use massa_serialization::DeserializeError;

    #[test]
    fn test_exec_ops_changes_ser_deser() {
        // ExecutedOpsChanges serialization and deserialization

        let mut changes = PreHashMap::default();

        let slot_1 = Slot::new(1, 0);
        let op_id_1 = OperationId::new(Hash::compute_from(&[0]));
        changes.insert(op_id_1.clone(), (true, slot_1));
        let slot_2 = Slot::new(KEEP_EXECUTED_HISTORY_EXTRA_PERIODS + 2, 3);
        let op_id_2 = OperationId::new(Hash::compute_from(&[1]));
        changes.insert(op_id_2, (true, slot_2));

        let mut buf = Vec::new();
        let serializer = ExecutedOpsChangesSerializer::new();
        let deserializer_1 = ExecutedOpsChangesDeserializer::new(THREAD_COUNT, 1);
        let deserializer_2 = ExecutedOpsChangesDeserializer::new(THREAD_COUNT, 2);

        serializer.serialize(&changes, &mut buf).unwrap();
        // Only 1 changes allowed, should fail
        let res = deserializer_1.deserialize::<DeserializeError>(&buf);
        assert!(res.is_err());

        let (rem, changes_der) = deserializer_2
            .deserialize::<DeserializeError>(&buf)
            .unwrap();
        assert!(rem.is_empty());
        assert_eq!(changes_der, changes);
    }
}
