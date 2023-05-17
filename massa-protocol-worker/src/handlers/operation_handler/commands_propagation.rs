use massa_models::{operation::OperationId, prehash::PreHashSet};

#[derive(Clone)]
pub(crate) enum OperationHandlerPropagationCommand {
    Stop,
    /// operations ids
    AnnounceOperations(PreHashSet<OperationId>),
}
