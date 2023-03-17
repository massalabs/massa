use massa_storage::Storage;

/// Commands that the operations handler can process
#[derive(Debug)]
pub enum OperationHandlerCommand {
    /// Propagate operations (send batches)
    /// note: `Set<OperationId>` are replaced with `OperationPrefixIds`
    ///       by the controller
    PropagateOperations(Storage),
}
