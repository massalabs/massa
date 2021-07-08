use serde::{Deserialize, Serialize};

/// A unique connecion id for a node
#[derive(Default, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ConnectionId(pub u64);

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::fmt::Debug for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum ConnectionClosureReason {
    /// Connection was closed properly
    Normal,
    /// Connection failed for some reason
    Failed,
    /// Connection closed after node ban
    Banned,
}
