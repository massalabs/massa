// Copyright (c) 2022 MASSA LABS <info@massa.net>

use serde::{Deserialize, Serialize};

/// A unique connection id for a node
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
pub struct ConnectionId(pub u64);

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConnectionClosureReason {
    /// Connection was closed properly
    Normal,
    /// Connection failed for some reason
    Failed,
    /// Connection closed after node ban
    Banned,
}
