use super::{
    binders::{ReadBinder, WriteBinder},
    messages::Message,
    protocol_controller::NodeId,
};
use crate::{network::network_controller::NetworkController, CommunicationError};
use crypto::{
    signature::PrivateKey,
    {hash::Hash, signature::SignatureEngine},
};
use futures::future::try_join;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use time::UTime;
use tokio::time::timeout;

pub type HandshakeReturnType<NetworkControllerT> = Result<
    (
        NodeId,
        ReadBinder<<NetworkControllerT as NetworkController>::ReaderT>,
        WriteBinder<<NetworkControllerT as NetworkController>::WriterT>,
    ),
    CommunicationError,
>;

pub struct HandshakeWorker<NetworkControllerT: NetworkController> {
    reader: ReadBinder<NetworkControllerT::ReaderT>,
    writer: WriteBinder<NetworkControllerT::WriterT>,
    self_node_id: NodeId,
    private_key: PrivateKey,
    timeout_duration: UTime,
}

impl<NetworkControllerT: NetworkController> HandshakeWorker<NetworkControllerT> {
    pub fn new(
        socket_reader: NetworkControllerT::ReaderT,
        socket_writer: NetworkControllerT::WriterT,
        self_node_id: NodeId,
        private_key: PrivateKey,
        timeout_duration: UTime,
    ) -> HandshakeWorker<NetworkControllerT> {
        HandshakeWorker {
            reader: ReadBinder::new(socket_reader),
            writer: WriteBinder::new(socket_writer),
            self_node_id,
            private_key,
            timeout_duration,
        }
    }

    /// Manages one on going handshake
    /// consumes self
    /// Will not panic
    /// Returns a tuple (ConnectionId, Result)
    /// Creates the binders to communicate with that node
    pub async fn run(mut self) -> HandshakeReturnType<NetworkControllerT> {
        // generate random bytes
        let mut self_random_bytes = [0u8; 32];
        StdRng::from_entropy().fill_bytes(&mut self_random_bytes);
        let self_random_hash = Hash::hash(&self_random_bytes);
        // send handshake init future
        let send_init_msg = Message::HandshakeInitiation {
            public_key: self.self_node_id.0,
            random_bytes: self_random_bytes.clone(),
        };
        let send_init_fut = self.writer.send(&send_init_msg);

        // receive handshake init future
        let recv_init_fut = self.reader.next();

        // join send_init_fut and recv_init_fut with a timeout, and match result
        let (other_node_id, other_random_bytes) = match timeout(
            self.timeout_duration.to_duration(),
            try_join(send_init_fut, recv_init_fut),
        )
        .await
        {
            Err(_) => return Err(CommunicationError::HandshakeTimeoutError),
            Ok(Err(e)) => return Err(e),
            Ok(Ok((_, None))) => return Err(CommunicationError::HandshakeInterruptionError),
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeInitiation {
                    public_key: pk,
                    random_bytes: rb,
                } => (NodeId(pk), rb),
                _ => return Err(CommunicationError::HandshakeWrongMessageError),
            },
        };

        // check if remote node ID is the same as ours
        if other_node_id == self.self_node_id {
            return Err(CommunicationError::HandshakeKeyError);
        }

        // sign their random bytes
        let signature_engine = SignatureEngine::new();
        let other_random_hash = Hash::hash(&other_random_bytes);
        let self_signature = signature_engine.sign(&other_random_hash, &self.private_key)?;

        // send handshake reply future
        let send_reply_msg = Message::HandshakeReply {
            signature: self_signature,
        };
        let send_reply_fut = self.writer.send(&send_reply_msg);

        // receive handshake reply future
        let recv_reply_fut = self.reader.next();

        // join send_reply_fut and recv_reply_fut with a timeout, and match result
        let other_signature = match timeout(
            self.timeout_duration.to_duration(),
            try_join(send_reply_fut, recv_reply_fut),
        )
        .await
        {
            Err(_) => return Err(CommunicationError::HandshakeTimeoutError),
            Ok(Err(e)) => return Err(e),
            Ok(Ok((_, None))) => return Err(CommunicationError::HandshakeInterruptionError),
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeReply { signature: sig } => sig,
                _ => return Err(CommunicationError::HandshakeWrongMessageError),
            },
        };

        // check their signature
        if !signature_engine.verify(&self_random_hash, &other_signature, &other_node_id.0)? {
            return Err(CommunicationError::HandshakeInvalidSignatureError);
        }

        Ok((other_node_id, self.reader, self.writer))
    }
}
