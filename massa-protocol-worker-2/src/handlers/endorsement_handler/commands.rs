use massa_storage::Storage;

/// Commands that the endorsement handler can process
#[derive(Debug)]
pub enum EndorsementHandlerCommand {
    /// Propagate endorsements
    PropagateEndorsements(Storage),
}
