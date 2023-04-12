use peernet::peer_id::PeerId;

pub enum InternalMessage {
    /// (From peer id (optional, can come from API or other modules)), endorsements)
    /// TODO: Add data
    PropagateOperations((Option<PeerId>, ())),
}
