/// Events that are emitted by graph.
#[derive(Debug, Clone)]
pub enum GraphEvent {
    /// probable desynchronization detected, need re-synchronization
    NeedSync,
}
