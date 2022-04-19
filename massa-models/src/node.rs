// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_signature::PublicKey;
use serde::{Deserialize, Serialize};

use crate::ModelsError;

const NODE_ID_STRING_PREFIX: &str = "NOD";
/// `NodeId` wraps a public key to uniquely identify a node.
#[derive(Clone, Copy, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct NodeId(pub PublicKey);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", NODE_ID_STRING_PREFIX, self.0)
        } else {
            write!(f, "{}", self.0)
        }
    }
}

impl std::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", NODE_ID_STRING_PREFIX, self.0)
        } else {
            write!(f, "{}", self.0)
        }
    }
}

impl std::str::FromStr for NodeId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                Ok(NodeId(PublicKey::from_bs58_check(s)?))
            } else if v[0] != NODE_ID_STRING_PREFIX {
                Err(ModelsError::WrongPrefix(
                    NODE_ID_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                Ok(NodeId(PublicKey::from_bs58_check(v[1])?))
            }
        } else {
            Ok(NodeId(PublicKey::from_bs58_check(s)?))
        }
    }
}
