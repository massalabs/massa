use massa_models::{operation::OperationId, prehash::PreHashSet};

#[derive(Clone)]
pub enum OperationHandlerPropagationCommand {
    Stop,
    /// operations ids
    AnnounceOperations(PreHashSet<OperationId>),
}
