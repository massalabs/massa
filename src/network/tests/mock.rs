//TODO

/*
use super::super::config::NetworkConfig;
use super::super::network_controller::{NetworkController, ConnectionClosureReason, ConnectionId, NetworkEvent};
use std::net::IpAddr;
use tokio::sync::{mpsc, oneshot};

use std::error::Error;
type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug)]
pub enum NetworkControllerMockEvent {
    Stop,
    MergeAdvertisedPeerList(Vec<IpAddr>),
    GetAdvertisablePeerList(oneshot::Sender<Vec<IpAddr>>),
    ConnectionClosed((ConnectionId, ConnectionClosureReason)),
    ConnectionAlive(ConnectionId),
}

pub struct NetworkControllerMock {
    cfg: NetworkConfig,
    event_rx: Option<mpsc::Receiver<NetworkEvent>>,
    mock_event_tx: Option<mpsc::Sender<NetworkControllerMockEvent>>,
}

impl NetworkControllerMock {
    pub fn set_mock_channels(
        &mut self,
        event_rx: mpsc::Receiver<NetworkEvent>,
        mock_event_tx: mpsc::Sender<NetworkControllerMockEvent>,
    ) {
        self.event_rx = Some(event_rx);
        self.mock_event_tx = Some(mock_event_tx);
    }

    pub async fn new(cfg: &NetworkConfig) -> Self {
        Ok(NetworkControllerMock {
            cfg: cfg.clone(),
            event_rx: None,
            mock_event_tx: None,
        })
    }
}

impl NetworkController for NetworkControllerMock {

    #[allow(unused_mut)]
    pub async fn stop(mut self) {
        self.mock_event_tx
            .as_ref()
            .unwrap()
            .send(NetworkControllerMockEvent::Stop)
            .await
            .expect("could not send mock event");
    }

    pub async fn wait_event(&mut self) -> NetworkEvent {
        self.event_rx
            .as_mut()
            .unwrap()
            .recv()
            .await
            .expect("failed retrieving event")
    }

    pub async fn merge_advertised_peer_list(&mut self, ips: Vec<IpAddr>) {
        self.mock_event_tx
            .as_ref()
            .unwrap()
            .send(NetworkControllerMockEvent::MergeAdvertisedPeerList(ips))
            .await
            .expect("could not send mock event");
    }

    pub async fn get_advertisable_peer_list(&mut self) -> Vec<IpAddr> {
        let (response_tx, response_rx) = oneshot::channel::<Vec<IpAddr>>();
        self.mock_event_tx
            .as_ref()
            .unwrap()
            .send(NetworkControllerMockEvent::GetAdvertisablePeerList(
                response_tx,
            ))
            .await
            .expect("could not send mock event");
        response_rx.await.expect("could not get mock response")
    }

    pub async fn connection_closed(&mut self, id: ConnectionId, reason: ConnectionClosureReason) {
        self.mock_event_tx
            .as_ref()
            .unwrap()
            .send(NetworkControllerMockEvent::ConnectionClosed((id, reason)))
            .await
            .expect("could not send mock event");
    }

    pub async fn connection_alive(&mut self, id: ConnectionId) {
        self.mock_event_tx
            .as_ref()
            .unwrap()
            .send(NetworkControllerMockEvent::ConnectionAlive(id))
            .await
            .expect("could not send mock event");
    }
}
*/
