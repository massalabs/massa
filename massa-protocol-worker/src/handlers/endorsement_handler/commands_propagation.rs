use massa_storage::Storage;

pub(crate)  enum EndorsementHandlerPropagationCommand {
    Stop,
    // Storage that contains endorsements to propagate
    PropagateEndorsements(Storage),
}
