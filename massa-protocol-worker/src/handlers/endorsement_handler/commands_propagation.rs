use massa_storage::Storage;

pub enum EndorsementHandlerPropagationCommand {
    Stop,
    // Storage that contains endorsements to propagate
    PropagateEndorsements(Storage),
}
