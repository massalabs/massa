use massa_storage::Storage;

#[derive(Clone)]
pub enum OperationHandlerPropagationCommand {
    Stop,
    /// operations ids
    PropagateOperations(Storage),
}
