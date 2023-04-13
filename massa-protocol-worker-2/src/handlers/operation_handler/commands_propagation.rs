use massa_models::{operation::OperationId, prehash::PreHashSet};

#[derive(Clone)]
pub enum OperationHandlerCommand {
    /// operations ids
    AnnounceOperations(PreHashSet<OperationId>),
}
