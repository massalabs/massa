// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::serialization::{VecU8Deserializer, VecU8Serializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::error::{context, ContextError, ParseError};
use nom::multi::length_count;
use nom::sequence::tuple;
use nom::{IResult, Parser};
use std::collections::BTreeMap;
use std::ops::Bound::Included;

/// Datastore entry for Ledger & `ExecuteSC` Operation
/// A Datastore is a Key Value store where
/// Key: Byte array (max length should be 255)
/// Value: Byte array
/// What is stored can be arbitrary bytes but can often be smart contract bytecode (aka WASM binary)
pub type Datastore = BTreeMap<Vec<u8>, Vec<u8>>;

/// Serializer for `Datastore`
#[derive(Default)]
pub struct DatastoreSerializer {
    u64_serializer: U64VarIntSerializer,
    vec_u8_serializer: VecU8Serializer,
}

impl DatastoreSerializer {
    /// Creates a new `DatastoreSerializer`
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
        }
    }
}

impl Serializer<Datastore> for DatastoreSerializer {
    /// ## Example
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use massa_models::datastore::DatastoreSerializer;
    /// use massa_serialization::Serializer;
    ///
    /// let serializer = DatastoreSerializer::new();
    /// let mut buffer = Vec::new();
    /// let mut datastore = BTreeMap::new();
    /// datastore.insert(vec![1, 2, 3], vec![4, 5, 6]);
    /// datastore.insert(vec![3, 4, 5], vec![6, 7, 8]);
    /// serializer.serialize(&datastore, &mut buffer).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &BTreeMap<Vec<u8>, Vec<u8>>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        let entry_count: u64 = value.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!(
                "too many entries in ConsensusLedgerSubset: {}",
                err
            ))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;
        for (key, value) in value.iter() {
            self.vec_u8_serializer.serialize(key, buffer)?;
            self.vec_u8_serializer.serialize(value, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `Datastore` field in `LedgerEntry`
pub struct DatastoreDeserializer {
    length_deserializer: U64VarIntDeserializer,
    key_deserializer: VecU8Deserializer,
    value_deserializer: VecU8Deserializer,
}

impl DatastoreDeserializer {
    /// Creates a new `DatastoreDeserializer`
    pub fn new(
        max_datastore_entry_count: u64,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
    ) -> Self {
        Self {
            length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_datastore_entry_count),
            ),
            key_deserializer: VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_key_length as u64),
            ),
            value_deserializer: VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_value_length),
            ),
        }
    }
}

impl Deserializer<Datastore> for DatastoreDeserializer {
    /// ## Example
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use massa_models::datastore::{DatastoreDeserializer, DatastoreSerializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let serializer = DatastoreSerializer::new();
    /// let deserializer = DatastoreDeserializer::new(10000, 255, 10000);
    /// let mut buffer = Vec::new();
    /// let mut datastore = BTreeMap::new();
    /// datastore.insert(vec![1, 2, 3], vec![4, 5, 6]);
    /// datastore.insert(vec![3, 4, 5], vec![6, 7, 8]);
    /// serializer.serialize(&datastore, &mut buffer).unwrap();
    /// let (rest, deserialized) = deserializer.deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(deserialized, datastore);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BTreeMap<Vec<u8>, Vec<u8>>, E> {
        context(
            "Failed Datastore deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.length_deserializer.deserialize(input)
                }),
                tuple((
                    context("Failed key deserialization", |input| {
                        self.key_deserializer.deserialize(input)
                    }),
                    context("Failed value deserialization", |input| {
                        self.value_deserializer.deserialize(input)
                    }),
                )),
            ),
        )
        .map(|elements| elements.into_iter().collect())
        .parse(buffer)
    }
}

/// For lexicographically ordered keys,
/// gets the upper and lower bound of keys matching a prefix.
pub fn get_prefix_bounds(prefix: &[u8]) -> (std::ops::Bound<Vec<u8>>, std::ops::Bound<Vec<u8>>) {
    if prefix.is_empty() {
        return (std::ops::Bound::Unbounded, std::ops::Bound::Unbounded);
    }
    let n_keep = prefix
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, v)| if v < &255 { Some(i + 1) } else { None })
        .unwrap_or(0);
    let mut prefix_end = prefix[..n_keep].to_vec();
    if let Some(v) = prefix_end.last_mut() {
        *v += 1;
    }
    (
        std::ops::Bound::Included(prefix.to_vec()),
        if !prefix_end.is_empty() {
            std::ops::Bound::Excluded(prefix_end)
        } else {
            std::ops::Bound::Unbounded
        },
    )
}

/// Return the intersection of two ranges
pub fn range_intersection<T: Ord>(
    r1: (std::ops::Bound<T>, std::ops::Bound<T>),
    r2: (std::ops::Bound<T>, std::ops::Bound<T>),
) -> Option<(std::ops::Bound<T>, std::ops::Bound<T>)> {
    use std::cmp::{max, min};
    use std::ops::Bound;

    let (r1s, r1e) = r1;
    let (r2s, r2e) = r2;

    // Determine the start of the intersection
    let start = match (r1s, r2s) {
        (Bound::Included(a), Bound::Included(b)) => Bound::Included(max(a, b)),
        (Bound::Included(a), Bound::Excluded(b)) => {
            if a > b {
                Bound::Included(a)
            } else {
                Bound::Excluded(b)
            }
        }
        (Bound::Excluded(a), Bound::Included(b)) => {
            if b > a {
                Bound::Included(b)
            } else {
                Bound::Excluded(a)
            }
        }
        (Bound::Excluded(a), Bound::Excluded(b)) => Bound::Excluded(max(a, b)),
        (Bound::Unbounded, other) => other,
        (other, Bound::Unbounded) => other,
    };

    // Determine the end of the intersection
    let end = match (r1e, r2e) {
        (Bound::Included(a), Bound::Included(b)) => Bound::Included(min(a, b)),
        (Bound::Included(a), Bound::Excluded(b)) => {
            if a < b {
                Bound::Included(a)
            } else {
                Bound::Excluded(b)
            }
        }
        (Bound::Excluded(a), Bound::Included(b)) => {
            if b < a {
                Bound::Included(b)
            } else {
                Bound::Excluded(a)
            }
        }
        (Bound::Excluded(a), Bound::Excluded(b)) => Bound::Excluded(min(a, b)),
        (Bound::Unbounded, other) => other,
        (other, Bound::Unbounded) => other,
    };

    // Ensure the resulting range is valid
    match (&start, &end) {
        (Bound::Included(a), Bound::Included(b)) if a > b => None,
        (Bound::Included(a), Bound::Excluded(b)) if a >= b => None,
        (Bound::Excluded(a), Bound::Included(b)) if a >= b => None,
        (Bound::Excluded(a), Bound::Excluded(b)) if a >= b => None,
        _ => Some((start, end)),
    }
}

#[cfg(test)]
mod tests {

    use crate::config::{
        MAX_OPERATION_DATASTORE_ENTRY_COUNT, MAX_OPERATION_DATASTORE_KEY_LENGTH,
        MAX_OPERATION_DATASTORE_VALUE_LENGTH,
    };

    use super::*;
    use massa_serialization::DeserializeError;
    use serde::{Deserialize, Serialize};
    use serde_with::serde_as;

    #[serde_as]
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct SerdeWrapper(#[serde_as(as = "Vec<(_, _)>")] Datastore);

    #[test]
    fn test_ser_der() {
        let datastore = BTreeMap::from([
            (vec![1, 2], vec![3, 4]),
            (vec![5, 6, 7], vec![8]),
            (vec![9], vec![10, 11, 12, 13, 14]),
            (vec![], vec![]),
        ]);

        let datastore_serializer = DatastoreSerializer::new();
        let mut buffer = Vec::new();
        datastore_serializer
            .serialize(&datastore, &mut buffer)
            .expect("Should not fail while serializing Datastore");

        let datastore_deserializer = DatastoreDeserializer::new(
            MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        );
        let (_, datastore_der) = datastore_deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();
        assert_eq!(datastore, datastore_der);
    }

    #[test]
    #[should_panic]
    fn test_der_fail() {
        let max_operation_datastore_entry_count: usize = 10;

        // a datastore too much entries
        let datastore = std::iter::repeat(())
            .enumerate()
            .map(|(i, _)| (vec![i as u8, 1, 2], vec![33, 44, 55]))
            .take(max_operation_datastore_entry_count + 1)
            .collect();

        let datastore_serializer = DatastoreSerializer::new();
        let mut buffer = Vec::new();
        datastore_serializer
            .serialize(&datastore, &mut buffer)
            .expect("Should not fail while serializing Datastore");

        let datastore_deserializer = DatastoreDeserializer::new(
            max_operation_datastore_entry_count as u64,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        );
        let (_, _datastore_der) = datastore_deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();
    }

    #[test]
    fn test_datastore_serde() {
        let expected_datastore: Datastore = BTreeMap::from([
            (vec![1, 2], vec![3, 4]),
            (vec![5, 6, 7], vec![8]),
            (vec![9], vec![10, 11, 12, 13, 14]),
            (vec![], vec![]),
        ]);

        let wrapper = SerdeWrapper(expected_datastore.clone());
        let serialized = serde_json::to_string(&wrapper).unwrap();
        let actual_wrapper: SerdeWrapper = serde_json::from_str(&serialized).unwrap();

        assert_eq!(actual_wrapper.0, expected_datastore);
    }

    #[test]
    fn test_range_intersection() {
        // Overlapping ranges
        {
            let r1 = (std::ops::Bound::Included(1), std::ops::Bound::Included(5));
            let r2 = (std::ops::Bound::Included(3), std::ops::Bound::Included(7));
            let expected = (std::ops::Bound::Included(3), std::ops::Bound::Included(5));
            assert_eq!(range_intersection(r1, r2), Some(expected));
        }

        // Fully overlapping ranges
        {
            let r1 = (std::ops::Bound::Included(1), std::ops::Bound::Included(10));
            let r2 = (std::ops::Bound::Included(3), std::ops::Bound::Included(7));
            let expected = (std::ops::Bound::Included(3), std::ops::Bound::Included(7));
            assert_eq!(range_intersection(r1, r2), Some(expected));
        }

        // Adjacent ranges (no overlap)
        {
            let r1 = (std::ops::Bound::Included(1), std::ops::Bound::Excluded(5));
            let r2 = (std::ops::Bound::Included(5), std::ops::Bound::Included(10));
            assert_eq!(range_intersection(r1, r2), None);
        }

        // Exact match
        {
            let r1 = (std::ops::Bound::Included(1), std::ops::Bound::Included(5));
            let r2 = (std::ops::Bound::Included(1), std::ops::Bound::Included(5));
            let expected = (std::ops::Bound::Included(1), std::ops::Bound::Included(5));
            assert_eq!(range_intersection(r1, r2), Some(expected));
        }

        // Unbounded start
        {
            let r1 = (std::ops::Bound::Unbounded, std::ops::Bound::Included(5));
            let r2 = (std::ops::Bound::Included(3), std::ops::Bound::Included(7));
            let expected = (std::ops::Bound::Included(3), std::ops::Bound::Included(5));
            assert_eq!(range_intersection(r1, r2), Some(expected));
        }

        // Unbounded end
        {
            let r1 = (std::ops::Bound::Included(1), std::ops::Bound::Unbounded);
            let r2 = (std::ops::Bound::Included(3), std::ops::Bound::Included(7));
            let expected = (std::ops::Bound::Included(3), std::ops::Bound::Included(7));
            assert_eq!(range_intersection(r1, r2), Some(expected));
        }

        // Both ranges unbounded
        {
            let r1 = (std::ops::Bound::Unbounded, std::ops::Bound::Unbounded);
            let r2 = (std::ops::Bound::Included(3), std::ops::Bound::Included(7));
            let expected = (std::ops::Bound::Included(3), std::ops::Bound::Included(7));
            assert_eq!(range_intersection(r1, r2), Some(expected));
        }

        // Non-overlapping ranges
        {
            let r1 = (std::ops::Bound::Included(1), std::ops::Bound::Included(5));
            let r2 = (std::ops::Bound::Included(6), std::ops::Bound::Included(10));
            assert_eq!(range_intersection(r1, r2), None);
        }

        // Empty range
        {
            let r1 = (std::ops::Bound::Included(1), std::ops::Bound::Included(5));
            let r2 = (std::ops::Bound::Included(5), std::ops::Bound::Excluded(5));
            assert_eq!(range_intersection(r1, r2), None);
        }

        // Edge case: Excluded bounds
        {
            let r1 = (std::ops::Bound::Excluded(1), std::ops::Bound::Included(5));
            let r2 = (std::ops::Bound::Excluded(1), std::ops::Bound::Excluded(5));
            let expected = (std::ops::Bound::Excluded(1), std::ops::Bound::Excluded(5));
            assert_eq!(range_intersection(r1, r2), Some(expected));
        }
    }
}
