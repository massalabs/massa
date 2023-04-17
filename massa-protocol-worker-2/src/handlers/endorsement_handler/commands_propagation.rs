use massa_storage::Storage;

pub enum EndorsementHandlerCommand {
    // Storage that contains endorsements to propagate
    PropagateEndorsements(Storage),
}
