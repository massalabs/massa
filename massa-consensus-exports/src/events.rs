/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusEvent {
    NeedSync, // probable desync detected, need resync
}
