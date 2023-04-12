use massa_models::{operation::OperationId, prehash::PreHashSet};

#[derive(Clone)]
pub enum OperationHandlerCommand {
    /// (From peer id (optional, can come from API or other modules)), operations ids)
    AnnounceOperations(PreHashSet<OperationId>),
}
