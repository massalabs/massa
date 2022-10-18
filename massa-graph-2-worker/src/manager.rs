use massa_graph_2_exports::GraphManager;
use std::{sync::mpsc::SyncSender, thread::JoinHandle};
use tracing::log::info;

use crate::commands::GraphCommand;

pub struct GraphManagerImpl {
    pub thread_graph: Option<(SyncSender<GraphCommand>, JoinHandle<()>)>,
}

impl GraphManager for GraphManagerImpl {
    fn stop(&mut self) {
        info!("stopping graph worker...");
        // join the graph thread
        if let Some((tx, join_handle)) = self.thread_graph.take() {
            drop(tx);
            join_handle
                .join()
                .expect("graph thread panicked on try to join");
        }
        info!("graph worker stopped");
    }
}
