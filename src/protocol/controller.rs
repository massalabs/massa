use tokio::net::TcpStream;
use super::binders::{ReadBinder, WriteBinder};
use super::messages::Message;

use crate::network::controller::NetworkController;


pub struct ProtocolController {


}

async fn protocol_controller_fn() {
    
}

/*
pub struct PeerController<PeerId> {
    
}

pub struct PeerControllerNetworkInterface {
    /*
        TODO all
    */
}

async fn peer_controller_fn<PeerId>(
    peer_id: PeerId,
    socket: &mut TcpStream,
    network_tx: 
) {
    let (mut socket_reader, mut socket_writer) = socket.split();
    let (reader, writer) = (ReadBinder::new(&mut socket_reader), WriteBinder::new(&mut socket_writer));
    
}
*/