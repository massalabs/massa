mod config;
mod error;

#[macro_use]
extern crate logging;

pub use config::HangMonitorConfig;

use config::CHANNEL_SIZE;
use error::HangMonitorError;
use std::collections::HashMap;
use std::time::Instant;
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum MonitoredComponentId {
    Consensus,
    Network,
    Node,
    Protocol,
}

#[derive(Debug)]
pub enum ConsensusHangAnnotation {
    Slot,
    GetBlockGraphStatus,
    GetActiveBlock,
    GetSelectionDraws,
    GetBootGraph,
    ReceivedBlock,
    ReceivedBlockHeader,
    ReceivedTransaction,
    GetBlock,
}

#[derive(Debug)]
pub enum NetworkHangAnnotation {
    HandshakeFinished,
    ReceivedAskedPeerList,
    ReceivedPeerList,
    ReceivedBlock,
    ReceivedAskForBlock,
    ReceivedBlockHeader,
    ReceivedClosed,
    ReceivedBlockNotFound,
    ConnectionClosed,
    MissingNodeError,
    AskedPeerList,
    CommandBlockNotFound,
    Ban,
    SendBlockHeader,
    AskForBlock,
    SendBlock,
    GetPeers,
    NodeEventBlockNotFound,
    OutConnection,
    InConnection,
    NewConnection,
    ManageOutConnections,
    ManageInConnections,
}

#[derive(Debug)]
pub enum NodeHangAnnotation {
    Block,
    BlockHeader,
    AskForBlock,
    PeerList,
    AskPeerList,
    BlockNotFound,
    HandshakeInitiation,
    HandshakeReply,
    SendPeerList,
    SendBlock,
    SendBlockHeader,
    CommandAskForBlock,
    Close,
    CommandBlockNotFound,
    SendAskPeerList,
    Closed,
}

#[derive(Debug)]
pub enum ProtocolHangAnnotation {
    IntegratedBlock,
    AttackBlockDetected,
    FoundBlock,
    WishlistDelta,
    NetworkEventBlockNotFound,
    NewConnection,
    ConnectionClosed,
    ReceivedBlock,
    AskedForBlock,
    ReceivedBlockHeader,
    CommandBlockNotFound,
    BlockNotFound,
}

#[derive(Debug)]
pub enum HangAnnotation {
    Consensus(ConsensusHangAnnotation),
    Network(NetworkHangAnnotation),
    Node(NodeHangAnnotation),
    Protocol(ProtocolHangAnnotation),
}

#[derive(Debug)]
pub enum HangMonitorCommand {
    Register(MonitoredComponentId),
    Unregister(MonitoredComponentId),
    NotifyActivity(MonitoredComponentId, HangAnnotation),
    NotifyWait(MonitoredComponentId),
}

#[derive(Debug)]
pub enum HangMonitorManagementCommand {}

pub fn start_hang_monitor_controller(
    cfg: HangMonitorConfig,
) -> Result<(Option<HangMonitorCommandSender>, Option<HangMonitorManager>), HangMonitorError> {
    if !cfg.monitor {
        return Ok((None, None));
    }

    debug!("starting hang-monitor controller");
    massa_trace!("start", {});

    // start worker
    let (command_tx, command_rx) = mpsc::channel::<HangMonitorCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<HangMonitorManagementCommand>(1);
    let join_handle = tokio::spawn(async move {
        let res = HangMonitorWorker::new(cfg, command_rx, manager_rx)?
            .run_loop()
            .await;
        match res {
            Err(err) => {
                error!("hang-monitor worker crashed: {:?}", err);
                Err(err)
            }
            Ok(v) => {
                info!("hang-monitor worker finished cleanly");
                Ok(v)
            }
        }
    });
    let command_sender = HangMonitorCommandSender::new(command_tx);
    let manager = HangMonitorManager {
        manager_tx,
        join_handle,
    };
    Ok((Some(command_sender), Some(manager)))
}

pub struct HangMonitorCommandSender {
    sender: mpsc::Sender<HangMonitorCommand>,
    monitored_component: Option<MonitoredComponentId>,
}

impl Clone for HangMonitorCommandSender {
    fn clone(&self) -> Self {
        HangMonitorCommandSender {
            sender: self.sender.clone(),
            // The clone needs to register it's own component.
            monitored_component: None,
        }
    }
}

impl HangMonitorCommandSender {
    fn new(sender: mpsc::Sender<HangMonitorCommand>) -> Self {
        HangMonitorCommandSender {
            sender,
            monitored_component: None,
        }
    }

    pub async fn register(
        &mut self,
        component: MonitoredComponentId,
    ) -> Result<(), HangMonitorError> {
        self.monitored_component = Some(component.clone());
        self.sender
            .send(HangMonitorCommand::Register(component))
            .await
            .map_err(|_| HangMonitorError::ChannelError("register command send error".into()))
    }

    pub async fn unregister(&mut self) -> Result<(), HangMonitorError> {
        let component = self
            .monitored_component
            .clone()
            .expect("No component registered for hang monitoring.");
        self.sender
            .send(HangMonitorCommand::Unregister(component))
            .await
            .map_err(|_| HangMonitorError::ChannelError("unregister command send error".into()))
    }

    pub async fn notify_activity(
        &mut self,
        annotation: HangAnnotation,
    ) -> Result<(), HangMonitorError> {
        let component = self
            .monitored_component
            .clone()
            .expect("No component registered for hang monitoring.");
        self.sender
            .send(HangMonitorCommand::NotifyActivity(component, annotation))
            .await
            .map_err(|_| {
                HangMonitorError::ChannelError("notify_activity command send error".into())
            })
    }

    pub async fn notify_wait(&mut self) -> Result<(), HangMonitorError> {
        let component = self
            .monitored_component
            .clone()
            .expect("No component registered for hang monitoring.");
        self.sender
            .send(HangMonitorCommand::NotifyWait(component))
            .await
            .map_err(|_| {
                HangMonitorError::ChannelError("notify_activity command send error".into())
            })
    }
}

pub struct HangMonitorManager {
    join_handle: JoinHandle<Result<(), HangMonitorError>>,
    manager_tx: mpsc::Sender<HangMonitorManagementCommand>,
}

impl HangMonitorManager {
    pub async fn stop(self) -> Result<(), HangMonitorError> {
        drop(self.manager_tx);
        self.join_handle.await??;
        Ok(())
    }
}

struct MonitoredComponent {
    last_activity: Instant,
    last_annotation: Option<HangAnnotation>,
    is_waiting: bool,
}

struct HangMonitorWorker {
    cfg: HangMonitorConfig,
    controller_command_rx: mpsc::Receiver<HangMonitorCommand>,
    controller_manager_rx: mpsc::Receiver<HangMonitorManagementCommand>,
    monitored_components: HashMap<MonitoredComponentId, MonitoredComponent>,
}

impl HangMonitorWorker {
    pub fn new(
        cfg: HangMonitorConfig,
        controller_command_rx: mpsc::Receiver<HangMonitorCommand>,
        controller_manager_rx: mpsc::Receiver<HangMonitorManagementCommand>,
    ) -> Result<HangMonitorWorker, HangMonitorError> {
        Ok(HangMonitorWorker {
            cfg,
            controller_command_rx,
            controller_manager_rx,
            monitored_components: Default::default(),
        })
    }

    pub async fn run_loop(mut self) -> Result<(), HangMonitorError> {
        let monitor_interval = sleep(self.cfg.monitor_interval.into());
        tokio::pin!(monitor_interval);
        loop {
            tokio::select! {
                _ = &mut monitor_interval => {
                    let now = Instant::now();
                    for (id, component) in self.monitored_components.iter().filter(|(_, component)| component.is_waiting) {
                        let is_hanging = now.duration_since(component.last_activity) > self.cfg.hang_timeout.to_duration();
                        if is_hanging {
                             debug!("Component {:?} is hanging on {:?}.", id, component.last_annotation);
                        }
                    }
                },

                Some(cmd) = self.controller_command_rx.recv() => self.process_command(cmd)?,

                cmd = self.controller_manager_rx.recv() => match cmd {
                    None => break,
                    Some(_) => {}
                },
            }
        }

        Ok(())
    }

    fn process_command(&mut self, cmd: HangMonitorCommand) -> Result<(), HangMonitorError> {
        match cmd {
            HangMonitorCommand::Register(id) => {
                self.monitored_components.insert(
                    id,
                    MonitoredComponent {
                        last_activity: Instant::now(),
                        last_annotation: None,
                        is_waiting: false,
                    },
                );
            }
            HangMonitorCommand::Unregister(id) => {
                self.monitored_components.remove(&id);
            }
            HangMonitorCommand::NotifyActivity(id, annotation) => {
                if let Some(component) = self.monitored_components.get_mut(&id) {
                    component.last_activity =  Instant::now();
                    component.last_annotation = Some(annotation);
                }
            }
            HangMonitorCommand::NotifyWait(id) => {
                if let Some(component) = self.monitored_components.get_mut(&id) {
                    component.is_waiting = true;
                }
            }
        }
        Ok(())
    }
}
