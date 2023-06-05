// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use massa_signature::PublicKey;
use serde_with::{DeserializeFromStr, SerializeDisplay};

/// `NodeId` wraps a public key to uniquely identify a node.
#[derive(
    Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, SerializeDisplay, DeserializeFromStr,
)]
pub struct NodeId(PublicKey);

const NODEID_PREFIX: char = 'N';

impl NodeId {
    /// Create a new `NodeId` from a public key.
    pub fn new(public_key: PublicKey) -> Self {
        Self(public_key)
    }

    /// Get the public key of the `NodeId`.
    pub fn get_public_key(&self) -> PublicKey {
        self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            NODEID_PREFIX,
            bs58::encode(self.0.to_bytes()).with_check().into_string()
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
    /// # let keypair = KeyPair::generate(0).unwrap();
    /// # let node_id = NodeId::new(keypair.get_public_key());
    /// let ser = node_id.to_string();
    /// let res_node_id = NodeId::from_str(&ser).unwrap();
    /// let from_raw = NodeId::from_str("N12UbyLJDS7zimGWf3LTHe8hYY67RdLke1iDRZqJbQQLHQSKPW8j").unwrap();
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
                Ok(NodeId(PublicKey::from_bytes(&decoded_bs58_check)?))
            }
            _ => Err(ModelsError::NodeIdParseError),
        }
    }
}
