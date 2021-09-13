// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crypto::signature::PublicKey;
use serde::{Deserialize, Serialize};

/// NodeId wraps a public key to uniquely identify a node.
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub PublicKey);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
