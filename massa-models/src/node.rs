// Copyright (c) 2021 MASSA LABS <info@massa.net>

use massa_signature::PublicKey;
use serde::{Deserialize, Serialize};

/// NodeId wraps a public key to uniquely identify a node.
#[derive(Clone, Copy, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct NodeId(pub PublicKey);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0.to_bs58_check())
    }
}

impl std::str::FromStr for NodeId {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match PublicKey::from_bs58_check(s) {
            Ok(x) => Ok(NodeId(x)),
            Err(_) => Err(()),
        }
    }
}
