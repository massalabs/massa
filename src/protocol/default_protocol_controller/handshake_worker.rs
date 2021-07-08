use super::super::{
    binders::{ReadBinder, WriteBinder},
    messages::Message,
    protocol_controller::NodeId,
};
use crate::{
    crypto::signature::PrivateKey,
    crypto::signature::SignatureEngine,
    network::network_controller::{ConnectionId, NetworkController},
};
use futures::future::try_join;
use rand::{rngs::StdRng, FromEntropy, RngCore};
use tokio::time::{timeout, Duration};

pub type HandshakeReturnType<NetworkControllerT> = (
    ConnectionId,
    Result<
        (
            NodeId,
            ReadBinder<<NetworkControllerT as NetworkController>::ReaderT>,
            WriteBinder<<NetworkControllerT as NetworkController>::WriterT>,
        ),
        String,
    >,
);

pub struct HandshakeWorker<NetworkControllerT: NetworkController> {
    connection_id: ConnectionId,
    reader: ReadBinder<NetworkControllerT::ReaderT>,
    writer: WriteBinder<NetworkControllerT::WriterT>,
    self_node_id: NodeId,
    private_key: PrivateKey,
    timeout_duration: Duration,
}

impl<NetworkControllerT: NetworkController> HandshakeWorker<NetworkControllerT> {
    pub fn new(
        connection_id: ConnectionId,
        socket_reader: NetworkControllerT::ReaderT,
        socket_writer: NetworkControllerT::WriterT,
        self_node_id: NodeId,
        private_key: PrivateKey,
        timeout_duration: Duration,
    ) -> HandshakeWorker<NetworkControllerT> {
        HandshakeWorker {
            connection_id,
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
        let mut self_random_bytes = vec![0u8; 64];
        StdRng::from_entropy().fill_bytes(&mut self_random_bytes);

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
            self.timeout_duration,
            try_join(send_init_fut, recv_init_fut),
        )
        .await
        {
            Err(_) => return (self.connection_id, Err("handshake init timed out".into())),
            Ok(Err(_)) => return (self.connection_id, Err("handshake init r/w failed".into())),
            Ok(Ok((_, None))) => return (self.connection_id, Err("handshake init stopped".into())),
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeInitiation {
                    public_key: pk,
                    random_bytes: rb,
                } => (NodeId(pk), rb),
                _ => {
                    return (
                        self.connection_id,
                        Err("handshake init wrong message".into()),
                    )
                }
            },
        };

        // check if remote node ID is the same as ours
        if other_node_id == self.self_node_id {
            return (
                self.connection_id,
                Err("handshake announced own public key".into()),
            );
        }

        // sign their random bytes
        let signature_engine = SignatureEngine::new();
        let self_signature = signature_engine.sign(&other_random_bytes, &self.private_key);

        // send handshake reply future
        let send_reply_msg = Message::HandshakeReply {
            signature: self_signature,
        };
        let send_reply_fut = self.writer.send(&send_reply_msg);

        // receive handshake reply future
        let recv_reply_fut = self.reader.next();

        // join send_reply_fut and recv_reply_fut with a timeout, and match result
        let other_signature = match timeout(
            self.timeout_duration,
            try_join(send_reply_fut, recv_reply_fut),
        )
        .await
        {
            Err(_) => return (self.connection_id, Err("handshake reply timed out".into())),
            Ok(Err(_)) => return (self.connection_id, Err("handshake reply r/w failed".into())),
            Ok(Ok((_, None))) => {
                return (self.connection_id, Err("handshake reply stopped".into()))
            }
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeReply { signature: sig } => sig,
                _ => {
                    return (
                        self.connection_id,
                        Err("handshake reply wrong message".into()),
                    )
                }
            },
        };

        // check their signature
        if !signature_engine.verify(&self_random_bytes, &other_signature, &other_node_id.0) {
            return (
                self.connection_id,
                Err("invalid remote handshake signature".into()),
            );
        }

        (
            self.connection_id,
            Ok((other_node_id, self.reader, self.writer)),
        )
    }
}
