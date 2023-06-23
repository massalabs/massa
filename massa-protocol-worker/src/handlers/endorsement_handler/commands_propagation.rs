use massa_storage::Storage;

pub enum EndorsementHandlerPropagationCommand {
    Stop,
    // Storage that contains endorsements to propagate
    PropagateEndorsements(Storage),
}

impl Clone for EndorsementHandlerPropagationCommand {
    fn clone(&self) -> Self {
        match self {
            EndorsementHandlerPropagationCommand::Stop => {
                EndorsementHandlerPropagationCommand::Stop
            }
            EndorsementHandlerPropagationCommand::PropagateEndorsements(storage) => {
                EndorsementHandlerPropagationCommand::PropagateEndorsements(
                    storage.clone("protocol".into()),
                )
            }
        }
    }
}
