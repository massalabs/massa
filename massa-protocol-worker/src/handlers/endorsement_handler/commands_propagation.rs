use massa_storage::Storage;

#[derive(Clone)]
pub enum EndorsementHandlerPropagationCommand {
    Stop,
    // Storage that contains endorsements to propagate
    PropagateEndorsements(Storage),
}
