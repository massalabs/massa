use massa_graph_2_exports::GraphManager;
use std::{sync::mpsc::SyncSender, thread::JoinHandle};
use tracing::log::info;

use crate::commands::GraphCommand;

pub struct GraphManagerImpl {
    pub thread_graph: Option<JoinHandle<()>>,
    pub graph_command_sender: SyncSender<GraphCommand>,
}

impl GraphManager for GraphManagerImpl {
    fn stop(&mut self) {
        info!("stopping graph worker...");
        //TODO: Stop graph command sender
        //drop(self.graph_command_sender);
        // join the graph thread
        if let Some(join_handle) = self.thread_graph.take() {
            join_handle
                .join()
                .expect("graph thread panicked on try to join");
        }
        info!("graph worker stopped");
    }
}
