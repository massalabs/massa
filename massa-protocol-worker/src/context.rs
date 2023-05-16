use massa_protocol_exports::PeerId;
use peernet::context::Context as PeernetContext;

pub struct Context {
    pub our_keypair: KeyPair,
}

impl PeernetContext for Context {
    fn get_peer_id(&self) -> PeerId {
        PeerId::from_public_key(self.our_keypair.get_public_key())
    }
}