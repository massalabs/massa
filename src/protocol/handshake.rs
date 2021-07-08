use super::{
    binders::{ReadBinder, WriteBinder},
    messages::Message,
    node_controller::NodeId,
};
use crate::{
    crypto::signature::PrivateKey, crypto::signature::SignatureEngine,
    network::network_controller::ConnectionId,
};
use futures::future::try_join;
use rand::{rngs::StdRng, FromEntropy, RngCore};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

pub type HandshakeReturnType = (
    ConnectionId,
    Result<
        (
            NodeId,
            ReadBinder<OwnedReadHalf>,
            WriteBinder<OwnedWriteHalf>,
        ),
        String,
    >,
);

/// This function is lauched in a new task
/// It manages one on going handshake
/// Will not panic
/// Returns a tuple (ConnectionId, Result)
/// Creates the binders to communicate with that node
pub async fn handshake_fn(
    connection_id: ConnectionId,
    socket: TcpStream,
    self_node_id: NodeId, // NodeId.0 is our PublicKey
    private_key: PrivateKey,
    timeout_duration: Duration,
) -> HandshakeReturnType {
    // split socket, bind reader and writer
    let (socket_reader, socket_writer) = socket.into_split();
    let (mut reader, mut writer) = (
        ReadBinder::new(socket_reader),
        WriteBinder::new(socket_writer),
    );

    // generate random bytes
    let mut self_random_bytes = vec![0u8; 64];
    StdRng::from_entropy().fill_bytes(&mut self_random_bytes);

    // send handshake init future
    let send_init_msg = Message::HandshakeInitiation {
        public_key: self_node_id.0,
        random_bytes: self_random_bytes.clone(),
    };
    let send_init_fut = writer.send(&send_init_msg);

    // receive handshake init future
    let recv_init_fut = reader.next();

    // join send_init_fut and recv_init_fut with a timeout, and match result
    let (other_node_id, other_random_bytes) =
        match timeout(timeout_duration, try_join(send_init_fut, recv_init_fut)).await {
            Err(_) => return (connection_id, Err("handshake init timed out".into())),
            Ok(Err(_)) => return (connection_id, Err("handshake init r/w failed".into())),
            Ok(Ok((_, None))) => return (connection_id, Err("handshake init stopped".into())),
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeInitiation {
                    public_key: pk,
                    random_bytes: rb,
                } => (NodeId(pk), rb),
                _ => return (connection_id, Err("handshake init wrong message".into())),
            },
        };

    // check if remote node ID is the same as ours
    if other_node_id == self_node_id {
        return (
            connection_id,
            Err("handshake announced own public key".into()),
        );
    }

    // sign their random bytes
    let signature_engine = SignatureEngine::new();
    let self_signature = signature_engine.sign(&other_random_bytes, &private_key);

    // send handshake reply future
    let send_reply_msg = Message::HandshakeReply {
        signature: self_signature,
    };
    let send_reply_fut = writer.send(&send_reply_msg);

    // receive handshake reply future
    let recv_reply_fut = reader.next();

    // join send_reply_fut and recv_reply_fut with a timeout, and match result
    let other_signature =
        match timeout(timeout_duration, try_join(send_reply_fut, recv_reply_fut)).await {
            Err(_) => return (connection_id, Err("handshake reply timed out".into())),
            Ok(Err(_)) => return (connection_id, Err("handshake reply r/w failed".into())),
            Ok(Ok((_, None))) => return (connection_id, Err("handshake reply stopped".into())),
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeReply { signature: sig } => sig,
                _ => return (connection_id, Err("handshake reply wrong message".into())),
            },
        };

    // check their signature
    if !signature_engine.verify(&self_random_bytes, &other_signature, &other_node_id.0) {
        return (
            connection_id,
            Err("invalid remote handshake signature".into()),
        );
    }

    (connection_id, Ok((other_node_id, reader, writer)))
}
