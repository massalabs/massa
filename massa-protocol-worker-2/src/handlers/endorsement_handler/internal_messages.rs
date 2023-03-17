use massa_models::endorsement::SecureShareEndorsement;
use peernet::peer_id::PeerId;

pub enum InternalMessage {
    /// (From peer id, endorsements)
    PropagateEndorsements((PeerId, Vec<SecureShareEndorsement>)),
}
