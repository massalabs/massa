// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use massa_serialization::{
    DeserializeError, Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_signature::PublicKey;
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::ops::Bound::Included;

/// `NodeId` wraps a public key to uniquely identify a node.
#[derive(
    Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, SerializeDisplay, DeserializeFromStr,
)]
pub(crate)  struct NodeId(PublicKey);

const NODEID_PREFIX: char = 'N';
const NODEID_VERSION: u64 = 0;

impl NodeId {
    /// Create a new `NodeId` from a public key.
    pub(crate)  fn new(public_key: PublicKey) -> Self {
        Self(public_key)
    }

    /// Get the public key of the `NodeId`.
    pub(crate)  fn get_public_key(&self) -> PublicKey {
        self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        // might want to allocate the vector with capacity in order to avoid re-allocation
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer
            .serialize(&NODEID_VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.0.to_bytes());
        write!(
            f,
            "{}{}",
            NODEID_PREFIX,
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

impl std::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl std::str::FromStr for NodeId {
    type Err = ModelsError;
    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use std::str::FromStr;
    /// # use massa_models::node::NodeId;
    /// # let keypair = KeyPair::generate();
    /// # let node_id = NodeId::new(keypair.get_public_key());
    /// let ser = node_id.to_string();
    /// let res_node_id = NodeId::from_str(&ser).unwrap();
    /// assert_eq!(node_id, res_node_id);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == NODEID_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check = bs58::decode(data)
                    .with_check(None)
                    .into_vec()
                    .map_err(|_| ModelsError::NodeIdParseError)?;
                let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
                let (rest, _version) = u64_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::NodeIdParseError)?;
                Ok(NodeId(PublicKey::from_bytes(
                    rest.try_into().map_err(|_| ModelsError::NodeIdParseError)?,
                )?))
            }
            _ => Err(ModelsError::NodeIdParseError),
        }
    }
}
