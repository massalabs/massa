/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusEvent {
    /// probable desynchronization detected, need re-synchronization
    NeedSync,
    /// Network is ended should be send after `end_timestamp`
    Stop,
}
